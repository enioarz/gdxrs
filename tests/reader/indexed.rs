use super::*;
fn matches(keys: &[u32], clauses: &[DimensionSelection<'_>]) -> bool {
    clauses.iter().all(|c| {
        let key = keys[usize::from(c.dimension)];
        match c.keys {
            KeySelection::All => true,
            KeySelection::One(id) => key == id.get(),
            KeySelection::AnyOf(ids) => ids.iter().any(|id| key == id.get()),
            KeySelection::Range(r) => (r.first().get()..=r.last().get()).contains(&key),
            KeySelection::Ranges(ranges) => ranges
                .iter()
                .any(|r| (r.first().get()..=r.last().get()).contains(&key)),
        }
    })
}
fn collect(scan: &mut ScanCursor) -> Vec<String> {
    let mut rows = Vec::new();
    while let Some(batch) = scan.next_batch().unwrap() {
        for n in 0..batch.len() {
            let row = batch.row(n).unwrap();
            let keys = batch
                .key_dimensions()
                .iter()
                .map(|d| row.key(*d).unwrap().get().to_string())
                .collect::<Vec<_>>()
                .join(",");
            let values = batch
                .value_fields()
                .iter()
                .map(|f| common::value(row.value(*f).unwrap()))
                .collect::<Vec<_>>()
                .join(",");
            rows.push(format!("{keys}\t{values}"));
        }
    }
    rows
}
#[test]
fn indexed_native_fixtures_match_independent_slices_and_serial_selection() {
    let mut fixtures = 0;
    let mut queries = 0;
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
            let dimension = symbol.metadata().unwrap().dimension;
            let prefix = format!("R\t{id}\t");
            let expected: Vec<_> = text
                .lines()
                .filter_map(|l| l.strip_prefix(&prefix))
                .collect();
            let file = common::TempFile::new(&[]);
            std::fs::remove_file(&file.path).unwrap();
            let options = IndexOptions {
                record_stride: 257,
                decoded_byte_stride: 1024,
                ..IndexOptions::default()
            };
            reader
                .index_builder(&symbol, &file.path, options)
                .unwrap()
                .finish()
                .unwrap();
            let mut open = reader
                .open_index(&symbol, &file.path, options, IndexPolicy::UseIfValid)
                .unwrap();
            let first = UelId::new(1).unwrap();
            let late = expected
                .last()
                .and_then(|r| r.split('\t').next())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse::<u32>().ok())
                .map(|v| UelId::new(v).unwrap())
                .unwrap_or(first);
            let ids = [late, first, late];
            let ranges = [
                UelRange::new(first, first).unwrap(),
                UelRange::new(late, late).unwrap(),
            ];
            for case in 0..9 {
                if dimension == 0 && case > 1 {
                    continue;
                }
                let axis = if case == 4 { dimension - 1 } else { 0 };
                let late_axis = if case == 4 {
                    expected
                        .last()
                        .and_then(|r| r.split('\t').next())
                        .and_then(|s| s.split(',').next_back())
                        .and_then(|s| s.parse::<u32>().ok())
                        .map(|v| UelId::new(v).unwrap())
                        .unwrap_or(first)
                } else {
                    late
                };
                let clauses = match case {
                    0 | 1 => vec![],
                    2 => vec![DimensionSelection {
                        dimension: axis,
                        keys: KeySelection::AnyOf(&[]),
                    }],
                    3 | 4 => vec![DimensionSelection {
                        dimension: axis,
                        keys: KeySelection::One(late_axis),
                    }],
                    5 => vec![DimensionSelection {
                        dimension: axis,
                        keys: KeySelection::AnyOf(&ids),
                    }],
                    6 => vec![DimensionSelection {
                        dimension: axis,
                        keys: KeySelection::Range(UelRange::new(first, late).unwrap()),
                    }],
                    _ => vec![DimensionSelection {
                        dimension: axis,
                        keys: KeySelection::Ranges(&ranges),
                    }],
                };
                let limit = match case {
                    0 => Some(0),
                    1 | 5 => Some(1),
                    6 => Some(3),
                    _ => None,
                };
                let projected = case == 8;
                let scan_options = ScanOptions {
                    batch_bytes: 4096,
                    limit,
                    key_projection: if projected {
                        KeyProjection::Mask(0)
                    } else {
                        KeyProjection::All
                    },
                    value_projection: if projected {
                        ValueProjection::Mask(0)
                    } else {
                        ValueProjection::All
                    },
                };
                let selection = reader.selection(&symbol, &clauses).unwrap();
                let want: Vec<_> = expected
                    .iter()
                    .filter(|r| {
                        let keys: Vec<u32> = r
                            .split('\t')
                            .next()
                            .unwrap()
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.parse().unwrap())
                            .collect();
                        matches(&keys, &clauses)
                    })
                    .take(limit.unwrap_or(u64::MAX) as usize)
                    .map(|r| {
                        if projected {
                            "\t".to_owned()
                        } else {
                            r.to_string()
                        }
                    })
                    .collect();
                let mut scan = reader
                    .scan_selected_indexed(&symbol, &selection, scan_options.clone(), open)
                    .unwrap();
                let got = collect(&mut scan);
                assert_eq!(got, want, "{} symbol {id} case {case}", path.display());
                let statistics = scan.indexed_statistics().unwrap();
                assert_eq!(scan.statistics().returned_rows, got.len() as u64);
                if case == 0 || case == 2 {
                    assert_eq!(statistics.intervals_considered, 0);
                    assert_eq!(scan.statistics().decoded_rows, 0);
                }
                let index = scan.into_index().unwrap();
                open = IndexOpen {
                    status: IndexStatus::Ready,
                    index: Some(index),
                };
                let mut serial = reader
                    .scan_selected(&symbol, &selection, scan_options)
                    .unwrap();
                assert_eq!(got, collect(&mut serial));
                queries += 1;
            }
        }
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
    eprintln!("M5 indexed: {fixtures} native fixtures, {queries} exact queries");
}

#[test]
fn indexed_twenty_axis_single_group_crosses_buffers_and_honors_small_batch_cap() {
    // A fixed supplemental mathematical recipe, not a Rust-to-Rust substitute
    // for the independent native corpus above. Only 4096 fixture IDs are kept.
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if !cfg!(feature = "compression") && compression == WriteCompression::Zlib {
            continue;
        }
        let file = common::TempFile::new(&[]);
        std::fs::remove_file(&file.path).unwrap();
        let mut writer = Writer::create(
            &file.path,
            WriteOptions {
                memory_budget_bytes: 4 * 1024 * 1024,
                compression,
                ..WriteOptions::default()
            },
        )
        .unwrap();
        let mut metadata = MetadataOptions::new(file.path.parent().unwrap());
        metadata.merge_fan_in = 2;
        writer.begin_metadata(&metadata).unwrap();
        let mut ids = Vec::with_capacity(4096);
        writer
            .register_uels_fresh((1..=4096).map(|n| Ok(format!("key-{n}"))), |_, id| {
                ids.push(id);
                Ok(())
            })
            .unwrap();
        let symbol = writer
            .register_symbol(&WriteSymbol::new(b"long", SymbolType::Parameter, 20))
            .unwrap();
        let mut rows = writer.begin_symbol(symbol, None).unwrap();
        for (n, &id) in ids.iter().enumerate() {
            let mut keys = [ids[0]; 20];
            keys[19] = id;
            rows.write(&keys, &[Value::NumberBits((n as f64 + 0.25).to_bits())])
                .unwrap();
        }
        rows.finish().unwrap();
        writer.finish().unwrap();
        drop(writer);
        let reader = Reader::open(
            &file.path,
            OpenOptions {
                memory_budget_bytes: 1024 * 1024,
                maximum_batch_bytes: 64,
                metadata_cache_bytes: 0,
            },
        )
        .unwrap();
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let index_file = common::TempFile::new(&[]);
        std::fs::remove_file(&index_file.path).unwrap();
        let options = IndexOptions {
            record_stride: 37,
            decoded_byte_stride: 512,
            ..IndexOptions::default()
        };
        reader
            .index_builder(&symbol, &index_file.path, options)
            .unwrap()
            .finish()
            .unwrap();
        let open = reader
            .open_index(&symbol, &index_file.path, options, IndexPolicy::UseIfValid)
            .unwrap();
        let ranges = [
            UelRange::new(UelId::new(3000).unwrap(), UelId::new(3010).unwrap()).unwrap(),
            UelRange::new(UelId::new(4090).unwrap(), UelId::new(4096).unwrap()).unwrap(),
        ];
        let selection = reader
            .selection(
                &symbol,
                &[
                    DimensionSelection {
                        dimension: 0,
                        keys: KeySelection::One(UelId::new(1).unwrap()),
                    },
                    DimensionSelection {
                        dimension: 19,
                        keys: KeySelection::Ranges(&ranges),
                    },
                ],
            )
            .unwrap();
        let options = ScanOptions {
            key_projection: KeyProjection::Mask(1 << 19),
            value_projection: ValueProjection::Mask(1),
            limit: Some(14),
            ..reader.scan_options()
        };
        let mut scan = reader
            .scan_selected_indexed(&symbol, &selection, options.clone(), open)
            .unwrap();
        let got = collect(&mut scan);
        let want: Vec<_> = (3000..=3010)
            .chain(4090..=4092)
            .map(|id| {
                format!(
                    "{id}\t{}",
                    common::value(Value::NumberBits((id as f64 - 1.0 + 0.25).to_bits()))
                )
            })
            .collect();
        assert_eq!(got, want);
        let stats = scan.indexed_statistics().unwrap();
        assert!(stats.intervals_pruned > 20);
        assert!(scan.statistics().decoded_rows < 150);
        assert_eq!(stats.checkpoint_restarts, 2);
        assert!(stats.source_io.decoded_buffers > 0);
        let mut serial = reader.scan_selected(&symbol, &selection, options).unwrap();
        assert_eq!(got, collect(&mut serial));
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
    }
}
#[test]
fn indexed_disk_membership_remains_exact_and_closure_respects_cursor_ownership() {
    let reader = Reader::open(
        "tests/fixtures/v7/case-distinct-labels.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let symbol = reader.find_symbol(b"I").unwrap().unwrap();
    let file = common::TempFile::new(&[]);
    std::fs::remove_file(&file.path).unwrap();
    let index_options = IndexOptions {
        record_stride: 1,
        ..IndexOptions::default()
    };
    reader
        .index_builder(&symbol, &file.path, index_options)
        .unwrap()
        .finish()
        .unwrap();
    let open = reader
        .open_index(&symbol, &file.path, index_options, IndexPolicy::UseIfValid)
        .unwrap();
    let mut options = SpillOptions::new(file.path.parent().unwrap());
    options.run_bytes = 528;
    options.merge_fan_in = 2;
    let mut lookup = reader.large_label_lookup(&options).unwrap();
    for label in [b"A".as_slice(), b"a", b"missing"] {
        lookup.push(label).unwrap();
    }
    let matches = lookup.finish().unwrap();
    let base = reader.selection(&symbol, &[]).unwrap();
    let mut selection = reader
        .selection_from_large_labels(&symbol, 0, &base, matches, &options)
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(
        selection.close_spill().unwrap_err().kind,
        ErrorKind::InvalidOptions
    );
    let got = collect(&mut scan);
    assert_eq!(
        scan.indexed_statistics().unwrap().mode,
        IndexScanMode::Sequential
    );
    drop(scan);
    let mut serial = reader
        .scan_selected(&symbol, &selection, reader.scan_options())
        .unwrap();
    assert_eq!(got, collect(&mut serial));
    drop(serial);
    selection.close_spill().unwrap();
}
