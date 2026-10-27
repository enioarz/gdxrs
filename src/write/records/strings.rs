//! Two-pass string records. One owned checksummed spool; no retained dictionary.
use super::*;
use crate::model::Bytes;
use crate::write::store::{Frame, Store};
use std::sync::{Mutex, MutexGuard};

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidSpill, "invalid string record spool")
}
fn lock(store: &Mutex<Store>) -> Result<MutexGuard<'_, Store>> {
    store.lock().map_err(|_| invalid())
}
fn number(bytes: &[u8]) -> Result<u32> {
    let b: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
    Ok(u32::from_le_bytes(b))
}
fn count(n: u32) -> Result<u32> {
    n.checked_add(1)
        .filter(|v| *v <= i32::MAX as u32)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidCount,
                "string record count exceeds v7 range",
            )
        })
}
/// Completion of the string staging/replay phase, before Writer::finish publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StringWriteStatistics {
    pub symbols: u32,
    pub records: u64,
    pub label_requests: u32,
    pub uels: u32,
    pub merge_passes: u64,
}
/// Stage contiguous symbol groups before freezing metadata, then map/replay at finish.
/// Labels retain exact bytes; IDs follow existing registration then first encounter.
/// No output can publish if this phase fails or is abandoned.
pub struct StringRecordWriter<'a> {
    writer: &'a mut Writer,
    store: Arc<Mutex<Store>>,
    active: Option<(SymbolType, usize)>,
    group_records: u32,
    statistics: StringWriteStatistics,
    keys: Buffer<WriterUelId>,
    mapping: Buffer<u32>,
    finished: bool,
    _state: Reservation,
}
impl Writer {
    /// Begin a two-pass string phase before any symbol data is written.
    /// Register symbols, domains, texts and acronyms first. Requires seven scratch
    /// handles within MetadataOptions; the spool shares its journal-space half.
    pub fn begin_string_records(&mut self) -> Result<StringRecordWriter<'_>> {
        self.open()?;
        let prepared = (|| {
            if self.symbol_active {
                return Err(Error::new(ErrorKind::InvalidWriteState, "symbol is active"));
            }
            let state = self.budget.reserve(CONTROL)?;
            if size_of::<StringRecordWriter<'_>>() > CONTROL {
                return Err(invalid());
            }
            let keys = Buffer::new(&self.budget, 20)?;
            let mapping = Buffer::new(&self.budget, 1024)?;
            let store = self.metadata_call(Metadata::string_begin)?;
            Ok((state, keys, mapping, store))
        })();
        let (state, keys, mapping, store) = prepared.map_err(|e| self.fail(e))?;
        self.symbol_active = true;
        Ok(StringRecordWriter {
            writer: self,
            store,
            active: None,
            group_records: 0,
            statistics: StringWriteStatistics {
                symbols: 0,
                records: 0,
                label_requests: 0,
                uels: 0,
                merge_passes: 0,
            },
            keys,
            mapping,
            finished: false,
            _state: state,
        })
    }
}
impl StringRecordWriter<'_> {
    pub fn write_f64(&mut self, labels: &[&[u8]], values: &[f64]) -> Result<()> {
        let typed = self
            .writer
            .convert_values(values)
            .map_err(|e| self.writer.fail(e))?;
        self.write(labels, &typed[..values.len()])
    }
    fn append(&mut self, tag: u8, bytes: &[u8]) -> Result<()> {
        self.writer.open()?;
        let mut store = lock(&self.store)?;
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .string_append(&mut store, tag, bytes)
    }
    fn end(&mut self) -> Result<()> {
        if self.active.is_some() {
            self.append(4, &self.group_records.to_le_bytes())?;
            self.active = None;
        }
        Ok(())
    }
    /// Close the previous input group and start a symbol. Every symbol may appear
    /// once; write domain sets before dependent symbols. Arbitrary row order is allowed.
    pub fn begin_symbol(&mut self, id: WriterSymbolId) -> Result<()> {
        let result = (|| {
            self.end()?;
            self.writer.open()?;
            let shape = self
                .writer
                .metadata
                .as_mut()
                .ok_or_else(invalid)?
                .string_shape(id)?;
            let symbols = count(self.statistics.symbols)?;
            self.append(1, &id.get().to_le_bytes())?;
            self.statistics.symbols = symbols;
            self.group_records = 0;
            self.active = Some(shape);
            Ok(())
        })();
        result.map_err(|e| self.writer.fail(e))
    }
    /// Stream one row; no label slice or value slice is retained.
    /// Invalid labels/shape fail now; mapped duplicates/domains/value references
    /// are checked during finish replay. Both failures are terminal.
    pub fn write(&mut self, labels: &[&[u8]], values: &[Value]) -> Result<()> {
        let result = (|| {
            self.writer.open()?;
            let (kind, dim) = self.active.ok_or_else(|| {
                Error::new(ErrorKind::InvalidWriteState, "begin a string symbol first")
            })?;
            if labels.len() != dim {
                return Err(Error::new(
                    ErrorKind::InvalidDimension,
                    "string key arity mismatch",
                ));
            }
            if values.len() != kind.fields() {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "string value arity mismatch",
                ));
            }
            let n = count(self.group_records)?;
            let requests = self
                .statistics
                .label_requests
                .checked_add(dim as u32)
                .filter(|v| *v <= i32::MAX as u32)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidCount,
                        "string label count exceeds adapter range",
                    )
                })?;
            for bytes in labels {
                super::super::metadata::validate_string_label(bytes)?;
            }
            let packet = unsorted::Packet::values(values)?;
            self.append(2, packet.payload())?;
            for bytes in labels {
                self.append(3, bytes)?;
            }
            self.group_records = n;
            self.statistics.label_requests = requests;
            self.statistics.records = self.statistics.records.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })();
        result.map_err(|e| self.writer.fail(e))
    }
    /// Build the first-encounter dictionary/remap and externally sort/replay each
    /// group. Metadata freezes here. Writer::finish is still required to publish.
    pub fn finish(mut self) -> Result<StringWriteStatistics> {
        let result = self.complete();
        self.finished = true;
        match result {
            Ok(()) => Ok(self.statistics),
            Err(e) if self.writer.statistics.status == WriteStatus::Open => {
                Err(self.writer.fail(e))
            }
            Err(e) => Err(self.writer.error.clone().unwrap_or(e)),
        }
    }
    fn complete(&mut self) -> Result<()> {
        self.end()?;
        let extent = {
            let mut store = lock(&self.store)?;
            store.sync()?;
            store.length
        };
        let remap_bytes = u64::from(self.statistics.label_requests)
            .checked_mul(19)
            .ok_or_else(invalid)?;
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .string_reserve_mapping(remap_bytes)?;
        let shared = Arc::clone(&self.store);
        let mut at = 0;
        let mut requests = 0u32;
        let expected = self.statistics.label_requests;
        let labels = std::iter::from_fn(|| {
            loop {
                if at == extent {
                    return if requests == expected {
                        None
                    } else {
                        Some(Err(invalid()))
                    };
                }
                let result = (|| {
                    if at > extent {
                        return Err(invalid());
                    }
                    let frame = lock(&shared)?.next(&mut at)?.ok_or_else(invalid)?;
                    if at > extent || !(1..=4).contains(&frame.tag) {
                        return Err(invalid());
                    }
                    if frame.tag == 3 {
                        requests = count(requests)?;
                        return Ok(Some(Bytes::from_slice(frame.payload())?));
                    }
                    Ok(None)
                })();
                match result {
                    Ok(Some(label)) => return Some(Ok(label)),
                    Ok(None) => {}
                    Err(e) => return Some(Err(e)),
                }
            }
        });
        let mut used = extent;
        let mut peak = extent;
        let limit = extent.checked_add(remap_bytes).ok_or_else(invalid)?;
        let mut mapped = 0u32;
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .bulk_extend(labels, |ordinal, id| {
                mapped = count(mapped)?;
                if ordinal != u64::from(mapped) {
                    return Err(invalid());
                }
                let mut payload = [0; 8];
                payload[..4].copy_from_slice(&mapped.to_le_bytes());
                payload[4..].copy_from_slice(&id.get().to_le_bytes());
                lock(&shared)?.append(8, &payload, &mut used, &mut peak, limit)
            })?;
        if mapped != expected || used != limit {
            return Err(invalid());
        }
        lock(&self.store)?.sync()?;
        let metadata = self
            .writer
            .metadata
            .as_ref()
            .ok_or_else(invalid)?
            .statistics();
        self.statistics.uels = metadata.uels;
        self.statistics.merge_passes = metadata.merge_passes;
        self.writer.symbol_active = false;
        let (mut at, mut mapping, mut ordinal, mut symbols, mut records) =
            (0, extent, 0u32, 0u32, 0u64);
        let mut map_index = 0usize;
        let mut loaded = 0u32;
        while at < extent {
            let frame = next(&self.store, &mut at, extent, 1)?;
            let id = self
                .writer
                .metadata
                .as_ref()
                .ok_or_else(invalid)?
                .string_symbol(number(frame.payload())?)?;
            let mut rows = self.writer.begin_unsorted_symbol(id, None)?;
            let inner = rows.inner.as_ref().ok_or_else(invalid)?;
            let (dim, fields) = (inner.dim, inner.kind.fields());
            let replay = (|| {
                let mut group_count = 0u32;
                loop {
                    let frame = lock(&self.store)?.next(&mut at)?.ok_or_else(invalid)?;
                    if at > extent {
                        return Err(invalid());
                    }
                    if frame.tag == 4 {
                        if number(frame.payload())? != group_count {
                            return Err(invalid());
                        }
                        break;
                    }
                    if frame.tag != 2 {
                        return Err(invalid());
                    }
                    let values = unsorted::decode_values(frame.payload(), fields)?;
                    self.keys.data.clear();
                    for _ in 0..dim {
                        // Verify the original label frame as well as the ordinal mapping.
                        let label = next(&self.store, &mut at, extent, 3)?;
                        super::super::metadata::validate_string_label(label.payload())?;
                        if map_index == self.mapping.data.len() {
                            self.mapping.data.clear();
                            map_index = 0;
                            while self.mapping.data.len() < self.mapping.data.capacity()
                                && loaded < expected
                            {
                                let mapped = next(&self.store, &mut mapping, limit, 8)?;
                                if mapped.payload().len() != 8 {
                                    return Err(invalid());
                                }
                                loaded = count(loaded)?;
                                if number(&mapped.payload()[..4])? != loaded {
                                    return Err(invalid());
                                }
                                self.mapping.push(number(&mapped.payload()[4..])?)?;
                            }
                        }
                        ordinal = count(ordinal)?;
                        let mapped_id = self
                            .mapping
                            .data
                            .get(map_index)
                            .copied()
                            .ok_or_else(invalid)?;
                        map_index = map_index.checked_add(1).ok_or_else(invalid)?;
                        let id = rows
                            .inner
                            .as_ref()
                            .ok_or_else(invalid)?
                            .writer
                            .metadata
                            .as_ref()
                            .ok_or_else(invalid)?
                            .bound_uel(mapped_id)?;
                        self.keys.push(id)?;
                    }
                    rows.write(&self.keys.data, &values[..fields])?;
                    group_count = count(group_count)?;
                }
                Ok(group_count)
            })();
            let group_count = match replay {
                Ok(n) => n,
                Err(e) => return Err(rows.reject(e)),
            };
            let stats = rows.finish()?;
            self.statistics.merge_passes = self
                .statistics
                .merge_passes
                .checked_add(stats.spill.merge_passes)
                .ok_or_else(invalid)?;
            symbols = symbols.checked_add(1).ok_or_else(invalid)?;
            records = records
                .checked_add(u64::from(group_count))
                .ok_or_else(invalid)?;
        }
        if at != extent
            || mapping != limit
            || ordinal != expected
            || loaded != expected
            || symbols != self.statistics.symbols
            || records != self.statistics.records
        {
            return Err(invalid());
        }
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(invalid)?
            .string_done(limit)?;
        Ok(())
    }
}
fn next(store: &Mutex<Store>, at: &mut u64, end: u64, tag: u8) -> Result<Frame> {
    let frame = lock(store)?.next(at)?.ok_or_else(invalid)?;
    if *at > end || frame.tag != tag {
        return Err(invalid());
    }
    Ok(frame)
}
impl Drop for StringRecordWriter<'_> {
    fn drop(&mut self) {
        if !self.finished && self.writer.statistics.status == WriteStatus::Open {
            self.writer.fail(Error::new(
                ErrorKind::InvalidWriteState,
                "string records abandoned",
            ));
        }
    }
}
#[cfg(test)]
#[path = "string_tests.rs"]
mod tests;
