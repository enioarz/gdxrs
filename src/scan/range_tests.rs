use super::*;
use crate::source::{ReadAt, Source};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Control {
    bytes: Arc<Mutex<Vec<u8>>>,
    unchanged: AtomicBool,
    fail: AtomicBool,
    reads: AtomicUsize,
}
struct Backend(Arc<Control>);
impl ReadAt for Backend {
    fn unchanged(&self) -> io::Result<bool> {
        Ok(self.0.unchanged.load(Ordering::Relaxed))
    }
    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
        self.0.reads.fetch_add(1, Ordering::Relaxed);
        if self.0.fail.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected range read failure",
            ));
        }
        let data = self.0.bytes.lock().unwrap();
        let tail = data.get(at as usize..).unwrap_or(&[]);
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        Ok(n)
    }
}
fn fixture() -> (Reader, Arc<Control>) {
    fixture_at("tests/fixtures/v7/semantics-0.gdx")
}
fn fixture_at(path: &str) -> (Reader, Arc<Control>) {
    let data = std::fs::read(path).unwrap();
    let length = data.len() as u64;
    let control = Arc::new(Control {
        bytes: Arc::new(Mutex::new(data)),
        unchanged: AtomicBool::new(true),
        fail: AtomicBool::new(false),
        reads: AtomicUsize::new(0),
    });
    let reader = Reader::from_source(Source::testing(
        Box::new(Backend(Arc::clone(&control))),
        length,
    ))
    .unwrap();
    (reader, control)
}

#[test]
fn range_revalidates_private_restore_identity() {
    let (reader, _) = fixture();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut local = point(&reader, &symbol);
    let (other, _) = fixture();
    let foreign = other.symbol(symbol.id()).unwrap();
    local.position = point(&other, &foreign).position;
    let mut cursor = reader
        .scan_range(&symbol, &local, 3, reader.scan_options())
        .unwrap();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(cursor.statistics(), ScanStatistics::default());
}

#[cfg(feature = "compression")]
#[test]
fn range_verifies_entire_compressed_restart_block_checksum() {
    let (reader, control) = fixture_at("tests/fixtures/v7/semantics-1.gdx");
    let symbol = reader.symbol(SymbolId::new(7).unwrap()).unwrap();
    let mut seed = reader
        .scan(
            &symbol,
            ScanOptions {
                limit: Some(8),
                ..reader.scan_options()
            },
        )
        .unwrap();
    while seed.next_batch().unwrap().is_some() {}
    let point = seed.checkpoint().unwrap();
    assert_eq!(point.row_ordinal(), 8);
    drop(seed);
    // Corrupt the actual record frame's Adler checksum, not a metadata frame.
    // The backend deliberately hides mutation to isolate checksum validation.
    let at = symbol.inner.record.position as usize;
    let mut bytes = control.bytes.lock().unwrap();
    assert!(symbol.inner.record.compressed);
    assert_eq!(bytes[at], 1);
    let length = usize::from(u16::from_be_bytes([bytes[at + 1], bytes[at + 2]]));
    bytes[at + 3 + length - 1] ^= 1;
    drop(bytes);
    let mut cursor = reader
        .scan_range(&symbol, &point, 9, reader.scan_options())
        .unwrap();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCompression
    );
    assert_eq!(cursor.statistics(), ScanStatistics::default());
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCompression
    );
}
fn point(reader: &Reader, symbol: &SymbolHandle) -> RecordCheckpoint {
    reader
        .scan(symbol, reader.scan_options())
        .unwrap()
        .checkpoint()
        .unwrap()
}

#[test]
fn small_range_admits_only_its_possible_rows_and_still_checks_eof() {
    let reader = Reader::open(
        "tests/fixtures/v7/semantics-0.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let point = point(&reader, &symbol);
    // Default maximum batch payload is itself 1 MiB. The range has only three
    // possible output rows and must leave room for the decoder under this root.
    let mut cursor = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    assert_eq!(cursor.next_batch().unwrap().unwrap().len(), 3);
    assert!(cursor.next_batch().unwrap().is_none());
    let mut full = reader
        .scan_range(&symbol, &point, 5, reader.scan_options())
        .unwrap();
    assert_eq!(full.next_batch().unwrap().unwrap().len(), 5);
    assert!(!full.done); // Full final batch still owes EOF framing.
    assert!(full.next_batch().unwrap().is_none());
    assert!(full.done);
    assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
}

#[test]
fn range_eof_terminator_is_checked_but_interior_and_explicit_limits_stop() {
    let (reader, control) = fixture();
    let symbol = reader.symbol(SymbolId::new(7).unwrap()).unwrap();
    let point = point(&reader, &symbol);
    let next = reader.symbol(SymbolId::new(8).unwrap()).unwrap();
    let terminator = next.inner.record.position as usize - 1;
    assert_eq!(control.bytes.lock().unwrap()[terminator], 255);
    // This test backend deliberately reports unchanged to isolate framing.
    control.bytes.lock().unwrap()[terminator] = 1;
    let options = ScanOptions {
        batch_bytes: 2 * size_of::<UelId>() + size_of::<Value>(),
        ..reader.scan_options()
    };
    let mut interior = reader
        .scan_range(&symbol, &point, 15, options.clone())
        .unwrap();
    while interior.next_batch().unwrap().is_some() {}
    assert_eq!(interior.statistics().decoded_rows, 15);
    assert_eq!(interior.checkpoint().unwrap().row_ordinal(), 15);
    let mut limited = reader
        .scan_range(
            &symbol,
            &point,
            16,
            ScanOptions {
                limit: Some(16),
                ..options.clone()
            },
        )
        .unwrap();
    while limited.next_batch().unwrap().is_some() {}
    assert_eq!(limited.statistics().returned_rows, 16);
    let mut full = reader.scan_range(&symbol, &point, 16, options).unwrap();
    for _ in 0..16 {
        assert_eq!(full.next_batch().unwrap().unwrap().len(), 1)
    }
    assert_eq!(
        full.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
    assert_eq!(
        full.statistics(),
        ScanStatistics {
            decoded_rows: 16,
            returned_rows: 16
        }
    );
    assert_eq!(
        full.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
    assert_eq!(
        full.checkpoint().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
    // Empty ranges at EOF do not owe a record read, even with pending framing.
    let end = limited.checkpoint().unwrap();
    let before = control.reads.load(Ordering::Relaxed);
    let mut empty = reader
        .scan_range(&symbol, &end, 16, reader.scan_options())
        .unwrap();
    assert!(empty.next_batch().unwrap().is_none());
    assert_eq!(control.reads.load(Ordering::Relaxed), before);
}

#[test]
fn range_state_and_restart_ownership_share_root_and_denials_are_fallible() {
    let (reader, _) = fixture();
    let source = Arc::clone(&reader.source);
    let budget = Arc::clone(&source.budget);
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let point = point(&reader, &symbol);
    let base = budget.statistics().current_bytes;
    let free = budget.statistics().limit_bytes - base;
    let block = budget.reserve(free - size_of::<ScanCursor>() + 1).unwrap();
    assert_eq!(
        reader
            .scan_range(&symbol, &point, 3, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::BudgetTooSmall
    );
    drop(block);
    assert_eq!(budget.statistics().current_bytes, base);
    let mut cursor = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    assert_eq!(
        budget.statistics().current_bytes,
        base + size_of::<ScanCursor>()
    );
    let free = budget.statistics().limit_bytes - budget.statistics().current_bytes;
    let block = budget.reserve(free).unwrap();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    drop(block);
    assert_eq!(
        budget.statistics().current_bytes,
        base + size_of::<ScanCursor>()
    );
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    assert_eq!(cursor.statistics(), ScanStatistics::default());
    drop(cursor);
    let mut cursor = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    drop(point);
    drop(reader);
    drop(symbol);
    assert_eq!(cursor.next_batch().unwrap().unwrap().len(), 3);
    assert!(cursor.next_batch().unwrap().is_none());
    drop(cursor);
    assert_eq!(budget.statistics().current_bytes, 16 * 1024);
    drop(source);
    assert_eq!(budget.statistics().current_bytes, 0);
}

#[test]
fn range_source_changes_are_terminal_and_preserve_acknowledged_prefix() {
    let (reader, control) = fixture();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let point = point(&reader, &symbol);
    control.unchanged.store(false, Ordering::Relaxed);
    assert_eq!(
        reader
            .scan_range(&symbol, &point, 0, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceChanged
    );
    control.unchanged.store(true, Ordering::Relaxed);
    let mut cursor = reader
        .scan_range(
            &symbol,
            &point,
            3,
            ScanOptions {
                batch_bytes: size_of::<UelId>() + size_of::<Value>(),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert_eq!(cursor.next_batch().unwrap().unwrap().len(), 1);
    control.unchanged.store(false, Ordering::Relaxed);
    let error = cursor.next_batch().err().unwrap();
    assert_eq!(error.kind, ErrorKind::SourceChanged);
    assert_eq!(error.symbol_id, Some(1));
    assert_eq!(
        cursor.statistics(),
        ScanStatistics {
            decoded_rows: 1,
            returned_rows: 1
        }
    );
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
}

#[test]
fn range_lazy_zero_empty_selection_and_restore_io_failure() {
    let (reader, control) = fixture();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let point = point(&reader, &symbol);
    let empty = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::AnyOf(&[]),
            }],
        )
        .unwrap();
    control.fail.store(true, Ordering::Relaxed);
    let before = control.reads.load(Ordering::Relaxed);
    let mut zero = reader
        .scan_range(
            &symbol,
            &point,
            3,
            ScanOptions {
                limit: Some(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert!(zero.next_batch().unwrap().is_none());
    let mut selected = reader
        .scan_selected_range(&symbol, &empty, &point, 3, reader.scan_options())
        .unwrap();
    assert!(selected.next_batch().unwrap().is_none());
    assert_eq!(control.reads.load(Ordering::Relaxed), before);
    let mut cursor = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    assert_eq!(cursor.next_batch().err().unwrap().kind, ErrorKind::Io);
    assert_eq!(cursor.statistics(), ScanStatistics::default());
    assert_eq!(cursor.next_batch().err().unwrap().kind, ErrorKind::Io);
    control.fail.store(false, Ordering::Relaxed);
    let mut retry = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    assert_eq!(retry.next_batch().unwrap().unwrap().len(), 3);
}
