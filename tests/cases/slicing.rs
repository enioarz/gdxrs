use gdxrs::*;
use std::fmt::Write;

fn id(n: u32) -> UelId {
    UelId::new(n).unwrap()
}
fn clause(dimension: u8, keys: KeySelection<'_>) -> DimensionSelection<'_> {
    DimensionSelection { dimension, keys }
}
fn range(first: u32, last: u32) -> UelRange {
    UelRange::new(id(first), id(last)).unwrap()
}
fn scan_lines(
    reader: &Reader,
    symbol: &SymbolHandle,
    selection: &Selection,
    limit: Option<u64>,
) -> Vec<String> {
    let meta = symbol.metadata().unwrap();
    let fields = if matches!(meta.kind, SymbolType::Variable | SymbolType::Equation) {
        5
    } else {
        1
    };
    let bytes = usize::from(meta.dimension) * std::mem::size_of::<UelId>()
        + fields * std::mem::size_of::<Value>();
    let mut cursor = reader
        .scan_selected(
            symbol,
            selection,
            ScanOptions {
                limit,
                batch_bytes: bytes.max(1) * 2,
                ..reader.scan_options()
            },
        )
        .unwrap();
    let mut lines = Vec::new(); // Frozen corpus only; not a production collection API.
    while let Some(batch) = cursor.next_batch().unwrap() {
        assert!(!batch.is_empty());
        for i in 0..batch.len() {
            let mut line = format!("R\t{}\t", symbol.id().get());
            for (n, d) in batch.key_dimensions().iter().enumerate() {
                write!(
                    &mut line,
                    "{}{}",
                    if n == 0 { "" } else { "," },
                    batch.key_column(*d).unwrap()[i].get()
                )
                .unwrap();
            }
            line.push('\t');
            for (n, f) in batch.value_fields().iter().enumerate() {
                write!(
                    &mut line,
                    "{}{}",
                    if n == 0 { "" } else { "," },
                    super::common::value(batch.value_column(*f).unwrap()[i])
                )
                .unwrap();
            }
            lines.push(line);
        }
    }
    assert_eq!(cursor.statistics().returned_rows, lines.len() as u64);
    if limit == Some(0) {
        assert_eq!(cursor.statistics().decoded_rows, 0);
    }
    lines
}

#[test]
fn all_frozen_symbols_match_independent_expected_slices() {
    let mut fixtures = 0;
    let mut comparisons = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|v| v != "gdx") {
            continue;
        }
        let reader = match Reader::open(&path, OpenOptions::default()) {
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
        let info = reader.information().unwrap();
        for n in 0..=info.symbol_count {
            let symbol = reader.symbol(SymbolId::new(n).unwrap()).unwrap();
            let meta = symbol.metadata().unwrap();
            let prefix = format!("R\t{n}\t");
            let rows: Vec<_> = expected
                .lines()
                .filter(|line| line.starts_with(&prefix))
                .collect();
            let sets = [id(info.uel_count), id(1), id(info.uel_count)];
            let ranges = [
                range(info.uel_count, info.uel_count),
                range(1, 1),
                range(1, 1),
            ];
            let mut cases = vec![Vec::new()];
            if meta.dimension > 0 {
                cases.extend([
                    vec![clause(0, KeySelection::All)],
                    vec![clause(0, KeySelection::One(id(info.uel_count)))],
                    vec![clause(meta.dimension - 1, KeySelection::AnyOf(&sets))],
                    vec![clause(
                        0,
                        KeySelection::Range(range(1, info.uel_count.min(2))),
                    )],
                    vec![clause(0, KeySelection::Ranges(&ranges))],
                    vec![clause(0, KeySelection::AnyOf(&[]))],
                ]);
                if meta.dimension > 1 {
                    cases.push(vec![
                        clause(0, KeySelection::One(id(1))),
                        clause(
                            meta.dimension - 1,
                            KeySelection::Range(range(1, info.uel_count.min(2))),
                        ),
                    ]);
                }
            }
            for clauses in cases {
                let selection = reader.selection(&symbol, &clauses).unwrap();
                // Reference filtering uses only authored TSV tuples and public
                // request values; it never calls the production matcher/decoder.
                let accepted: Vec<_> = rows
                    .iter()
                    .filter(|line| {
                        let tuple: Vec<u32> = line
                            .split('\t')
                            .nth(2)
                            .unwrap()
                            .split(',')
                            .filter(|v| !v.is_empty())
                            .map(|s| s.parse().unwrap())
                            .collect();
                        clauses.iter().all(|c| {
                            let key = tuple[usize::from(c.dimension)];
                            match c.keys {
                                KeySelection::All => true,
                                KeySelection::One(value) => key == value.get(),
                                KeySelection::AnyOf(values) => {
                                    values.iter().any(|v| key == v.get())
                                }
                                KeySelection::Range(r) => {
                                    (r.first().get()..=r.last().get()).contains(&key)
                                }
                                KeySelection::Ranges(rs) => rs
                                    .iter()
                                    .any(|r| (r.first().get()..=r.last().get()).contains(&key)),
                            }
                        })
                    })
                    .copied()
                    .collect();
                for limit in [None, Some(0), Some(1), Some(3)] {
                    let output = scan_lines(&reader, &symbol, &selection, limit);
                    let expected: Vec<_> = accepted
                        .iter()
                        .take(limit.unwrap_or(u64::MAX) as usize)
                        .copied()
                        .collect();
                    assert_eq!(
                        output,
                        expected,
                        "{}, symbol {n}, limit {limit:?}",
                        path.display()
                    );
                    comparisons += 1;
                }
            }
        }
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
    eprintln!("M2 frozen slice comparisons={comparisons}, fixtures={fixtures}");
}

#[test]
fn nonleading_filter_precedes_projection_and_exact_limit() {
    for path in ["semantics-0", "semantics-1"] {
        let reader = match Reader::open(
            format!("tests/fixtures/v7/{path}.gdx"),
            OpenOptions::default(),
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
        let symbol = reader.symbol(SymbolId::new(7).unwrap()).unwrap();
        let selection = reader
            .selection(&symbol, &[clause(1, KeySelection::One(id(4)))])
            .unwrap();
        let mut cursor = reader
            .scan_selected(
                &symbol,
                &selection,
                ScanOptions {
                    key_projection: KeyProjection::Mask(1),
                    limit: Some(3),
                    batch_bytes: 20,
                    ..reader.scan_options()
                },
            )
            .unwrap();
        for (n, expected) in [
            Value::Special(SpecialValue::NegativeInfinity),
            Value::NumberBits((-1f64).to_bits()),
            Value::Acronym {
                id: AcronymId::new(7).unwrap(),
                encoded_bits: (7.0_f64 * 10.0e300).to_bits(),
            },
        ]
        .into_iter()
        .enumerate()
        {
            let batch = cursor.next_batch().unwrap().unwrap();
            assert_eq!(batch.len(), 1);
            assert_eq!(batch.key_dimensions(), &[0]);
            assert!(batch.key_column(1).is_none());
            assert_eq!(batch.key_column(0).unwrap()[0].get(), n as u32 + 1);
            assert_eq!(batch.value_column(ValueField::Level).unwrap()[0], expected);
        }
        assert!(cursor.next_batch().unwrap().is_none());
        assert_eq!(
            cursor.statistics(),
            ScanStatistics {
                decoded_rows: 12,
                returned_rows: 3
            }
        );
        let mut cursor = reader
            .scan_selected(
                &symbol,
                &selection,
                ScanOptions {
                    key_projection: KeyProjection::Mask(0),
                    value_projection: ValueProjection::Mask(0),
                    ..reader.scan_options()
                },
            )
            .unwrap();
        assert_eq!(cursor.next_batch().unwrap().unwrap().len(), 4);
        assert!(cursor.next_batch().unwrap().is_none());
        assert_eq!(
            cursor.statistics(),
            ScanStatistics {
                decoded_rows: 16,
                returned_rows: 4
            }
        );
    }
}

#[test]
fn selection_validation_is_complete_and_source_symbol_bound() {
    let reader = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(7).unwrap()).unwrap();
    let before = reader.budget_statistics().current_bytes;
    for (clauses, kind) in [
        (
            vec![clause(2, KeySelection::All)],
            ErrorKind::InvalidSelection,
        ),
        (
            vec![clause(0, KeySelection::All), clause(0, KeySelection::All)],
            ErrorKind::InvalidSelection,
        ),
        (
            vec![
                clause(0, KeySelection::AnyOf(&[])),
                clause(1, KeySelection::One(id(6))),
            ],
            ErrorKind::InvalidReference,
        ),
        (
            vec![clause(0, KeySelection::Range(range(1, 6)))],
            ErrorKind::InvalidReference,
        ),
    ] {
        let reads = reader.io_statistics().bytes_read;
        assert_eq!(
            reader.selection(&symbol, &clauses).err().unwrap().kind,
            kind
        );
        assert_eq!(reader.io_statistics().bytes_read, reads);
        assert_eq!(reader.budget_statistics().current_bytes, before);
    }
    assert_eq!(
        UelRange::new(id(2), id(1)).err().unwrap().kind,
        ErrorKind::InvalidSelection
    );
    let scalar = reader.symbol(SymbolId::new(5).unwrap()).unwrap();
    assert_eq!(
        reader
            .selection(&scalar, &[clause(0, KeySelection::All)])
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidSelection
    );
    let selection = reader.selection(&symbol, &[]).unwrap();
    let other = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    assert_eq!(
        reader
            .scan_selected(&other, &selection, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidSelection
    );
    let reader2 =
        Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let foreign = reader2.symbol(SymbolId::new(7).unwrap()).unwrap();
    assert_eq!(
        reader2
            .scan_selected(&foreign, &selection, reader2.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        reader
            .scan_selected(&foreign, &selection, reader.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        reader.selection(&foreign, &[]).err().unwrap().kind,
        ErrorKind::SourceMismatch
    );
}

#[test]
fn selection_payload_quota_clone_ownership_and_union_normalization() {
    let reader = Reader::open(
        "tests/fixtures/v7/key-widths.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let mut ids = [id(65536), id(1), id(1)];
    let selection = reader
        .selection(&symbol, &[clause(0, KeySelection::AnyOf(&ids))])
        .unwrap();
    assert_eq!(
        reader.budget_statistics().current_bytes,
        baseline + 16 * 1024 + 12
    );
    ids.fill(id(2)); // Owned selection must not borrow or observe later caller mutation.
    let clone = selection.clone();
    drop(selection);
    let mut scan = reader
        .scan_selected(&symbol, &clone, reader.scan_options())
        .unwrap();
    drop(clone);
    let batch = scan.next_batch().unwrap().unwrap();
    assert_eq!(
        batch
            .key_column(0)
            .unwrap()
            .iter()
            .map(|v| v.get())
            .collect::<Vec<_>>(),
        [1, 65536]
    );
    assert_eq!(
        scan.statistics(),
        ScanStatistics {
            decoded_rows: 65536,
            returned_rows: 2
        }
    );
    drop(scan);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
    let ranges = [
        range(1, 10),
        range(2, 3),
        range(10, 12),
        range(65536, 65536),
    ];
    let selection = reader
        .selection(&symbol, &[clause(0, KeySelection::Ranges(&ranges))])
        .unwrap();
    let mut scan = reader
        .scan_selected(&symbol, &selection, reader.scan_options())
        .unwrap();
    assert_eq!(scan.next_batch().unwrap().unwrap().len(), 13);
    drop(scan);
    drop(selection);
    let oversized = vec![id(1); 300_000]; // Caller-owned input, outside library budget.
    assert_eq!(
        reader
            .selection(&symbol, &[clause(0, KeySelection::AnyOf(&oversized))])
            .err()
            .unwrap()
            .kind,
        ErrorKind::BudgetTooSmall
    );
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
    assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
}

#[test]
fn explicit_empty_selected_scan_does_not_decode_corrupt_record_data() {
    let reader = Reader::open(
        "tests/fixtures/invalid/bad-value-tag.gdx",
        OpenOptions::default(),
    )
    .unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    for keys in [KeySelection::AnyOf(&[]), KeySelection::Ranges(&[])] {
        let selection = reader.selection(&symbol, &[clause(0, keys)]).unwrap();
        assert!(selection.is_empty());
        assert_eq!(
            reader
                .scan_selected(
                    &symbol,
                    &selection,
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
        let mut scan = reader
            .scan_selected(&symbol, &selection, reader.scan_options())
            .unwrap();
        let current = reader.budget_statistics().current_bytes;
        let io = reader.io_statistics().bytes_read;
        assert!(scan.next_batch().unwrap().is_none());
        assert!(scan.next_batch().unwrap().is_none());
        assert_eq!(scan.statistics(), ScanStatistics::default());
        assert_eq!(reader.budget_statistics().current_bytes, current);
        assert_eq!(reader.io_statistics().bytes_read, io);
    }
    let selection = reader
        .selection(&symbol, &[clause(0, KeySelection::One(id(1)))])
        .unwrap();
    let mut scan = reader
        .scan_selected(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(0),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let io = reader.io_statistics().bytes_read;
    assert!(scan.next_batch().unwrap().is_none());
    assert_eq!(reader.io_statistics().bytes_read, io);
    let mut scan = reader
        .scan_selected(&symbol, &selection, reader.scan_options())
        .unwrap();
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidValueTag
    );
    assert_eq!(scan.statistics(), ScanStatistics::default());
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidValueTag
    );
}
