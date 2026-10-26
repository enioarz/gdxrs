//! Explicit session-local caller IDs, independent of on-disk one-based UEL IDs.
use crate::*;
use crate::{budget::Reservation, write::Metadata};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct MappedUelId(u32);
impl MappedUelId {
    /// Native caller maps permit zero; negative and wider-than-i32 IDs are invalid.
    pub fn new(value: i64) -> Result<Self> {
        if !(0..=i64::from(i32::MAX)).contains(&value) {
            return Err(Error::new(
                ErrorKind::InvalidReference,
                "mapped UEL ID must be in 0..=i32::MAX",
            ));
        }
        Ok(Self(value as u32))
    }
    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingMapping {
    Error,
    SkipRecord,
}
#[derive(Clone, Copy, Debug)]
pub struct MappedReadOptions {
    pub missing: MissingMapping,
    /// Applied after mapped ordering, never to the raw prescan.
    pub limit: Option<u64>,
    pub batch_bytes: usize,
}
impl Default for MappedReadOptions {
    fn default() -> Self {
        Self {
            missing: MissingMapping::Error,
            limit: None,
            batch_bytes: 4096,
        }
    }
}
/// Source-bound immutable sparse caller map; not persisted in GDX files.
/// Registration failures retain an abortable owner. Close reports cleanup errors.
pub struct MappedUels {
    reader: Reader,
    metadata: Metadata,
    initialized: bool,
    closed: bool,
    error: Option<Error>,
    _state: Reservation,
}
impl Reader {
    /// Create an empty map owner. Register explicit raw-to-caller pairs next.
    pub fn mapped_uels(&self, options: &MetadataOptions) -> Result<MappedUels> {
        if std::mem::size_of::<MappedUels>() > 16384 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "mapped owner controls exceed allowance",
            ));
        }
        let state = self.source.budget.reserve(16384)?;
        let metadata = Metadata::new(Arc::clone(&self.source.budget), self.source.id, options)?;
        Ok(MappedUels {
            reader: self.clone(),
            metadata,
            initialized: false,
            closed: false,
            error: None,
            _state: state,
        })
    }
    /// Explicit mapped-order operation: fully scan/spill before returning rows.
    /// Raw APIs retain their streaming behavior. Map and scan share one quota/root.
    pub fn scan_mapped<'a>(
        &self,
        symbol: &SymbolHandle,
        map: &'a mut MappedUels,
        options: MappedReadOptions,
    ) -> Result<MappedCursor<'a>> {
        map.ensure()?;
        self.check_handle(symbol)?;
        if options.batch_bytes == 0 || options.batch_bytes > self.source.options.maximum_batch_bytes
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "mapped batch cap outside reader maximum",
            ));
        }
        if self.source.id != map.reader.source.id {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "caller map belongs to another reader",
            ));
        }
        let state = self.source.budget.reserve(16384)?;
        let info = symbol.metadata()?;
        if std::mem::size_of::<MappedCursor<'_>>() > 16384 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "mapped cursor controls exceed allowance",
            ));
        }
        let dim = usize::from(info.dimension);
        let fields = info.kind.fields();
        let raw_max = self.source.counts()?.uels;
        let mut sort = map.metadata.record_sorter()?;
        let observe = sort.observer();
        let mut statistics = MappedScanStatistics::default();
        let result = (|| {
            if options.limit == Some(0) {
                return Ok(());
            }
            let mut scan_options = self.scan_options();
            scan_options.batch_bytes = options.batch_bytes;
            let mut scan = self.scan(symbol, scan_options)?;
            while let Some(batch) = scan.next_batch()? {
                for row in 0..batch.len() {
                    statistics.decoded = statistics.decoded.checked_add(1).ok_or_else(invalid)?;
                    let mut packet = [0u8; 225];
                    let mut missing = false;
                    for d in 0..dim {
                        let raw = batch
                            .key_column(d as u8)
                            .and_then(|c| c.get(row))
                            .copied()
                            .ok_or_else(invalid)?;
                        let Some(mapped) = map.metadata.map_lookup(raw.get(), false)? else {
                            missing = true;
                            break;
                        };
                        packet[d * 4..d * 4 + 4].copy_from_slice(&mapped.to_be_bytes());
                        packet[dim * 4 + d * 4..dim * 4 + d * 4 + 4]
                            .copy_from_slice(&raw.get().to_be_bytes());
                    }
                    if missing {
                        if options.missing == MissingMapping::Error {
                            return Err(Error::new(
                                ErrorKind::InvalidReference,
                                "record has an unmapped raw UEL",
                            ));
                        }
                        statistics.skipped =
                            statistics.skipped.checked_add(1).ok_or_else(invalid)?;
                        continue;
                    }
                    let mut values = [Value::ZERO; 5];
                    for (f, value) in values.iter_mut().enumerate().take(fields) {
                        *value = *batch
                            .value_column(ValueField::ALL[f])
                            .and_then(|v| v.get(row))
                            .ok_or_else(invalid)?;
                    }
                    let n = write::pack_values(&values[..fields], &mut packet[dim * 8..])?;
                    statistics.accepted = statistics
                        .accepted
                        .checked_add(1)
                        .filter(|n| *n <= i32::MAX as u64)
                        .ok_or_else(invalid)?;
                    sort.push(&packet[..dim * 8 + n], statistics.accepted)?;
                }
            }
            Ok(())
        })();
        if let Err(mut error) = result {
            if let Err(cleanup) = sort.cancel()
                && error.cleanup_error().is_none()
            {
                error = error.with_cleanup(std::io::Error::other(cleanup));
            }
            drop(sort);
            map.metadata.map_sort_closed(observe(), false);
            return Err(map.fail(error));
        }
        let cursor = match sort.finish() {
            Ok(cursor) => cursor,
            Err(e) => {
                map.metadata.map_sort_closed(observe(), false);
                return Err(map.fail(e));
            }
        };
        let one = UelId::new(1)?;
        Ok(MappedCursor {
            map,
            cursor: Some(cursor),
            dimensions: dim,
            fields,
            raw_max,
            raw: [one; 20],
            mapped: [MappedUelId(0); 20],
            values: [Value::ZERO; 5],
            previous: None,
            statistics,
            limit: options.limit,
            error: None,
            _state: state,
        })
    }
}
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidSpill, "invalid mapped record state")
}
impl MappedUels {
    fn ensure(&self) -> Result<()> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.closed || !self.metadata.has_map() {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "caller map is not registered or is closed",
            ));
        }
        self.reader.source.check_unchanged()
    }
    fn fail(&mut self, mut error: Error) -> Error {
        self.closed = true;
        if let Err(cleanup) = self.metadata.cleanup()
            && error.cleanup_error().is_none()
        {
            error = error.with_cleanup(cleanup);
        }
        self.error = Some(error.clone());
        error
    }
    /// Register once. Repeated identical pairs are idempotent; either-direction
    /// collisions fail instead of overwriting inverse entries. IDs are sparse.
    pub fn register<I>(&mut self, entries: I) -> Result<()>
    where
        I: IntoIterator<Item = Result<(UelId, MappedUelId)>>,
    {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let result = (|| {
            if self.initialized || self.closed {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "register the caller map once",
                ));
            }
            self.initialized = true;
            self.reader.source.check_unchanged()?;
            self.metadata.initialize_journal()?;
            let maximum = self.reader.source.counts()?.uels;
            self.metadata.build_raw_map(
                entries
                    .into_iter()
                    .map(|entry| entry.map(|(raw, mapped)| (raw.get(), mapped.get()))),
                maximum,
            )
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn mapped(&mut self, raw: UelId) -> Result<Option<MappedUelId>> {
        self.ensure()?;
        let result = self
            .metadata
            .map_lookup(raw.get(), false)
            .and_then(|v| v.map(|n| MappedUelId::new(i64::from(n))).transpose());
        result.map_err(|e| self.fail(e))
    }
    pub fn raw(&mut self, mapped: MappedUelId) -> Result<Option<UelId>> {
        self.ensure()?;
        let result = self
            .metadata
            .map_lookup(mapped.get(), true)
            .and_then(|v| v.map(UelId::new).transpose());
        result.map_err(|e| self.fail(e))
    }
    pub fn len(&self) -> u32 {
        self.metadata.map_count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn statistics(&self) -> MetadataStatistics {
        self.metadata.statistics()
    }
    /// Explicitly close, or retry removal after a failure. The map cannot reopen.
    pub fn abort(&mut self) -> Result<()> {
        self.closed = true;
        self.metadata.cleanup().map_err(Error::from)
    }
    pub fn close(&mut self) -> Result<()> {
        self.abort()
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MappedScanStatistics {
    pub decoded: u64,
    pub accepted: u64,
    pub skipped: u64,
    pub returned: u64,
    pub spill: SpillStatistics,
}
pub struct MappedRecord<'a> {
    pub raw_keys: &'a [UelId],
    pub mapped_keys: &'a [MappedUelId],
    pub values: &'a [Value],
}
/// One bounded row at a time in caller-ID order, retaining original raw keys.
/// Errors are terminal; previously returned rows are never replayed implicitly.
///
/// ```compile_fail,E0499
/// use gdxrs::{MappedCursor, Result};
/// fn retain(cursor: &mut MappedCursor<'_>) -> Result<()> {
///     let first = cursor.next()?.unwrap();
///     let _second = cursor.next()?;
///     println!("{}", first.raw_keys.len());
///     Ok(())
/// }
/// ```
pub struct MappedCursor<'a> {
    map: &'a mut MappedUels,
    cursor: Option<SpillCursor>,
    dimensions: usize,
    fields: usize,
    raw_max: u32,
    raw: [UelId; 20],
    mapped: [MappedUelId; 20],
    values: [Value; 5],
    previous: Option<[MappedUelId; 20]>,
    statistics: MappedScanStatistics,
    limit: Option<u64>,
    error: Option<Error>,
    _state: Reservation,
}
impl MappedCursor<'_> {
    pub fn statistics(&self) -> MappedScanStatistics {
        let mut stats = self.statistics;
        if let Some(cursor) = &self.cursor {
            stats.spill = cursor.statistics();
        }
        stats
    }
    fn end(&mut self) -> Result<()> {
        if let Some(mut cursor) = self.cursor.take() {
            let result = cursor.close();
            let statistics = cursor.statistics();
            drop(cursor);
            self.statistics.spill = statistics;
            self.map
                .metadata
                .map_sort_closed(statistics, result.is_ok());
            result?;
        }
        Ok(())
    }
    pub fn close(&mut self) -> Result<MappedScanStatistics> {
        if let Err(e) = self.end() {
            let e = self.map.fail(e);
            self.error = Some(e.clone());
            return Err(e);
        }
        Ok(self.statistics())
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<MappedRecord<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.cursor.is_none() {
            return Ok(None);
        }
        let result = (|| {
            self.map.reader.source.check_unchanged()?;
            if self
                .limit
                .is_some_and(|limit| self.statistics.returned >= limit)
            {
                self.end()?;
                return Ok(false);
            }
            let Some(cursor) = self.cursor.as_mut() else {
                return Ok(false);
            };
            let Some(entry) = cursor.next()? else {
                if self.statistics.returned != self.statistics.accepted {
                    return Err(invalid());
                }
                self.end()?;
                return Ok(false);
            };
            let prefix = self.dimensions.checked_mul(8).ok_or_else(invalid)?;
            if entry.id == 0
                || entry.id > self.statistics.accepted
                || entry.label.len() < prefix
                || entry.label.len() > 225
            {
                return Err(invalid());
            }
            for d in 0..self.dimensions {
                let mapped = u32::from_be_bytes(
                    entry.label[d * 4..d * 4 + 4]
                        .try_into()
                        .map_err(|_| invalid())?,
                );
                let raw = u32::from_be_bytes(
                    entry.label[self.dimensions * 4 + d * 4..self.dimensions * 4 + d * 4 + 4]
                        .try_into()
                        .map_err(|_| invalid())?,
                );
                self.mapped[d] = MappedUelId::new(i64::from(mapped))?;
                self.raw[d] = UelId::new(raw)?;
                if raw > self.raw_max || self.map.metadata.map_lookup(raw, false)? != Some(mapped) {
                    return Err(invalid());
                }
            }
            self.values = write::unpack_values(&entry.label[prefix..], self.fields)?;
            if self
                .previous
                .is_some_and(|keys| keys[..self.dimensions] >= self.mapped[..self.dimensions])
            {
                return Err(invalid());
            }
            self.previous = Some(self.mapped);
            self.statistics.returned = self
                .statistics
                .returned
                .checked_add(1)
                .ok_or_else(invalid)?;
            Ok(true)
        })();
        match result {
            Ok(true) => Ok(Some(MappedRecord {
                raw_keys: &self.raw[..self.dimensions],
                mapped_keys: &self.mapped[..self.dimensions],
                values: &self.values[..self.fields],
            })),
            Ok(false) => Ok(None),
            Err(mut error) => {
                if let Err(cleanup) = self.end()
                    && error.cleanup_error().is_none()
                {
                    error = error.with_cleanup(std::io::Error::other(cleanup));
                }
                let error = self.map.fail(error);
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
}
impl Drop for MappedCursor<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
