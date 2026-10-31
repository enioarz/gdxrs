use crate::{
    budget::{Buffer, Reservation},
    records::{Decoder, DecoderPosition},
    *,
};
use std::mem::size_of;

struct Batch {
    keys: [Option<Buffer<UelId>>; 20],
    values: [Option<Buffer<Value>>; 5],
    dimensions: [u8; 20],
    fields: [ValueField; 5],
    key_count: usize,
    value_count: usize,
    rows: usize,
}
impl Batch {
    fn new(reader: &Reader, key_mask: u32, value_mask: u8, capacity: usize) -> Result<Self> {
        let mut batch = Self {
            keys: std::array::from_fn(|_| None),
            values: std::array::from_fn(|_| None),
            dimensions: [0; 20],
            fields: [ValueField::Level; 5],
            key_count: 0,
            value_count: 0,
            rows: 0,
        };
        for (d, column) in batch.keys.iter_mut().enumerate() {
            if key_mask & (1 << d) != 0 {
                *column = Some(Buffer::new(&reader.source.budget, capacity)?);
                batch.dimensions[batch.key_count] = d as u8;
                batch.key_count += 1;
            }
        }
        for (f, column) in batch.values.iter_mut().enumerate() {
            if value_mask & (1 << f) != 0 {
                *column = Some(Buffer::new(&reader.source.budget, capacity)?);
                batch.fields[batch.value_count] = ValueField::ALL[f];
                batch.value_count += 1;
            }
        }
        Ok(batch)
    }
    fn clear(&mut self) {
        for column in self.keys.iter_mut().flatten() {
            column.data.clear();
        }
        for column in self.values.iter_mut().flatten() {
            column.data.clear();
        }
        self.rows = 0;
    }
    fn push(&mut self, row: &crate::records::Record) -> Result<()> {
        for (d, column) in self.keys.iter_mut().enumerate() {
            if let Some(column) = column {
                column.push(row.key(d).ok_or_else(|| {
                    Error::new(ErrorKind::InvalidReference, "missing projected key")
                })?)?;
            }
        }
        for (f, column) in self.values.iter_mut().enumerate() {
            if let Some(column) = column {
                column.push(row.values[f])?;
            }
        }
        self.rows += 1;
        Ok(())
    }
}

/// A borrowed batch. Its buffers are reused by the next cursor call.
///
/// ```compile_fail,E0499
/// # use gdxrs::{ScanCursor, Result};
/// fn retain(cursor: &mut ScanCursor) -> Result<()> {
///     let first = cursor.next_batch()?.unwrap();
///     let second = cursor.next_batch()?;
///     println!("{}", first.len()); // first still borrows cursor
///     Ok(())
/// }
/// ```
pub struct BatchView<'a> {
    batch: &'a Batch,
}
impl BatchView<'_> {
    pub fn len(&self) -> usize {
        self.batch.rows
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn key_dimensions(&self) -> &[u8] {
        &self.batch.dimensions[..self.batch.key_count]
    }
    pub fn value_fields(&self) -> &[ValueField] {
        &self.batch.fields[..self.batch.value_count]
    }
    pub fn key_column(&self, dimension: u8) -> Option<&[UelId]> {
        self.batch
            .keys
            .get(usize::from(dimension))?
            .as_ref()
            .map(|b| b.data.as_slice())
    }
    pub fn value_column(&self, field: ValueField) -> Option<&[Value]> {
        self.batch
            .values
            .get(field.index())?
            .as_ref()
            .map(|b| b.data.as_slice())
    }
    pub fn row(&self, index: usize) -> Option<RecordView<'_>> {
        (index < self.len()).then_some(RecordView {
            batch: self.batch,
            index,
        })
    }
}
pub struct RecordView<'a> {
    batch: &'a Batch,
    index: usize,
}
impl RecordView<'_> {
    pub fn key(&self, dimension: u8) -> Option<UelId> {
        self.batch
            .keys
            .get(usize::from(dimension))?
            .as_ref()?
            .data
            .get(self.index)
            .copied()
    }
    pub fn value(&self, field: ValueField) -> Option<Value> {
        self.batch
            .values
            .get(field.index())?
            .as_ref()?
            .data
            .get(self.index)
            .copied()
    }
}

/// Per-cursor work counters; physical I/O remains in Reader::io_statistics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanStatistics {
    /// Complete decoded public rows, including rejected rows and failed fills.
    pub decoded_rows: u64,
    /// Rows exposed in successful nonempty batches, excluding failed fills.
    pub returned_rows: u64,
}

/// Opaque, root-charged position immediately before the next raw public row.
/// Bound to one open Reader source and one requested symbol, including aliases.
/// This is not a serialized sidecar or a cryptographic content identity.
/// Resume takes fresh projection, batch and accepted-row limit options. Checkpoint
/// values are not implicitly cloned; drop them to release their root charge.
pub struct RecordCheckpoint {
    source: std::sync::Arc<crate::source::Source>,
    symbol: SymbolId,
    position: DecoderPosition,
    _state: Reservation,
}
impl RecordCheckpoint {
    /// Number of raw public rows preceding this point, including a synthesized
    /// empty scalar's default row or universe rows. Not a byte offset.
    pub fn row_ordinal(&self) -> u64 {
        self.position.row_ordinal()
    }
}
impl std::fmt::Debug for RecordCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordCheckpoint")
            .field("symbol", &self.symbol)
            .field("row_ordinal", &self.row_ordinal())
            .finish_non_exhaustive()
    }
}

pub struct ScanCursor {
    reader: Reader,
    handle: SymbolHandle,
    key_mask: u32,
    value_mask: u8,
    capacity: usize,
    remaining: u64,
    selection: Option<Selection>,
    statistics: ScanStatistics,
    decoder: Option<Decoder>,
    position: Option<DecoderPosition>,
    batch: Option<Batch>,
    done: bool,
    error: Option<Error>,
    _state: Reservation,
}
impl ScanCursor {
    /// Capture an unselected raw scan boundary after the last returned batch.
    /// All values/keys have been decoded regardless of projection. Before the
    /// first batch this explicitly reads only the data preamble/read-ahead, not
    /// a symbol prescan. Ordinary zero-limit scans remain lazy.
    ///
    /// Selected cursors return InvalidOptions without changing their state.
    /// Root reservation denial also leaves the cursor usable. I/O/source/decode
    /// failures are terminal, as for next_batch; no partial checkpoint escapes.
    ///
    /// ```compile_fail,E0499
    /// # use gdxrs::{ScanCursor, Result};
    /// fn retain(cursor: &mut ScanCursor) -> Result<()> {
    ///     let batch = cursor.next_batch()?.unwrap();
    ///     let point = cursor.checkpoint()?;
    ///     println!("{}", batch.len()); // batch still borrows cursor
    ///     Ok(())
    /// }
    /// ```
    pub fn checkpoint(&mut self) -> Result<RecordCheckpoint> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.selection.is_some() {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "selected checkpoint export is not supported",
            ));
        }
        let state = self
            .reader
            .source
            .budget
            .reserve(size_of::<RecordCheckpoint>())?;
        let result = (|| {
            self.reader.source.check_unchanged()?;
            let position = if let Some(decoder) = &mut self.decoder {
                decoder.checkpoint()?
            } else if let Some(position) = self.position {
                position
            } else {
                let mut decoder = Decoder::new(&self.reader, &self.handle.inner.record)?;
                let point = decoder.checkpoint()?;
                self.position = Some(point);
                if !self.done && self.remaining != 0 {
                    self.decoder = Some(decoder);
                }
                point
            };
            self.reader.source.check_unchanged()?;
            Ok(RecordCheckpoint {
                source: std::sync::Arc::clone(&self.reader.source),
                symbol: self.handle.id(),
                position,
                _state: state,
            })
        })();
        result.map_err(|error| self.fail(error))
    }
    fn fail(&mut self, mut error: Error) -> Error {
        error.source_id = Some(self.reader.source.id);
        error.symbol_id = Some(self.handle.id().get());
        self.decoder = None;
        self.batch = None;
        self.position = None;
        self.error = Some(error.clone());
        error
    }
    pub fn statistics(&self) -> ScanStatistics {
        self.statistics
    }
    pub(crate) fn selected(
        reader: Reader,
        handle: SymbolHandle,
        selection: Selection,
        options: ScanOptions,
    ) -> Result<Self> {
        let mut cursor = Self::new(reader, handle, options)?;
        cursor.selection = Some(selection);
        Ok(cursor)
    }
    pub(crate) fn new(reader: Reader, handle: SymbolHandle, options: ScanOptions) -> Result<Self> {
        reader.check_handle(&handle)?;
        let resolved = reader.resolve(&handle.inner.record)?;
        let allowed_keys = (1u32 << resolved.dimension) - 1;
        let allowed_values = (1u8 << resolved.kind.fields()) - 1;
        let key_mask = match options.key_projection {
            KeyProjection::All => allowed_keys,
            KeyProjection::Mask(m) => m,
        };
        let value_mask = match options.value_projection {
            ValueProjection::All => allowed_values,
            ValueProjection::Mask(m) => m,
        };
        if key_mask & !allowed_keys != 0 || value_mask & !allowed_values != 0 {
            return Err(Error::new(
                ErrorKind::InvalidProjection,
                "projection exceeds symbol arity",
            )
            .symbol(handle.id().get()));
        }
        if options.batch_bytes == 0
            || options.batch_bytes > reader.source.options.maximum_batch_bytes
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "batch cap outside reader maximum",
            ));
        }
        let payload = (key_mask.count_ones() as usize)
            .checked_mul(size_of::<UelId>())
            .and_then(|n| n.checked_add(value_mask.count_ones() as usize * size_of::<Value>()))
            .ok_or_else(|| Error::new(ErrorKind::BatchTooSmall, "row layout overflow"))?;
        let capacity = options
            .batch_bytes
            .checked_div(payload)
            .unwrap_or(65_536)
            .min(65_536);
        if capacity == 0 {
            return Err(Error::new(
                ErrorKind::BatchTooSmall,
                "batch cannot hold one projected row",
            ));
        }
        // Includes inline decoder/column headers; decoder/stream reservations also
        // conservatively charge their own fixed state while active.
        if size_of::<Self>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "scan state exceeds audited bound",
            ));
        }
        let state = reader.source.budget.reserve(size_of::<Self>())?;
        Ok(Self {
            reader,
            handle,
            key_mask,
            value_mask,
            capacity,
            remaining: options.limit.unwrap_or(u64::MAX),
            selection: None,
            statistics: ScanStatistics::default(),
            decoder: None,
            position: None,
            batch: None,
            done: false,
            error: None,
            _state: state,
        })
    }
    fn fill(&mut self) -> Result<bool> {
        if self.done {
            return Ok(false);
        }
        if self.remaining == 0 || self.selection.as_ref().is_some_and(Selection::is_empty) {
            self.done = true;
            self.decoder = None;
            self.batch = None;
            return Ok(false);
        }
        self.reader.source.check_unchanged()?;
        if self.decoder.is_none() {
            self.decoder = Some(if let Some(position) = &self.position {
                Decoder::resume(&self.reader, &self.handle.inner.record, position)?
            } else {
                Decoder::new(&self.reader, &self.handle.inner.record)?
            });
        }
        if let Some(batch) = &mut self.batch {
            batch.clear();
        }
        let capacity = self
            .capacity
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let decoder = self
            .decoder
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "missing decoder"))?;
        let mut accepted = 0;
        while accepted < capacity {
            let Some(row) = decoder.next()? else {
                self.done = true;
                break;
            };
            self.statistics.decoded_rows =
                self.statistics.decoded_rows.checked_add(1).ok_or_else(|| {
                    Error::new(ErrorKind::InvalidCount, "decoded row counter overflow")
                })?;
            if let Some(selection) = &self.selection
                && !selection.matches(row)?
            {
                continue;
            }
            if self.batch.is_none() {
                self.batch = Some(Batch::new(
                    &self.reader,
                    self.key_mask,
                    self.value_mask,
                    capacity,
                )?);
            }
            let batch = self
                .batch
                .as_mut()
                .ok_or_else(|| Error::new(ErrorKind::AllocationFailed, "missing batch"))?;
            batch.push(row)?;
            self.remaining -= 1;
            accepted += 1;
        }
        if self.done || self.remaining == 0 {
            self.position = Some(decoder.position()?);
            self.decoder = None;
        }
        let nonempty = self.batch.as_ref().is_some_and(|b| b.rows != 0);
        if !nonempty {
            self.batch = None;
        }
        self.statistics.returned_rows = self
            .statistics
            .returned_rows
            .checked_add(accepted as u64)
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "returned row counter overflow"))?;
        Ok(nonempty)
    }
    pub fn next_batch(&mut self) -> Result<Option<BatchView<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        match self.fill() {
            Ok(true) => Ok(self.batch.as_ref().map(|batch| BatchView { batch })),
            Ok(false) => {
                self.batch = None;
                Ok(None)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
}
impl Reader {
    /// Start an unselected raw suffix scan in the same immutable open source.
    /// No rows preceding the point are decoded. The bounded data preamble is
    /// revalidated, and a compressed restart block is fully checksum-verified.
    /// Options and counters start fresh; limits count newly accepted rows.
    pub fn scan_from_checkpoint(
        &self,
        symbol: &SymbolHandle,
        point: &RecordCheckpoint,
        options: ScanOptions,
    ) -> Result<ScanCursor> {
        self.check_handle(symbol)?;
        if !std::sync::Arc::ptr_eq(&self.source, &point.source) || symbol.id() != point.symbol {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "checkpoint belongs to another source/symbol",
            )
            .symbol(symbol.id().get()));
        }
        self.source.check_unchanged()?;
        let mut scan = ScanCursor::new(self.clone(), symbol.clone(), options)?;
        scan.position = Some(point.position);
        Ok(scan)
    }
    /// Defaults using this reader's configured maximum batch payload.
    pub fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            batch_bytes: self.source.options.maximum_batch_bytes,
            ..ScanOptions::default()
        }
    }
    pub fn scan(&self, symbol: &SymbolHandle, options: ScanOptions) -> Result<ScanCursor> {
        ScanCursor::new(self.clone(), symbol.clone(), options)
    }
}

#[cfg(all(test, feature = "compression"))]
mod tests {
    use super::*;
    use crate::source::{ReadAt, Source};
    use std::{
        io,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };
    pub(super) struct Scripted {
        pub(super) bytes: Vec<u8>,
        pub(super) fail_at: Arc<AtomicU64>,
        pub(super) ranges: Arc<Mutex<Vec<(u64, usize)>>>,
    }
    impl ReadAt for Scripted {
        fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
            if at >= self.fail_at.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected data read failure",
                ));
            }
            let rest = self.bytes.get(at as usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len()).min(7);
            out[..n].copy_from_slice(&rest[..n]);
            self.ranges.lock().unwrap().push((at, n));
            Ok(n)
        }
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    #[test]
    fn filtered_fill_failure_exposes_no_partial_batch_and_reports_decoded_work() {
        let bytes = std::fs::read("tests/fixtures/v7/blocks.gdx").unwrap();
        let len = bytes.len() as u64;
        let fail_at = Arc::new(AtomicU64::new(u64::MAX));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let source = Source::testing(
            Box::new(Scripted {
                bytes,
                fail_at: Arc::clone(&fail_at),
                ranges: Arc::clone(&ranges),
            }),
            len,
        );
        let budget = Arc::clone(&source.budget);
        let reader = Reader::from_source(source).unwrap();
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let bounds = [
            UelRange::new(UelId::new(1).unwrap(), UelId::new(250).unwrap()).unwrap(),
            UelRange::new(UelId::new(4000).unwrap(), UelId::new(4500).unwrap()).unwrap(),
        ];
        let selection = reader
            .selection(
                &symbol,
                &[DimensionSelection {
                    dimension: 0,
                    keys: KeySelection::Ranges(&bounds),
                }],
            )
            .unwrap();
        ranges.lock().unwrap().clear();
        let empty = reader
            .selection(
                &symbol,
                &[DimensionSelection {
                    dimension: 0,
                    keys: KeySelection::AnyOf(&[]),
                }],
            )
            .unwrap();
        let mut no_rows = reader
            .scan_selected(&symbol, &empty, reader.scan_options())
            .unwrap();
        let held = budget.statistics().current_bytes;
        assert!(no_rows.next_batch().unwrap().is_none());
        assert!(ranges.lock().unwrap().is_empty());
        assert_eq!(budget.statistics().current_bytes, held);
        drop(no_rows);
        drop(empty);
        let baseline = budget.statistics().current_bytes;
        let mut cursor = reader
            .scan_selected(
                &symbol,
                &selection,
                ScanOptions {
                    batch_bytes: 4096,
                    ..reader.scan_options()
                },
            )
            .unwrap();
        let first = cursor.next_batch().unwrap().unwrap();
        let published = first.len();
        assert_eq!(published, 4096 / (size_of::<UelId>() + size_of::<Value>()));
        assert_eq!(
            first.key_column(0).unwrap().last().unwrap().get(),
            published as u32
        );
        fail_at.store(25609, Ordering::Relaxed);
        let error = cursor.next_batch().err().unwrap();
        assert_eq!(error.kind, ErrorKind::Io);
        assert_eq!(error.symbol_id, Some(1));
        assert_eq!(error.physical_offset, Some(25609));
        assert!(std::error::Error::source(&error).is_some());
        let stats = cursor.statistics();
        assert!(stats.decoded_rows > 250 && stats.decoded_rows < 4000);
        assert_eq!(stats.returned_rows, published as u64);
        assert_eq!(cursor.next_batch().err().unwrap().kind, ErrorKind::Io);
        assert_eq!(cursor.statistics(), stats);
        assert_eq!(
            budget.statistics().current_bytes,
            baseline + size_of::<ScanCursor>()
        );
        drop(cursor);
        assert_eq!(budget.statistics().current_bytes, baseline);
        drop(reader);
        drop(symbol);
        assert!(budget.statistics().current_bytes > 0); // Selection retains source.
        drop(selection);
        assert_eq!(budget.statistics().current_bytes, 0);
        eprintln!(
            "M2 filtered fault: decoded={}, returned={}, peak={}",
            stats.decoded_rows,
            stats.returned_rows,
            budget.statistics().peak_bytes
        );
    }
    #[test]
    fn first_batch_zero_limit_short_reads_and_terminal_io_failure() {
        let bytes = std::fs::read("tests/fixtures/v7/blocks.gdx").unwrap();
        let len = bytes.len() as u64;
        let fail_at = Arc::new(AtomicU64::new(u64::MAX));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let source = Source::testing(
            Box::new(Scripted {
                bytes,
                fail_at: Arc::clone(&fail_at),
                ranges: Arc::clone(&ranges),
            }),
            len,
        );
        let reader = Reader::from_source(Arc::clone(&source)).unwrap();
        let handle = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let baseline = reader.budget_statistics().current_bytes;
        ranges.lock().unwrap().clear();
        let mut zero = reader
            .scan(
                &handle,
                ScanOptions {
                    limit: Some(0),
                    ..reader.scan_options()
                },
            )
            .unwrap();
        let held = reader.budget_statistics().current_bytes;
        assert!(zero.next_batch().unwrap().is_none());
        assert!(ranges.lock().unwrap().is_empty());
        assert_eq!(reader.budget_statistics().current_bytes, held);
        drop(zero);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
        let mut cursor = reader
            .scan(
                &handle,
                ScanOptions {
                    batch_bytes: 4096,
                    ..reader.scan_options()
                },
            )
            .unwrap();
        let first = cursor.next_batch().unwrap().unwrap();
        assert_eq!(first.key_column(0).unwrap()[0].get(), 1);
        assert_eq!(
            first.value_column(ValueField::Level).unwrap()[0],
            Value::NumberBits(0x3ff32d2335df4552)
        );
        let mut accepted = first.len();
        let header = source.header().unwrap();
        let observed = ranges.lock().unwrap();
        assert!(
            observed
                .iter()
                .all(|(start, n)| *start >= header.data_start && *start + *n as u64 <= 25609)
        );
        assert!(
            observed.iter().map(|(_, n)| n).sum::<usize>()
                < (header.offsets[0] - header.data_start) as usize
        );
        drop(observed);
        let mut limited = reader
            .scan(
                &handle,
                ScanOptions {
                    limit: Some(13),
                    batch_bytes: 80,
                    ..reader.scan_options()
                },
            )
            .unwrap();
        let mut count = 0;
        while let Some(batch) = limited.next_batch().unwrap() {
            for key in batch.key_column(0).unwrap() {
                count += 1;
                assert_eq!(key.get(), count);
            }
        }
        assert_eq!(count, 13);
        drop(limited);
        let mut empty = reader
            .scan(
                &handle,
                ScanOptions {
                    key_projection: KeyProjection::Mask(0),
                    value_projection: ValueProjection::Mask(0),
                    limit: Some(12),
                    ..reader.scan_options()
                },
            )
            .unwrap();
        assert_eq!(empty.next_batch().unwrap().unwrap().len(), 12);
        assert!(empty.next_batch().unwrap().is_none());
        drop(empty);
        // Fail the second block after many successful batches. A failing fill
        // contains decoded rows internally, but returns only Err, never that prefix.
        fail_at.store(25609, Ordering::Relaxed);
        let error = loop {
            match cursor.next_batch() {
                Ok(Some(batch)) => accepted += batch.len(),
                Ok(None) => panic!("fault was not reached"),
                Err(e) => break e,
            }
        };
        assert!(accepted > 0 && accepted < 5000);
        assert_eq!(error.kind, ErrorKind::Io);
        assert_eq!(error.source_id, Some(source.id));
        assert_eq!(error.symbol_id, Some(1));
        assert_eq!(error.physical_offset, Some(25609));
        assert!(std::error::Error::source(&error).is_some());
        let repeated = cursor.next_batch().err().unwrap();
        assert_eq!(repeated.kind, error.kind);
        assert_eq!(repeated.physical_offset, error.physical_offset);
        assert_eq!(
            reader.budget_statistics().current_bytes,
            baseline + size_of::<ScanCursor>()
        );
        drop(cursor);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
        assert_eq!(size_of::<UelId>(), 4);
        eprintln!(
            "M1 layouts: ScanCursor={}, Decoder={}, Value={}, UelId={}; fault after {accepted} published rows; peak={}",
            size_of::<ScanCursor>(),
            size_of::<Decoder>(),
            size_of::<Value>(),
            size_of::<UelId>(),
            reader.budget_statistics().peak_bytes
        );
    }
}

#[cfg(test)]
mod checkpoint_tests;
