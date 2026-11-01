use super::*;
use std::fmt::Write;

fn rows(batch: &BatchView<'_>, id: u32) -> Vec<String> {
    // Only the fixed, independently authored small fixture corpus is collected.
    (0..batch.len())
        .map(|n| {
            let row = batch.row(n).unwrap();
            let mut text = format!("R\t{id}\t");
            for (i, d) in batch.key_dimensions().iter().enumerate() {
                write!(
                    text,
                    "{}{}",
                    if i == 0 { "" } else { "," },
                    row.key(*d).unwrap().get()
                )
                .unwrap();
            }
            text.push('\t');
            for (i, f) in batch.value_fields().iter().enumerate() {
                write!(
                    text,
                    "{}{}",
                    if i == 0 { "" } else { "," },
                    common::value(row.value(*f).unwrap())
                )
                .unwrap();
            }
            text
        })
        .collect()
}

#[test]
fn split_ranges_recombine_every_frozen_native_symbol_exactly() {
    let mut fixtures = 0;
    let mut records = 0;
    let mut ranges = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|s| s != "gdx") {
            continue;
        }
        let reader = match Reader::open(
            &path,
            OpenOptions {
                memory_budget_bytes: 1024 * 1024,
                metadata_cache_bytes: 0,
                ..OpenOptions::default()
            },
        ) {
            Ok(r) => r,
            Err(e)
                if !cfg!(feature = "compression")
                    && e.kind == ErrorKind::CompressionUnavailable =>
            {
                continue;
            }
            Err(e) => panic!("{e}"),
        };
        let expected = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        for id in 0..=reader.information().unwrap().symbol_count {
            let prefix = format!("R\t{id}\t");
            let expected: Vec<_> = expected
                .lines()
                .filter(|s| s.starts_with(&prefix))
                .collect();
            let symbol = reader.symbol(SymbolId::new(id).unwrap()).unwrap();
            let mut seed = reader.scan(&symbol, reader.scan_options()).unwrap();
            let mut point = seed.checkpoint().unwrap();
            drop(seed);
            let mut at = 0;
            // Includes tiny cuts, interior cuts and source EOF; <=9 owned ranges
            // at a time (actually only one). No production partition collection.
            let step = if expected.len() < 64 {
                1
            } else {
                expected.len() / 7 + 1
            };
            while at < expected.len() {
                let end = (at + step).min(expected.len());
                let mut cursor = reader
                    .scan_range(
                        &symbol,
                        &point,
                        end as u64,
                        ScanOptions {
                            batch_bytes: 240,
                            ..reader.scan_options()
                        },
                    )
                    .unwrap();
                drop(point); // Cursor owns its restart tuple independently.
                let mut seen = at;
                while let Some(batch) = cursor.next_batch().unwrap() {
                    let actual = rows(&batch, id);
                    assert_eq!(
                        actual,
                        expected[seen..seen + actual.len()],
                        "{} symbol {id}",
                        path.display()
                    );
                    seen += actual.len();
                }
                assert_eq!(seen, end);
                assert_eq!(
                    cursor.statistics(),
                    ScanStatistics {
                        decoded_rows: (end - at) as u64,
                        returned_rows: (end - at) as u64
                    }
                );
                point = cursor.checkpoint().unwrap();
                assert_eq!(point.row_ordinal(), end as u64);
                ranges += 1;
                at = end;
            }
            let mut empty = reader
                .scan_range(&symbol, &point, at as u64, reader.scan_options())
                .unwrap();
            // Constructor may resolve an alias through catalog metadata. Lazy
            // consumption itself performs no source or record-codec work.
            let before = reader.io_statistics();
            assert!(empty.next_batch().unwrap().is_none());
            assert_eq!(empty.statistics(), ScanStatistics::default());
            assert_eq!(reader.io_statistics().decoded_bytes, before.decoded_bytes);
            records += at;
        }
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
    if cfg!(feature = "compression") {
        assert_eq!(records, 141358)
    }
    eprintln!("M6 range corpus: {fixtures} fixtures, {records} ordered rows, {ranges} partitions");
}

#[test]
fn range_selection_projection_and_limits_follow_raw_ordinals() {
    let reader = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut seed = reader
        .scan(
            &symbol,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert!(seed.next_batch().unwrap().is_some());
    let point = seed.checkpoint().unwrap();
    assert_eq!(point.row_ordinal(), 1);
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(3).unwrap()),
            }],
        )
        .unwrap();
    // Raw range [1,2) contains key 2, which is rejected. The following key 3
    // must never be decoded just to satisfy the accepted limit.
    let mut cursor = reader
        .scan_selected_range(
            &symbol,
            &selection,
            &point,
            2,
            ScanOptions {
                limit: Some(1),
                key_projection: KeyProjection::Mask(0),
                value_projection: ValueProjection::Mask(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert!(cursor.next_batch().unwrap().is_none());
    assert_eq!(
        cursor.statistics(),
        ScanStatistics {
            decoded_rows: 1,
            returned_rows: 0
        }
    );
    assert_eq!(
        cursor.checkpoint().err().unwrap().kind,
        ErrorKind::InvalidOptions
    );
    let mut cursor = reader
        .scan_selected_range(
            &symbol,
            &selection,
            &point,
            3,
            ScanOptions {
                limit: Some(1),
                key_projection: KeyProjection::Mask(0),
                value_projection: ValueProjection::Mask(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let batch = cursor.next_batch().unwrap().unwrap();
    assert_eq!(batch.len(), 1);
    assert!(batch.key_dimensions().is_empty());
    assert!(batch.value_fields().is_empty());
    assert!(cursor.next_batch().unwrap().is_none());
    assert_eq!(
        cursor.statistics(),
        ScanStatistics {
            decoded_rows: 2,
            returned_rows: 1
        }
    );
    // A fresh range has a fresh accepted limit and keeps the original start.
    let mut cursor = reader
        .scan_range(
            &symbol,
            &point,
            3,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert_eq!(
        cursor.next_batch().unwrap().unwrap().key_column(0).unwrap()[0].get(),
        2
    );
    assert!(cursor.next_batch().unwrap().is_none());
    assert_eq!(cursor.checkpoint().unwrap().row_ordinal(), 2);
}

#[test]
fn range_bounds_source_alias_and_selection_identity_are_checked() {
    let path = "tests/fixtures/v7/semantics-0.gdx";
    let reader = Reader::open(path, OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut seed = reader
        .scan(
            &symbol,
            ScanOptions {
                limit: Some(2),
                ..reader.scan_options()
            },
        )
        .unwrap();
    seed.next_batch().unwrap().unwrap();
    let point = seed.checkpoint().unwrap();
    for end in [0, 1, 7, u64::MAX] {
        assert_eq!(
            reader
                .scan_range(&symbol, &point, end, reader.scan_options())
                .err()
                .unwrap()
                .kind,
            ErrorKind::InvalidOptions
        );
    }
    let other = Reader::open(path, OpenOptions::default()).unwrap();
    let handle = other.symbol(symbol.id()).unwrap();
    assert_eq!(
        other
            .scan_range(&handle, &point, 3, other.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let alias = reader.symbol(SymbolId::new(2).unwrap()).unwrap();
    assert_eq!(
        reader
            .scan_range(&alias, &point, 3, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let selection = other.selection(&handle, &[]).unwrap();
    assert_eq!(
        reader
            .scan_selected_range(&symbol, &selection, &point, 3, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let mut good = reader
        .scan_range(&symbol, &point, 3, reader.scan_options())
        .unwrap();
    assert_eq!(good.next_batch().unwrap().unwrap().len(), 1);

    assert_eq!(
        reader
            .scan_range(
                &symbol,
                &point,
                2,
                ScanOptions {
                    key_projection: KeyProjection::Mask(2),
                    ..reader.scan_options()
                }
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidProjection
    );
    assert_eq!(
        reader
            .scan_range(
                &symbol,
                &point,
                2,
                ScanOptions {
                    batch_bytes: 0,
                    ..reader.scan_options()
                }
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
}

#[cfg(feature = "index")]
#[test]
fn range_owns_verified_persistent_restart_after_index_and_point_drop() {
    let path = if cfg!(feature = "compression") {
        "tests/fixtures/v7/blocks.gdx"
    } else {
        "tests/fixtures/v7/key-widths.gdx"
    };
    let reader = Reader::open(
        path,
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let file = common::TempFile::new(&[]);
    std::fs::remove_file(&file.path).unwrap();
    let options = IndexOptions {
        record_stride: if cfg!(feature = "compression") {
            257
        } else {
            3
        },
        decoded_byte_stride: 1024,
        ..IndexOptions::default()
    };
    reader
        .index_builder(&symbol, &file.path, options)
        .unwrap()
        .finish()
        .unwrap();
    let mut index = reader
        .open_index(&symbol, &file.path, options, IndexPolicy::UseIfValid)
        .unwrap()
        .index
        .unwrap();
    let validation = index.validation_statistics();
    let interval = index.interval(index.interval_count() / 2).unwrap();
    assert!(interval.start > 0);
    let (start, end) = (interval.start as usize, interval.end as usize);
    let mut cursor = reader
        .scan_range(
            &symbol,
            &interval.checkpoint,
            interval.end,
            ScanOptions {
                batch_bytes: 240,
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert_eq!(
        index.validation_statistics().source_bytes_hashed,
        validation.source_bytes_hashed
    );
    drop(interval);
    drop(index); // Neither is borrowed or reopened by the cursor.
    std::fs::write(&file.path, b"sidecar no longer needed by this cursor").unwrap();
    let expectation =
        std::fs::read_to_string(std::path::Path::new(path).with_extension("expected.tsv")).unwrap();
    let expected: Vec<_> = expectation
        .lines()
        .filter(|s| s.starts_with("R\t1\t"))
        .collect();
    let mut at = start;
    while let Some(batch) = cursor.next_batch().unwrap() {
        let actual = rows(&batch, 1);
        assert_eq!(actual, expected[at..at + actual.len()]);
        at += actual.len();
    }
    assert_eq!(at, end);
    assert_eq!(
        cursor.statistics(),
        ScanStatistics {
            decoded_rows: (end - start) as u64,
            returned_rows: (end - start) as u64
        }
    );
    assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
}
