use super::*;
#[test]
fn checkpoint_ownership_budget_denial_and_release_are_explicit() {
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions::default(),
    )
    .unwrap();
    let source = std::sync::Arc::clone(&reader.source);
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut scan = reader.scan(&symbol, reader.scan_options()).unwrap();
    let before = source.budget.statistics().current_bytes;
    let free = source.budget.statistics().limit_bytes - before;
    let block = source
        .budget
        .reserve(free - size_of::<RecordCheckpoint>() + 1)
        .unwrap();
    assert_eq!(
        scan.checkpoint().err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    assert_eq!(scan.statistics().decoded_rows, 0);
    drop(block);
    // Also deny decoder initialization after the checkpoint's own reservation
    // succeeds. This must remain a retryable export, not poison a lazy scan.
    let block = source
        .budget
        .reserve(free - size_of::<RecordCheckpoint>() - 1024)
        .unwrap();
    let before_io = source.statistics().bytes_read;
    assert_eq!(
        scan.checkpoint().err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    assert!(scan.error.is_none());
    assert_eq!(source.statistics().bytes_read, before_io);
    drop(block);
    assert!(scan.next_batch().unwrap().is_some());
    let before = source.budget.statistics().current_bytes;
    let point = scan.checkpoint().unwrap();
    assert_eq!(
        source.budget.statistics().current_bytes,
        before + size_of::<RecordCheckpoint>()
    );
    assert_eq!(point.row_ordinal(), 1);
    drop(scan);
    drop(reader);
    drop(symbol);
    assert!(source.budget.statistics().current_bytes >= size_of::<RecordCheckpoint>());
    drop(point);
    assert_eq!(source.budget.statistics().current_bytes, 16 * 1024);
}

#[cfg(feature = "compression")]
#[test]
fn resumed_failure_is_sticky_and_does_not_decode_the_record_prefix() {
    use crate::scan::tests::Scripted;
    use crate::source::Source;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    let bytes = std::fs::read("tests/fixtures/v7/blocks.gdx").unwrap();
    let len = bytes.len() as u64;
    let fail = Arc::new(AtomicU64::new(u64::MAX));
    let source = Source::testing(
        Box::new(Scripted {
            bytes,
            fail_at: Arc::clone(&fail),
            ranges: Arc::new(Mutex::new(Vec::new())),
        }),
        len,
    );
    let reader = Reader::from_source(source).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut scan = reader
        .scan(
            &symbol,
            ScanOptions {
                limit: Some(4000),
                ..reader.scan_options()
            },
        )
        .unwrap();
    while scan.next_batch().unwrap().is_some() {}
    let point = scan.checkpoint().unwrap();
    drop(scan);
    let mut suffix = reader
        .scan_from_checkpoint(
            &symbol,
            &point,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert!(suffix.next_batch().unwrap().is_some());
    assert_eq!(
        suffix.statistics(),
        ScanStatistics {
            decoded_rows: 1,
            returned_rows: 1
        }
    );
    assert_eq!(suffix.checkpoint().unwrap().row_ordinal(), 4001);
    drop(suffix);
    fail.store(25609, Ordering::Relaxed);
    let mut suffix = reader
        .scan_from_checkpoint(&symbol, &point, reader.scan_options())
        .unwrap();
    assert_eq!(suffix.next_batch().err().unwrap().kind, ErrorKind::Io);
    assert_eq!(suffix.checkpoint().err().unwrap().kind, ErrorKind::Io);
    assert_eq!(suffix.next_batch().err().unwrap().kind, ErrorKind::Io);
    assert_eq!(suffix.statistics(), ScanStatistics::default());
}

#[cfg(feature = "compression")]
#[test]
fn close_retains_the_acknowledged_prefix_for_later_checkpoint_export() {
    let reader = Reader::open("tests/fixtures/v7/blocks.gdx", OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut total_scan = reader.scan(&symbol, reader.scan_options()).unwrap();
    let mut total = 0u64;
    while let Some(batch) = total_scan.next_batch().unwrap() {
        total += batch.len() as u64;
    }
    let mut scan = reader.scan(&symbol, reader.scan_options()).unwrap();
    let acknowledged = scan.next_batch().unwrap().unwrap().len() as u64;
    assert!(acknowledged > 0);
    assert_eq!(scan.checkpoint().unwrap().row_ordinal(), acknowledged);
    scan.close().unwrap();
    let point = scan.checkpoint().unwrap();
    assert_eq!(point.row_ordinal(), acknowledged);
    assert!(scan.next_batch().unwrap().is_none());
    drop(scan);
    let mut suffix = reader
        .scan_from_checkpoint(&symbol, &point, reader.scan_options())
        .unwrap();
    let mut resumed = 0u64;
    while let Some(batch) = suffix.next_batch().unwrap() {
        resumed += batch.len() as u64;
    }
    assert_eq!(resumed, total - acknowledged);
}

#[cfg(feature = "compression")]
#[test]
fn close_after_resume_retains_the_extended_prefix() {
    let reader = Reader::open("tests/fixtures/v7/blocks.gdx", OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut head = reader
        .scan(
            &symbol,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
        )
        .unwrap();
    head.next_batch().unwrap().unwrap();
    let point = head.checkpoint().unwrap();
    assert_eq!(point.row_ordinal(), 1);
    drop(head);
    let mut suffix = reader
        .scan_from_checkpoint(&symbol, &point, reader.scan_options())
        .unwrap();
    let consumed = suffix.next_batch().unwrap().unwrap().len() as u64;
    assert!(consumed > 0);
    let before = suffix.checkpoint().unwrap().row_ordinal();
    assert_eq!(before, 1 + consumed);
    suffix.close().unwrap();
    assert_eq!(suffix.checkpoint().unwrap().row_ordinal(), before);
    assert!(suffix.next_batch().unwrap().is_none());
}
