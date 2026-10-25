//! Bounded external joins and observed-domain enumeration.
use crate::{budget::Reservation, model::Bytes, source::Source, *};
use std::sync::Arc;

fn split(options: &SpillOptions) -> Result<SpillOptions> {
    if options.directory.as_os_str().len() > 4096 || options.directory.capacity() > 16 * 1024 {
        return Err(Error::new(
            ErrorKind::InvalidOptions,
            "scratch path exceeds bounded allowance",
        ));
    }
    let mut limits = options.clone();
    limits.maximum_temporary_bytes /= 3;
    limits.maximum_open_files /= 3;
    if !(2..=64).contains(&limits.merge_fan_in)
        || options.maximum_open_files <= options.merge_fan_in
        || limits.maximum_open_files < 3
    {
        return Err(Error::new(
            ErrorKind::InvalidOptions,
            "large lookup needs three fan-in-plus-output handle shares",
        ));
    }
    limits.merge_fan_in = limits.merge_fan_in.min(limits.maximum_open_files - 1);
    Ok(limits)
}
fn increment(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "large lookup counter overflow"))
}

pub struct LargeLabelLookup {
    reader: Reader,
    options: SpillOptions,
    requests: SpillSorter,
    count: u64,
    error: Option<Error>,
    _state: Reservation,
}
/// A borrowed result; requesting the next entry reuses label storage.
///
/// ```compile_fail,E0499
/// # use gdxrs::{LargeLabelCursor, Result};
/// fn retain(cursor: &mut LargeLabelCursor) -> Result<()> {
///     let first = cursor.next()?.unwrap();
///     let second = cursor.next()?;
///     println!("{:?}", first.label);
///     Ok(())
/// }
/// ```
pub struct LargeResolvedLabel<'a> {
    pub ordinal: u64,
    pub label: &'a [u8],
    pub id: Option<UelId>,
}
pub struct LargeLabelCursor {
    source: Arc<Source>,
    requests: SpillCursor,
    labels: Option<SpillCursor>,
    head: Option<(Bytes, UelId)>,
    current: Bytes,
    error: Option<Error>,
    done: bool,
    _state: Reservation,
}
impl Reader {
    /// Start a request stream. Limits are divided among three concurrent sessions.
    pub fn large_label_lookup(&self, options: &SpillOptions) -> Result<LargeLabelLookup> {
        if std::mem::size_of::<LargeLabelLookup>() > 16 * 1024
            || std::mem::size_of::<LargeLabelCursor>() > 16 * 1024
        {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "large lookup controls exceed allowance",
            ));
        }
        let state = self.source.budget.reserve(16 * 1024)?;
        let limits = split(options)?;
        let requests = self.spill_sort(&limits)?;
        Ok(LargeLabelLookup {
            reader: self.clone(),
            options: limits,
            requests,
            count: 0,
            error: None,
            _state: state,
        })
    }
    /// Intersect known lookup IDs with a source/symbol-bound base selection.
    pub fn selection_from_large_labels(
        &self,
        symbol: &SymbolHandle,
        dimension: u8,
        base: &Selection,
        mut matches: LargeLabelCursor,
        options: &SpillOptions,
    ) -> Result<Selection> {
        let result = (|| {
            base.validate(self, symbol)?;
            if base.has_disk() {
                return Err(Error::new(
                    ErrorKind::InvalidSelection,
                    "base already has a disk clause",
                ));
            }
            if !Arc::ptr_eq(&self.source, &matches.source) {
                return Err(Error::new(
                    ErrorKind::SourceMismatch,
                    "lookup belongs to another source",
                ));
            }
            if dimension >= self.resolve(&symbol.inner.record)?.dimension {
                return Err(Error::new(
                    ErrorKind::InvalidSelection,
                    "membership dimension outside symbol",
                ));
            }
            if matches.done || matches.requests.statistics().returned_records != 0 {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "membership requires an unconsumed lookup cursor",
                ));
            }
            let limits = split(options)?;
            // Input streams must use the same resource shares as the output job.
            matches.check_limits(&limits)?;
            let mut sorter = self.spill_sort(&limits)?;
            let fill = (|| {
                while let Some(entry) = matches.next()? {
                    if let Some(id) = entry.id {
                        sorter.push(b"", u64::from(id.get()))?;
                    }
                }
                Ok(())
            })();
            if let Err(e) = fill {
                return Err(cancel_error(e, &mut sorter));
            }
            base.clone()
                .with_disk(dimension, sorter.finish()?.into_ids()?)
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => Err(matches.close_error(e)),
        }
    }
}
fn cancel_error(mut primary: Error, sorter: &mut SpillSorter) -> Error {
    if let Err(e) = sorter.cancel() {
        primary = primary.with_cleanup(std::io::Error::other(e));
    }
    primary
}
impl LargeLabelLookup {
    pub fn push(&mut self, label: &[u8]) -> Result<()> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let result = increment(self.count).and_then(|next| {
            self.requests.push(label, self.count)?;
            self.count = next;
            Ok(())
        });
        if let Err(e) = result {
            let e = cancel_error(e, &mut self.requests);
            self.error = Some(e.clone());
            return Err(e);
        }
        Ok(())
    }
    pub fn directory(&self) -> &std::path::Path {
        self.requests.directory()
    }
    pub fn cancel(&mut self) -> Result<SpillStatistics> {
        self.requests.cancel()
    }
    pub fn finish(self) -> Result<LargeLabelCursor> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let mut requests = self.requests.finish()?;
        let result = (|| {
            if self.count == 0 {
                return Ok(None);
            }
            let mut sorter = self.reader.spill_sort(&self.options)?;
            let fill = (|| {
                let mut labels = self.reader.uels()?;
                while let Some(entry) = labels.next()? {
                    sorter.push(entry.label, u64::from(entry.id.get()))?;
                }
                Ok(())
            })();
            if let Err(e) = fill {
                return Err(cancel_error(e, &mut sorter));
            }
            Ok(Some(sorter.finish()?))
        })();
        match result {
            Ok(labels) => Ok(LargeLabelCursor {
                source: Arc::clone(&self.reader.source),
                requests,
                labels,
                head: None,
                current: Bytes::default(),
                error: None,
                done: false,
                _state: self._state,
            }),
            Err(mut e) => {
                if let Err(cleanup) = requests.close() {
                    e = e.with_cleanup(std::io::Error::other(cleanup));
                }
                Err(e)
            }
        }
    }
}
impl LargeLabelCursor {
    pub fn directories(&self) -> (&std::path::Path, Option<&std::path::Path>) {
        (
            self.requests.directory(),
            self.labels.as_ref().map(SpillCursor::directory),
        )
    }
    pub fn is_complete(&self) -> bool {
        self.error.is_none()
            && self.requests.statistics().complete
            && self.labels.as_ref().is_none_or(|c| c.statistics().complete)
    }
    fn check_limits(&self, limits: &SpillOptions) -> Result<()> {
        if !self.requests.within_limits(limits)
            || self
                .labels
                .as_ref()
                .is_some_and(|c| !c.within_limits(limits))
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "membership limits differ from lookup shares",
            ));
        }
        Ok(())
    }
    fn close_error(&mut self, mut e: Error) -> Error {
        if let Err(cleanup) = self.close() {
            e = e.with_cleanup(std::io::Error::other(cleanup));
        }
        self.error = Some(e.clone());
        e
    }
    /// Return label-sorted results with original zero-based request ordinals.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<LargeResolvedLabel<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.done {
            return Ok(None);
        }
        let result = (|| {
            self.source.check_unchanged()?;
            let Some(request) = self.requests.next()? else {
                if let Some(labels) = &mut self.labels {
                    while labels.next()?.is_some() {}
                }
                self.done = true;
                return Ok(None);
            };
            self.current = Bytes::from_slice(request.label)?;
            let ordinal = request.id;
            if let Some(labels) = &mut self.labels {
                loop {
                    if self
                        .head
                        .as_ref()
                        .is_some_and(|(label, _)| label.as_slice() >= self.current.as_slice())
                    {
                        break;
                    }
                    self.head = match labels.next()? {
                        Some(entry) => Some((
                            Bytes::from_slice(entry.label)?,
                            UelId::new(u32::try_from(entry.id).map_err(|_| {
                                Error::new(ErrorKind::InvalidSpill, "UEL ID overflow")
                            })?)?,
                        )),
                        None => None,
                    };
                    if self.head.is_none() {
                        break;
                    }
                }
            }
            let id = self
                .head
                .as_ref()
                .filter(|(label, _)| label.as_slice() == self.current.as_slice())
                .map(|(_, id)| *id);
            Ok(Some((ordinal, id)))
        })();
        match result {
            Ok(Some((ordinal, id))) => Ok(Some(LargeResolvedLabel {
                ordinal,
                label: self.current.as_slice(),
                id,
            })),
            Ok(None) => Ok(None),
            Err(e) => Err(self.close_error(e)),
        }
    }
    /// Close both input streams; retryable after cleanup errors.
    pub fn close(&mut self) -> Result<()> {
        self.done = true;
        self.head = None;
        let a = self.requests.close().map(|_| ());
        let b = match &mut self.labels {
            Some(c) => c.close().map(|_| ()),
            None => Ok(()),
        };
        match (a, b) {
            (Err(a), Err(b)) => Err(a.with_cleanup(std::io::Error::other(b))),
            (Err(e), _) | (_, Err(e)) => Err(e),
            _ => Ok(()),
        }
    }
    pub fn statistics(&self) -> (SpillStatistics, Option<SpillStatistics>) {
        (
            self.requests.statistics(),
            self.labels.as_ref().map(SpillCursor::statistics),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainId {
    pub dimension: u8,
    pub id: UelId,
}
pub struct UniqueDomainCursor {
    cursor: SpillCursor,
    previous: Option<DomainId>,
    error: Option<Error>,
}
impl Reader {
    /// Enumerate observed IDs from exactly the accepted, limited selected rows.
    pub fn unique_domains(
        &self,
        symbol: &SymbolHandle,
        selection: &Selection,
        dimensions: &[u8],
        options: ScanOptions,
        spill: &SpillOptions,
    ) -> Result<UniqueDomainCursor> {
        if std::mem::size_of::<UniqueDomainCursor>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "domain cursor controls exceed allowance",
            ));
        }
        selection.validate(self, symbol)?;
        let dimension = self.resolve(&symbol.inner.record)?.dimension;
        let mut mask = 0u32;
        for &d in dimensions {
            if d >= dimension || mask & (1u32 << d) != 0 {
                return Err(Error::new(
                    ErrorKind::InvalidSelection,
                    "invalid or duplicate retained dimension",
                ));
            }
            mask |= 1u32 << d;
        }
        // Validate the caller's original projection even though extraction overrides it.
        drop(self.scan_selected(symbol, selection, options.clone())?);
        let mut sorter = self.spill_sort(spill)?;
        let fill = (|| {
            if mask == 0 {
                return Ok(());
            }
            let mut cursor = self.scan_selected(
                symbol,
                selection,
                ScanOptions {
                    key_projection: KeyProjection::Mask(mask),
                    value_projection: ValueProjection::Mask(0),
                    ..options
                },
            )?;
            while let Some(batch) = cursor.next_batch()? {
                for &d in batch.key_dimensions() {
                    let keys = batch.key_column(d).ok_or_else(|| {
                        Error::new(ErrorKind::InvalidProjection, "missing domain column")
                    })?;
                    for id in keys {
                        sorter.push(&[d], u64::from(id.get()))?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = fill {
            return Err(cancel_error(e, &mut sorter));
        }
        Ok(UniqueDomainCursor {
            cursor: sorter.finish()?,
            previous: None,
            error: None,
        })
    }
}
impl UniqueDomainCursor {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<DomainId>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let result = (|| {
            while let Some(entry) = self.cursor.next()? {
                let &[dimension] = entry.label else {
                    return Err(Error::new(ErrorKind::InvalidSpill, "invalid domain frame"));
                };
                if dimension >= 20 {
                    return Err(Error::new(
                        ErrorKind::InvalidSpill,
                        "invalid domain dimension",
                    ));
                }
                let id = UelId::new(
                    u32::try_from(entry.id)
                        .map_err(|_| Error::new(ErrorKind::InvalidSpill, "domain ID overflow"))?,
                )?;
                let next = DomainId { dimension, id };
                if self.previous != Some(next) {
                    self.previous = Some(next);
                    return Ok(Some(next));
                }
            }
            Ok(None)
        })();
        match result {
            Err(mut e) => {
                if let Err(cleanup) = self.cursor.close() {
                    e = e.with_cleanup(std::io::Error::other(cleanup));
                }
                self.error = Some(e.clone());
                Err(e)
            }
            other => other,
        }
    }
    pub fn directory(&self) -> &std::path::Path {
        self.cursor.directory()
    }
    pub fn close(&mut self) -> Result<SpillStatistics> {
        self.cursor.close()
    }
    pub fn statistics(&self) -> SpillStatistics {
        self.cursor.statistics()
    }
}
