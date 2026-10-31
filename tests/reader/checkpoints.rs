use super::*;
use std::fmt::Write;

fn rows(batch: &BatchView<'_>, id: u32) -> Vec<String> {
    (0..batch.len())
        .map(|n| {
            let row = batch.row(n).unwrap();
            let mut out = format!("R\t{id}\t");
            for (i, d) in batch.key_dimensions().iter().enumerate() {
                write!(
                    out,
                    "{}{}",
                    if i == 0 { "" } else { "," },
                    row.key(*d).unwrap().get()
                )
                .unwrap();
            }
            out.push('\t');
            for (i, f) in batch.value_fields().iter().enumerate() {
                write!(
                    out,
                    "{}{}",
                    if i == 0 { "" } else { "," },
                    common::value(row.value(*f).unwrap())
                )
                .unwrap();
            }
            out
        })
        .collect()
}

#[test]
fn checkpoint_suffixes_match_frozen_native_expectations() {
    let mut fixtures = 0;
    let mut total = 0;
    let mut points = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "gdx") {
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
            Ok(reader) => reader,
            Err(e)
                if !cfg!(feature = "compression")
                    && e.kind == ErrorKind::CompressionUnavailable =>
            {
                continue;
            }
            Err(e) => panic!("{e}"),
        };
        // Only the fixed frozen small corpus is collected, never production data.
        let expectation = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        for id in 0..=reader.information().unwrap().symbol_count {
            let prefix = format!("R\t{id}\t");
            let expected: Vec<_> = expectation
                .lines()
                .filter(|l| l.starts_with(&prefix))
                .collect();
            let symbol = reader.symbol(SymbolId::new(id).unwrap()).unwrap();
            let metadata = symbol.metadata().unwrap();
            let fields = if matches!(metadata.kind, SymbolType::Variable | SymbolType::Equation) {
                5
            } else {
                1
            };
            let batch_bytes = if metadata.raw_record_count <= 64 {
                usize::from(metadata.dimension) * std::mem::size_of::<UelId>()
                    + fields * std::mem::size_of::<Value>()
            } else {
                240
            };
            let mut scan = reader
                .scan(
                    &symbol,
                    ScanOptions {
                        batch_bytes,
                        ..reader.scan_options()
                    },
                )
                .unwrap();
            let mut ordinal = 0;
            loop {
                let point = scan.checkpoint().unwrap();
                assert_eq!(point.row_ordinal(), ordinal as u64);
                let mut suffix = reader
                    .scan_from_checkpoint(
                        &symbol,
                        &point,
                        ScanOptions {
                            batch_bytes: 240,
                            limit: Some(2),
                            ..reader.scan_options()
                        },
                    )
                    .unwrap();
                let mut actual = Vec::new();
                while let Some(batch) = suffix.next_batch().unwrap() {
                    actual.extend(rows(&batch, id));
                }
                let end = (ordinal + 2).min(expected.len());
                assert_eq!(
                    actual,
                    expected[ordinal..end],
                    "{} symbol {id} point {ordinal}",
                    path.display()
                );
                assert_eq!(suffix.statistics().decoded_rows, (end - ordinal) as u64);
                assert_eq!(suffix.statistics().returned_rows, (end - ordinal) as u64);
                points += 1;
                drop(suffix);
                // Check a complete suffix too, at the beginning and around midpoint.
                if ordinal == 0
                    || ordinal < expected.len() / 2 && ordinal + 12 >= expected.len() / 2
                {
                    let mut suffix = reader
                        .scan_from_checkpoint(&symbol, &point, reader.scan_options())
                        .unwrap();
                    let mut at = ordinal;
                    while let Some(batch) = suffix.next_batch().unwrap() {
                        let actual = rows(&batch, id);
                        assert_eq!(actual, expected[at..at + actual.len()]);
                        at += actual.len();
                    }
                    assert_eq!(at, expected.len());
                }
                let Some(batch) = scan.next_batch().unwrap() else {
                    break;
                };
                let actual = rows(&batch, id);
                assert_eq!(actual, expected[ordinal..ordinal + actual.len()]);
                ordinal += actual.len();
            }
            assert_eq!(ordinal, expected.len());
            let end = scan.checkpoint().unwrap();
            let mut resumed = reader
                .scan_from_checkpoint(&symbol, &end, reader.scan_options())
                .unwrap();
            assert!(resumed.next_batch().unwrap().is_none());
            total += ordinal;
        }
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
    if cfg!(feature = "compression") {
        assert_eq!(total, 141358);
    }
    eprintln!("M5 checkpoint corpus: {fixtures} fixtures, {total} rows, {points} boundaries");
}

#[test]
fn checkpoint_identity_projection_limits_and_selection_are_explicit() {
    let path = "tests/fixtures/v7/semantics-0.gdx";
    let reader = Reader::open(path, OpenOptions::default()).unwrap();
    let set = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut scan = reader
        .scan(
            &set,
            ScanOptions {
                limit: Some(2),
                key_projection: KeyProjection::Mask(0),
                value_projection: ValueProjection::Mask(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert_eq!(scan.next_batch().unwrap().unwrap().len(), 2);
    let point = scan.checkpoint().unwrap();
    assert_eq!(point.row_ordinal(), 2);
    assert!(scan.next_batch().unwrap().is_none());
    let mut resumed = reader
        .scan_from_checkpoint(
            &set,
            &point,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let batch = resumed.next_batch().unwrap().unwrap();
    assert_eq!(batch.key_column(0).unwrap()[0].get(), 3);
    assert_eq!(batch.len(), 1);
    assert!(resumed.next_batch().unwrap().is_none());
    assert_eq!(resumed.statistics().decoded_rows, 1);
    assert_eq!(resumed.checkpoint().unwrap().row_ordinal(), 3);
    let other = Reader::open(path, OpenOptions::default()).unwrap();
    let other_set = other.symbol(SymbolId::new(1).unwrap()).unwrap();
    assert_eq!(
        other
            .scan_from_checkpoint(&other_set, &point, other.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let universe = reader.symbol(SymbolId::UNIVERSE).unwrap();
    assert_eq!(
        reader
            .scan_from_checkpoint(&universe, &point, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let selection = reader.selection(&set, &[]).unwrap();
    let mut selected = reader
        .scan_selected(&set, &selection, reader.scan_options())
        .unwrap();
    assert_eq!(
        selected.checkpoint().err().unwrap().kind,
        ErrorKind::InvalidOptions
    );
    assert!(selected.next_batch().unwrap().is_some());
    let mut zero = reader
        .scan(
            &set,
            ScanOptions {
                limit: Some(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let before = reader.io_statistics().bytes_read;
    assert!(zero.next_batch().unwrap().is_none());
    assert_eq!(reader.io_statistics().bytes_read, before);
    let initial = zero.checkpoint().unwrap();
    assert_eq!(initial.row_ordinal(), 0);
    let mut resumed = reader
        .scan_from_checkpoint(&set, &initial, reader.scan_options())
        .unwrap();
    assert_eq!(
        resumed
            .next_batch()
            .unwrap()
            .unwrap()
            .key_column(0)
            .unwrap()[0]
            .get(),
        1
    );
}

#[test]
fn checkpoint_source_changes_are_terminal_and_alias_binding_is_preserved() {
    use std::io::Write;
    let reader = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let alias = reader.symbol(SymbolId::new(2).unwrap()).unwrap();
    let set = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let point = reader
        .scan(&alias, reader.scan_options())
        .unwrap()
        .checkpoint()
        .unwrap();
    assert_eq!(
        reader
            .scan_from_checkpoint(&set, &point, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert!(
        reader
            .clone()
            .scan_from_checkpoint(&alias, &point, reader.scan_options())
            .unwrap()
            .next_batch()
            .unwrap()
            .is_some()
    );
    let mut zero = reader
        .scan_from_checkpoint(
            &alias,
            &point,
            ScanOptions {
                limit: Some(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let before = reader.io_statistics().bytes_read;
    assert!(zero.next_batch().unwrap().is_none());
    assert_eq!(reader.io_statistics().bytes_read, before);
    assert_eq!(
        zero.checkpoint().unwrap().row_ordinal(),
        point.row_ordinal()
    );
    let copy =
        common::TempFile::new(&std::fs::read("tests/fixtures/v7/optional-domains.gdx").unwrap());
    let reader = Reader::open(&copy.path, OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut scan = reader.scan(&symbol, reader.scan_options()).unwrap();
    let point = scan.checkpoint().unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&copy.path)
        .unwrap()
        .write_all(b"mutation")
        .unwrap();
    assert_eq!(
        reader
            .scan_from_checkpoint(&symbol, &point, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceChanged
    );
    assert_eq!(
        scan.checkpoint().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
}

#[test]
fn checkpoints_retain_twenty_axis_predecessors_across_delta_and_group_changes() {
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if compression == WriteCompression::Zlib && !cfg!(feature = "compression") {
            continue;
        }
        let temp = common::TempFile::new(&[]);
        std::fs::remove_file(&temp.path).unwrap();
        let mut writer = Writer::create(
            &temp.path,
            WriteOptions {
                compression,
                ..WriteOptions::default()
            },
        )
        .unwrap();
        writer
            .begin_metadata(&MetadataOptions::new(temp.path.parent().unwrap()))
            .unwrap();
        let ids: Vec<_> = (1..=100)
            .map(|n| writer.register_uel(format!("u{n}").as_bytes()).unwrap())
            .collect();
        let symbol = writer
            .register_symbol(&WriteSymbol::new(b"wide", SymbolType::Parameter, 20))
            .unwrap();
        let mut rows = writer.begin_symbol(symbol, None).unwrap();
        for n in 0..50 {
            let mut keys = [ids[0]; 20];
            keys[0] = ids[n / 10];
            keys[1..19].copy_from_slice(&ids[2..20]);
            keys[19] = ids[n % 10];
            rows.write(&keys, &[Value::NumberBits(0x3ff0000000000000 | n as u64)])
                .unwrap();
        }
        rows.finish().unwrap();
        writer.finish().unwrap();
        drop(writer);
        let reader = Reader::open(
            &temp.path,
            OpenOptions {
                memory_budget_bytes: 1024 * 1024,
                metadata_cache_bytes: 0,
                ..OpenOptions::default()
            },
        )
        .unwrap();
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let mut scan = reader
            .scan(
                &symbol,
                ScanOptions {
                    batch_bytes: 96,
                    ..reader.scan_options()
                },
            )
            .unwrap();
        for n in 0..50 {
            let point = scan.checkpoint().unwrap();
            assert_eq!(point.row_ordinal(), n as u64);
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
            let batch = suffix.next_batch().unwrap().unwrap();
            let row = batch.row(0).unwrap();
            for axis in 0..20 {
                let expected = if axis == 0 {
                    n / 10 + 1
                } else if axis == 19 {
                    n % 10 + 1
                } else {
                    axis + 2
                };
                assert_eq!(row.key(axis as u8).unwrap().get(), expected as u32);
            }
            assert_eq!(
                row.value(ValueField::Level),
                Some(Value::NumberBits(0x3ff0000000000000 | n as u64))
            );
            assert_eq!(scan.next_batch().unwrap().unwrap().len(), 1);
        }
        assert!(scan.next_batch().unwrap().is_none());
        assert_eq!(scan.checkpoint().unwrap().row_ordinal(), 50);
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
    }
}
