//! Disk-backed deferred domains and lossless, fallible error callbacks.
use super::*;
use crate::model::Bytes;
use crate::write::store::{Frame, Store};
use std::sync::{Mutex, MutexGuard};
const MAX_PACKET: usize = 238; // 20 raw + ordinal + local ordinal + origin + 20 mapped + five values
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidSpill, "invalid deferred record spool")
}
fn lock(store: &Mutex<Store>) -> Result<MutexGuard<'_, Store>> {
    store.lock().map_err(|_| invalid())
}
fn number(b: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(b.try_into().map_err(|_| invalid())?))
}
fn next(store: &Mutex<Store>, at: &mut u64, end: u64) -> Result<Frame> {
    let frame = lock(store)?.next(at)?.ok_or_else(invalid)?;
    if *at > end {
        return Err(invalid());
    }
    Ok(frame)
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ValidationStatistics {
    pub input_records: u64,
    /// Tentative rows encoded; publication still requires Writer::finish.
    pub accepted_records: u64,
    pub symbols: u32,
    pub auto_empty_sets: u32,
    pub errors: DataErrorStatistics,
}
/// Stage contiguous groups in any dependency order. Keys must be registered first.
/// Membership and duplicates are resolved on disk at finish. Other errors are terminal.
pub struct DeferredRecordWriter<'a> {
    writer: &'a mut Writer,
    store: Arc<Mutex<Store>>,
    active: Option<(SymbolType, usize)>,
    group_records: u32,
    groups: u32,
    finished: bool,
    keys: Buffer<WriterUelId>,
    _state: Reservation,
}
impl Writer {
    /// Begin a bounded staging phase before metadata freezes. Requires one extra
    /// scratch handle (seven total; eight with a retained mapped index).
    pub fn begin_deferred_records(&mut self) -> Result<DeferredRecordWriter<'_>> {
        self.open()?;
        let result = (|| {
            if self.symbol_active {
                return Err(Error::new(ErrorKind::InvalidWriteState, "symbol is active"));
            }
            let state = self.budget.reserve(CONTROL)?;
            if size_of::<DeferredRecordWriter<'_>>() + size_of::<[Bytes; 20]>() > CONTROL {
                return Err(invalid());
            }
            let keys = Buffer::new(&self.budget, 20)?;
            let store = self.metadata_call(Metadata::deferred_begin)?;
            Ok((state, keys, store))
        })();
        let (state, keys, store) = result.map_err(|e| self.fail(e))?;
        self.symbol_active = true;
        self.validation_statistics = ValidationStatistics::default();
        Ok(DeferredRecordWriter {
            writer: self,
            store,
            active: None,
            group_records: 0,
            groups: 0,
            finished: false,
            keys,
            _state: state,
        })
    }
    /// Available after failed replay as well as successful completion.
    pub fn validation_statistics(&self) -> ValidationStatistics {
        self.validation_statistics
    }
}
impl DeferredRecordWriter<'_> {
    pub fn statistics(&self) -> ValidationStatistics {
        self.writer.validation_statistics
    }
    fn append(&mut self, tag: u8, payload: &[u8]) -> Result<()> {
        self.writer.open()?;
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .string_append(&mut *lock(&self.store)?, tag, payload)
    }
    fn end(&mut self) -> Result<()> {
        if self.active.is_some() {
            self.append(3, &self.group_records.to_le_bytes())?;
            self.active = None;
        }
        Ok(())
    }
    fn staged(&self, id: u32) -> Result<bool> {
        let mut at = 0;
        let end = lock(&self.store)?.length;
        while at < end {
            let f = next(&self.store, &mut at, end)?;
            if f.tag == 1 && number(f.payload())? == id {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn begin_symbol(&mut self, id: WriterSymbolId) -> Result<()> {
        self.writer.open()?;
        let result = (|| {
            self.end()?;
            let m = self.writer.metadata.as_mut().ok_or_else(invalid)?;
            let shape = m.string_shape(id)?;
            if m.records_completed(id)? || self.staged(id.get())? {
                return Err(Error::new(
                    ErrorKind::InvalidWriteState,
                    "symbol group repeated",
                ));
            }
            let groups = self.groups.checked_add(1).ok_or_else(invalid)?;
            self.append(1, &id.get().to_le_bytes())?;
            self.active = Some(shape);
            self.group_records = 0;
            self.groups = groups;
            Ok(())
        })();
        result.map_err(|e| self.writer.fail(e))
    }
    /// Raw-key input, retaining one-based raw identities.
    pub fn write(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<()> {
        self.writer.open()?;
        let result = self.stage(keys, values, DataOrigin::RawWrite, &[]);
        result.map_err(|e| self.writer.fail(e))
    }
    /// Caller-key input. All mappings must already exist; missing mappings are terminal.
    pub fn write_mapped(&mut self, keys: &[MappedUelId], values: &[Value]) -> Result<()> {
        self.writer.open()?;
        let result = (|| {
            self.keys.data.clear();
            let m = self.writer.metadata.as_mut().ok_or_else(invalid)?;
            for key in keys {
                let raw = m.map_lookup(key.get(), true)?.ok_or_else(|| {
                    Error::new(ErrorKind::InvalidReference, "missing caller mapping")
                })?;
                self.keys.push(m.bound_uel(raw)?)?;
            }
            // Fixed stack copy lets stage borrow self without retaining caller slices.
            let mut raw = [None; 20];
            for (slot, key) in raw.iter_mut().zip(&self.keys.data) {
                *slot = Some(*key);
            }
            let mut packet_keys = Buffer::new(&self.writer.budget, keys.len().min(20))?;
            for key in raw.into_iter().take(keys.len()) {
                packet_keys.push(key.ok_or_else(invalid)?)?;
            }
            self.stage(&packet_keys.data, values, DataOrigin::MappedWrite, keys)
        })();
        result.map_err(|e| self.writer.fail(e))
    }
    /// Exact string keys must have been registered before this phase. Error
    /// callbacks can inspect their original bytes through DataErrorRecord::label.
    pub fn write_strings(&mut self, labels: &[&[u8]], values: &[Value]) -> Result<()> {
        self.writer.open()?;
        let result = (|| {
            let mut keys = Buffer::new(&self.writer.budget, labels.len().min(20))?;
            let m = self.writer.metadata.as_mut().ok_or_else(invalid)?;
            for label in labels {
                keys.push(m.existing_label(label)?)?;
            }
            self.stage(&keys.data, values, DataOrigin::StringWrite, &[])
        })();
        result.map_err(|e| self.writer.fail(e))
    }
    fn stage(
        &mut self,
        keys: &[WriterUelId],
        values: &[Value],
        origin: DataOrigin,
        mapped: &[MappedUelId],
    ) -> Result<()> {
        self.writer.open()?;
        let (kind, dim) = self.active.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidWriteState,
                "begin a deferred symbol first",
            )
        })?;
        if keys.len() != dim || values.len() != kind.fields() {
            return Err(Error::new(
                ErrorKind::InvalidDimension,
                "record arity mismatch",
            ));
        }
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .validate_record(&[None; 20], keys, values, kind)?;
        let local = self
            .group_records
            .checked_add(1)
            .filter(|n| *n <= i32::MAX as u32)
            .ok_or_else(invalid)?;
        let ordinal = self
            .writer
            .validation_statistics
            .input_records
            .checked_add(1)
            .ok_or_else(invalid)?;
        let mut bytes = [0; MAX_PACKET];
        let mut at = 0;
        for key in keys {
            bytes[at..at + 4].copy_from_slice(&key.get().to_be_bytes());
            at += 4;
        }
        bytes[at..at + 8].copy_from_slice(&ordinal.to_be_bytes());
        at += 8;
        bytes[at..at + 4].copy_from_slice(&local.to_le_bytes());
        at += 4;
        bytes[at] = match origin {
            DataOrigin::RawWrite => 0,
            DataOrigin::MappedWrite => 1,
            DataOrigin::StringWrite => 2,
            DataOrigin::MappedRead => return Err(invalid()),
        };
        at += 1;
        if origin == DataOrigin::MappedWrite {
            for key in mapped {
                bytes[at..at + 4].copy_from_slice(&key.get().to_le_bytes());
                at += 4;
            }
        }
        at += pack_values(values, &mut bytes[at..])?;
        self.append(2, &bytes[..at])?;
        self.group_records = local;
        self.writer.validation_statistics.input_records = ordinal;
        Ok(())
    }
    /// Terminal rejection remains the default; nothing is published on failure.
    pub fn finish(mut self) -> Result<ValidationStatistics> {
        self.writer.open()?;
        let result = self.complete(None, &mut |_| Ok(()));
        self.finish_result(result)
    }
    /// Reject invalid-domain and duplicate rows only. Every rejected row reaches
    /// the callback or the transaction fails. First input wins among valid duplicates.
    pub fn finish_reporting<F>(
        mut self,
        limits: ErrorLimits,
        mut callback: F,
    ) -> Result<ValidationStatistics>
    where
        F: for<'a> FnMut(DataErrorRecord<'a>) -> Result<()>,
    {
        self.writer.open()?;
        let result = self.complete(Some(limits), &mut callback);
        self.finish_result(result)
    }
    fn finish_result(&mut self, result: Result<()>) -> Result<ValidationStatistics> {
        self.finished = true;
        if let Err(e) = result {
            return Err(if self.writer.statistics.status == WriteStatus::Open {
                self.writer.fail(e)
            } else {
                self.writer.error = Some(e.clone());
                e
            });
        }
        Ok(self.statistics())
    }
    fn complete<F>(&mut self, limits: Option<ErrorLimits>, callback: &mut F) -> Result<()>
    where
        F: for<'a> FnMut(DataErrorRecord<'a>) -> Result<()>,
    {
        self.end()?;
        // Empty, unstaged domain sets must participate in dependency resolution.
        let symbols = self
            .writer
            .metadata
            .as_ref()
            .ok_or_else(invalid)?
            .statistics()
            .symbols;
        for n in 1..=symbols {
            let set = self
                .writer
                .metadata
                .as_mut()
                .ok_or_else(invalid)?
                .unwritten_set(n)?;
            if let Some(id) = set
                && !self.staged(n)?
            {
                self.append(1, &n.to_le_bytes())?;
                self.append(3, &0u32.to_le_bytes())?;
                self.groups = self.groups.checked_add(1).ok_or_else(invalid)?;
                self.writer.validation_statistics.auto_empty_sets += 1;
                let _ = id;
            }
        }
        lock(&self.store)?.sync()?;
        let extent = lock(&self.store)?.length;
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .freeze()?;
        self.writer.symbol_active = false;
        let mut completed = 0;
        while completed < self.groups {
            let mut at = 0;
            let mut progress = false;
            while at < extent {
                let f = next(&self.store, &mut at, extent)?;
                if f.tag != 1 {
                    return Err(invalid());
                }
                let m = self.writer.metadata.as_mut().ok_or_else(invalid)?;
                let id = m.string_symbol(number(f.payload())?)?;
                let done = m.records_completed(id)?;
                let ready = !done && m.records_ready(id)?;
                let (kind, dim) = m.deferred_shape(id)?;
                let mut sorter = if ready {
                    Some(m.record_sorter()?)
                } else {
                    None
                };
                let observe = sorter.as_ref().map(SpillSorter::observer);
                let mut count = 0u32;
                let staged = (|| {
                    loop {
                        let f = next(&self.store, &mut at, extent)?;
                        if f.tag == 3 {
                            if number(f.payload())? != count {
                                return Err(invalid());
                            }
                            break;
                        }
                        if f.tag != 2 || f.len > MAX_PACKET {
                            return Err(invalid());
                        }
                        count = count.checked_add(1).ok_or_else(invalid)?;
                        if let Some(sort) = sorter.as_mut() {
                            sort.push(f.payload(), u64::from(count))?;
                        }
                    }
                    Ok(())
                })();
                if let Err(mut error) = staged {
                    if let Some(mut sort) = sorter.take() {
                        if let Err(e) = sort.cancel() {
                            error = attach(error, e);
                        }
                        drop(sort);
                    }
                    if let Some(observe) = observe {
                        self.writer
                            .metadata
                            .as_mut()
                            .ok_or_else(invalid)?
                            .record_sort_statistics(observe());
                    }
                    return Err(error);
                }
                if let Some(sort) = sorter {
                    let observe = observe.ok_or_else(invalid)?;
                    let result = self.replay(id, kind, dim, count, sort, limits, callback);
                    if let Some(m) = self.writer.metadata.as_mut() {
                        m.record_sort_statistics(observe());
                    }
                    result?;
                    completed += 1;
                    progress = true;
                }
            }
            if !progress {
                return Err(Error::new(
                    ErrorKind::InvalidReference,
                    "unresolvable strict-domain dependency cycle",
                ));
            }
        }
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .deferred_done(extent)
    }
    #[allow(clippy::too_many_arguments)]
    fn replay<F>(
        &mut self,
        id: WriterSymbolId,
        kind: SymbolType,
        dim: usize,
        count: u32,
        mut sort: SpillSorter,
        limits: Option<ErrorLimits>,
        callback: &mut F,
    ) -> Result<()>
    where
        F: for<'a> FnMut(DataErrorRecord<'a>) -> Result<()>,
    {
        let mut rows = match self.writer.begin_symbol_internal(id, None, false) {
            Ok(rows) => rows,
            Err(e) => {
                let result = sort.cancel();
                return Err(match result {
                    Err(c) => attach(e, c),
                    Ok(_) => e,
                });
            }
        };
        let mut cursor = match sort.finish() {
            Ok(cursor) => cursor,
            Err(e) => return Err(rows.writer.fail(e)),
        };
        let replay = (|| {
            let mut seen = 0u32;
            while let Some(entry) = cursor.next()? {
                let bytes = entry.label;
                let header = dim * 4 + 13;
                if bytes.len() < header || bytes.len() > MAX_PACKET {
                    return Err(invalid());
                }
                self.keys.data.clear();
                let mut raw = [UelId::new(1)?; 20];
                let mut mapped = [MappedUelId::new(0)?; 20];
                let mut labels = std::array::from_fn::<_, 20, _>(|_| Bytes::literal(b""));
                let m = rows.writer.metadata.as_mut().ok_or_else(invalid)?;
                for d in 0..dim {
                    let n = u32::from_be_bytes(
                        bytes[d * 4..d * 4 + 4].try_into().map_err(|_| invalid())?,
                    );
                    raw[d] = UelId::new(n)?;
                    self.keys.push(m.bound_uel(n)?)?;
                }
                let ordinal = u64::from_be_bytes(
                    bytes[dim * 4..dim * 4 + 8]
                        .try_into()
                        .map_err(|_| invalid())?,
                );
                let local = number(&bytes[dim * 4 + 8..dim * 4 + 12])?;
                if ordinal == 0
                    || ordinal > rows.writer.validation_statistics.input_records
                    || local == 0
                    || local > count
                    || entry.id != u64::from(local)
                {
                    return Err(invalid());
                }
                let origin = match bytes[header - 1] {
                    0 => DataOrigin::RawWrite,
                    1 => DataOrigin::MappedWrite,
                    2 => DataOrigin::StringWrite,
                    _ => return Err(invalid()),
                };
                let mut at = header;
                if origin == DataOrigin::MappedWrite {
                    for key in mapped.iter_mut().take(dim) {
                        let end = at + 4;
                        *key = MappedUelId::new(i64::from(number(
                            bytes.get(at..end).ok_or_else(invalid)?,
                        )?))?;
                        at = end;
                    }
                }
                let values = unpack_values(bytes.get(at..).ok_or_else(invalid)?, kind.fields())?;
                let m = rows.writer.metadata.as_mut().ok_or_else(invalid)?;
                let mask = m.domain_violations(&rows.domains, &self.keys.data)?;
                let duplicate = rows.count > 0
                    && self
                        .keys
                        .data
                        .iter()
                        .enumerate()
                        .all(|(d, key)| key.get() == rows.previous[d]);
                let error = if mask != 0 {
                    Some(DataErrorKind::DomainViolation { dimensions: mask })
                } else if duplicate {
                    Some(DataErrorKind::Duplicate)
                } else {
                    None
                };
                if let Some(error) = error {
                    if origin == DataOrigin::StringWrite {
                        for d in 0..dim {
                            labels[d] = m.uel_label(raw[d])?;
                        }
                    }
                    let stats = &mut rows.writer.validation_statistics.errors;
                    if let Some(limits) = limits {
                        crate::data_error::report(
                            DataErrorRecord {
                                session_id: rows.writer.owner,
                                symbol: SymbolId::new(id.get())?,
                                input_ordinal: ordinal,
                                symbol_ordinal: local,
                                origin,
                                kind: error,
                                raw_keys: &raw[..dim],
                                values: &values[..kind.fields()],
                                mapped: &mapped[..dim],
                                mapped_mask: if origin == DataOrigin::MappedWrite {
                                    (1 << dim) - 1
                                } else {
                                    0
                                },
                                labels: if origin == DataOrigin::StringWrite {
                                    Some(&labels[..dim])
                                } else {
                                    None
                                },
                            },
                            limits,
                            stats,
                            callback,
                        )?;
                    } else {
                        stats.errors = stats.errors.checked_add(1).ok_or_else(invalid)?;
                        return Err(Error::new(
                            ErrorKind::InvalidReference,
                            "deferred domain or duplicate rejection",
                        ));
                    }
                } else {
                    rows.row(&self.keys.data, &values[..kind.fields()])?;
                    rows.writer.validation_statistics.accepted_records = rows
                        .writer
                        .validation_statistics
                        .accepted_records
                        .checked_add(1)
                        .ok_or_else(invalid)?;
                }
                seen = seen.checked_add(1).ok_or_else(invalid)?;
            }
            if seen != count || !cursor.statistics().complete {
                return Err(invalid());
            }
            Ok(())
        })();
        let close = cursor.close();
        if let Some(m) = rows.writer.metadata.as_mut() {
            m.record_sort_statistics(cursor.statistics());
        }
        match (replay, close) {
            (Err(e), cleanup) => Err(rows.writer.fail(match cleanup {
                Err(c) => attach(e, c),
                Ok(_) => e,
            })),
            (Ok(()), Err(e)) => Err(rows.writer.fail(e)),
            (Ok(()), Ok(_)) => {
                rows.finish()?;
                self.writer.validation_statistics.symbols += 1;
                Ok(())
            }
        }
    }
}
fn attach(error: Error, cleanup: Error) -> Error {
    if error.cleanup_error().is_some() {
        error
    } else {
        error.with_cleanup(io::Error::other(cleanup))
    }
}
impl Drop for DeferredRecordWriter<'_> {
    fn drop(&mut self) {
        if !self.finished && self.writer.statistics.status == WriteStatus::Open {
            self.writer.fail(Error::new(
                ErrorKind::InvalidWriteState,
                "deferred records abandoned",
            ));
        }
    }
}

#[cfg(test)]
#[path = "deferred_tests.rs"]
mod tests;
