use super::*;

fn indexed(
    reader: &Reader,
    symbol: &SymbolHandle,
    stride: u64,
) -> (common::TempFile, CheckpointIndex) {
    let file = common::TempFile::new(&[]);
    std::fs::remove_file(&file.path).unwrap();
    let options = IndexOptions {
        record_stride: stride,
        decoded_byte_stride: 1024,
        ..IndexOptions::default()
    };
    reader
        .index_builder(symbol, &file.path, options)
        .unwrap()
        .finish()
        .unwrap();
    let index = reader
        .open_index(symbol, &file.path, options, IndexPolicy::UseIfValid)
        .unwrap()
        .index
        .unwrap();
    (file, index)
}
fn reader(path: &std::path::Path) -> Result<Reader> {
    Reader::open(
        path,
        OpenOptions {
            memory_budget_bytes: 16 * 1024 * 1024,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 4096,
        },
    )
}

#[test]
fn parallel_ranges_match_all_frozen_native_symbols_in_order() {
    let mut fixtures = 0;
    let mut records = 0;
    let mut workers = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|s| s != "gdx") {
            continue;
        }
        let reader = match reader(&path) {
            Ok(r) => r,
            Err(e)
                if !cfg!(feature = "compression")
                    && e.kind == ErrorKind::CompressionUnavailable =>
            {
                continue;
            }
            Err(e) => panic!("{e}"),
        };
        let text = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        for id in 0..=reader.information().unwrap().symbol_count {
            let symbol = reader.symbol(SymbolId::new(id).unwrap()).unwrap();
            let prefix = format!("R\t{id}\t");
            let expected: Vec<_> = text.lines().filter(|s| s.starts_with(&prefix)).collect();
            let (_file, index) =
                indexed(&reader, &symbol, if expected.len() < 64 { 2 } else { 509 });
            let mut cursor = reader
                .scan_parallel(
                    &symbol,
                    index,
                    reader.scan_options(),
                    ParallelOptions::default(),
                )
                .unwrap();
            let mut at = 0;
            while let Some(batch) = cursor.next_batch().unwrap() {
                let actual = super::ranges::rows(&batch, id);
                assert_eq!(
                    actual,
                    expected[at..at + actual.len()],
                    "{} symbol {id}",
                    path.display()
                );
                at += actual.len();
            }
            assert_eq!(at, expected.len());
            let s = cursor.statistics();
            assert!(s.complete && !s.cancelled);
            assert_eq!(s.returned_rows, at as u64);
            assert_eq!(s.decoded_rows, at as u64);
            assert_eq!(s.produced_rows, at as u64);
            assert_eq!(s.joined_workers, s.admitted_workers);
            assert!(s.admitted_workers <= 2);
            assert!(s.peak_ready_batches <= s.maximum_buffered_batches);
            assert_eq!(s.tasks_completed, s.tasks_dispatched);
            cursor.close().unwrap();
            assert!(cursor.next_batch().unwrap().is_none());
            workers += s.joined_workers;
            records += at;
        }
        assert!(reader.budget_statistics().peak_bytes <= 16 * 1024 * 1024);
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
    if cfg!(feature = "compression") {
        assert_eq!(records, 141358);
    }
    eprintln!(
        "M6 parallel corpus: {fixtures} fixtures, {records} ordered rows, {workers} joined workers"
    );
}

#[test]
fn parallel_global_limit_truncates_the_last_projected_batch() {
    let reader = reader(std::path::Path::new("tests/fixtures/v7/semantics-0.gdx")).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let selection = reader.selection(&symbol, &[]).unwrap();
    let (_file, index) = indexed(&reader, &symbol, 2);
    let mut cursor = reader
        .scan_selected_parallel(
            &symbol,
            &selection,
            index,
            ScanOptions {
                limit: Some(3),
                value_projection: ValueProjection::Mask(0),
                ..reader.scan_options()
            },
            ParallelOptions::default(),
        )
        .unwrap();
    let mut keys = Vec::new();
    let mut batches = Vec::new();
    while let Some(batch) = cursor.next_batch().unwrap() {
        assert!(batch.value_fields().is_empty());
        assert_eq!(batch.key_column(0).unwrap().len(), batch.len());
        batches.push(batch.len());
        keys.extend(batch.key_column(0).unwrap().iter().map(|k| k.get()));
    }
    assert_eq!(keys, [1, 2, 3]);
    assert_eq!(batches, [2, 1]);
    let s = cursor.statistics();
    assert_eq!(s.returned_rows, 3);
    assert!(s.decoded_rows >= 3 && s.produced_rows >= 3 && s.complete && !s.cancelled);
    assert_eq!(s.joined_workers, s.admitted_workers);
    cursor.close().unwrap();
    assert!(cursor.statistics().complete);
}

#[test]
fn parallel_exact_selection_and_projection_keep_order_and_limit() {
    let reader = reader(std::path::Path::new("tests/fixtures/v7/key-widths.gdx")).unwrap();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let ranges = [
        UelRange::new(UelId::new(50).unwrap(), UelId::new(54).unwrap()).unwrap(),
        UelRange::new(UelId::new(65530).unwrap(), UelId::new(65535).unwrap()).unwrap(),
    ];
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Ranges(&ranges),
            }],
        )
        .unwrap();
    let (_file, index) = indexed(&reader, &symbol, 509);
    let mut cursor = reader
        .scan_selected_parallel(
            &symbol,
            &selection,
            index,
            ScanOptions {
                limit: Some(3),
                key_projection: KeyProjection::Mask(0),
                ..reader.scan_options()
            },
            ParallelOptions::default(),
        )
        .unwrap();
    let mut count = 0;
    while let Some(batch) = cursor.next_batch().unwrap() {
        assert!(batch.key_dimensions().is_empty());
        let values = batch.value_column(ValueField::Level).unwrap();
        assert_eq!(values.len(), batch.len());
        assert!(values.iter().all(|v| *v == Value::ZERO));
        count += batch.len();
    }
    assert_eq!(count, 3);
    assert_eq!(cursor.statistics().returned_rows, 3);
}

#[test]
fn parallel_zero_limit_and_empty_selection_do_not_launch_or_read_records() {
    let reader = reader(std::path::Path::new("tests/fixtures/v7/semantics-0.gdx")).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::AnyOf(&[]),
            }],
        )
        .unwrap();
    for empty in [false, true] {
        let (_file, index) = indexed(&reader, &symbol, 2);
        let before = reader.io_statistics();
        let mut cursor = if empty {
            reader.scan_selected_parallel(
                &symbol,
                &selection,
                index,
                reader.scan_options(),
                ParallelOptions::default(),
            )
        } else {
            reader.scan_parallel(
                &symbol,
                index,
                ScanOptions {
                    limit: Some(0),
                    ..reader.scan_options()
                },
                ParallelOptions::default(),
            )
        }
        .unwrap();
        assert!(cursor.next_batch().unwrap().is_none());
        assert_eq!(cursor.statistics().admitted_workers, 0);
        assert_eq!(cursor.statistics().decoded_rows, 0);
        assert!(cursor.statistics().complete);
        assert_eq!(reader.io_statistics().bytes_read, before.bytes_read);
        cursor.close().unwrap();
    }
}

#[test]
fn parallel_cancel_and_drop_join_workers_and_release_bulk_root() {
    let reader = reader(std::path::Path::new("tests/fixtures/v7/key-widths.gdx")).unwrap();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    for external in [false, true] {
        let (_file, index) = indexed(&reader, &symbol, 509);
        let mut cursor = reader
            .scan_parallel(
                &symbol,
                index,
                reader.scan_options(),
                ParallelOptions::default(),
            )
            .unwrap();
        assert!(cursor.next_batch().unwrap().is_some());
        let returned = cursor.statistics().returned_rows;
        if external {
            cursor.cancellation_handle().request_cancel();
            assert!(cursor.next_batch().unwrap().is_none());
        } else {
            cursor.cancel().unwrap();
        }
        let s = cursor.statistics();
        assert!(s.cancelled && !s.complete);
        assert_eq!(s.returned_rows, returned);
        assert_eq!(s.joined_workers, s.admitted_workers);
        cursor.close().unwrap();
        drop(cursor);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    let (_file, index) = indexed(&reader, &symbol, 509);
    let cursor = reader
        .scan_parallel(
            &symbol,
            index,
            reader.scan_options(),
            ParallelOptions::default(),
        )
        .unwrap();
    drop(cursor);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_validated_session_reuse_does_not_repeat_source_hashing() {
    let reader = reader(std::path::Path::new("tests/fixtures/v7/key-widths.gdx")).unwrap();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let (_file, index) = indexed(&reader, &symbol, 509);
    let validation = index.validation_statistics();
    let mut cursor = reader
        .scan_parallel(
            &symbol,
            index,
            ScanOptions {
                limit: Some(3),
                ..reader.scan_options()
            },
            ParallelOptions::default(),
        )
        .unwrap();
    while cursor.next_batch().unwrap().is_some() {}
    let index = cursor.into_index().unwrap();
    assert_eq!(
        index.validation_statistics().source_bytes_hashed,
        validation.source_bytes_hashed
    );
    let before = reader.io_statistics().bytes_read;
    let mut next = reader
        .scan_parallel(
            &symbol,
            index,
            ScanOptions {
                limit: Some(3),
                ..reader.scan_options()
            },
            ParallelOptions::default(),
        )
        .unwrap();
    let mut count = 0;
    while let Some(batch) = next.next_batch().unwrap() {
        count += batch.len();
    }
    assert_eq!(count, 3);
    // Universe scans use synthetic rows. Any full-source identity pass here
    // would be visible as GDX source reads rather than just sidecar pages.
    assert_eq!(reader.io_statistics().bytes_read, before);
    next.close().unwrap();
}
