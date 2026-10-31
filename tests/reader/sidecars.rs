use super::*;
#[test]
fn source_verified_sidecars_match_native_frozen_streams_and_exact_interval_summaries() {
    let mut checked = 0;
    let mut intervals = 0;
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
        let expectation = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        for id in 0..=reader.information().unwrap().symbol_count {
            let prefix = format!("R\t{id}\t");
            let expected: Vec<_> = expectation
                .lines()
                .filter(|l| l.starts_with(&prefix))
                .collect();
            let symbol = reader.symbol(SymbolId::new(id).unwrap()).unwrap();
            let file = common::TempFile::new(&[]);
            std::fs::remove_file(&file.path).unwrap();
            let options = IndexOptions {
                record_stride: 257,
                decoded_byte_stride: 1024,
                ..IndexOptions::default()
            };
            let mut builder = reader.index_builder(&symbol, &file.path, options).unwrap();
            let stats = builder.finish().unwrap();
            drop(builder);
            assert_eq!(stats.rows_decoded, expected.len() as u64);
            let mut index = reader
                .open_index(&symbol, &file.path, options, IndexPolicy::UseIfValid)
                .unwrap()
                .index
                .unwrap();
            let mut at = 0usize;
            for i in 0..index.interval_count() {
                let interval = index.interval(i).unwrap();
                assert_eq!(interval.start, at as u64);
                assert!(interval.end >= interval.start);
                let mut scan = reader
                    .scan_from_checkpoint(
                        &symbol,
                        &interval.checkpoint,
                        ScanOptions {
                            limit: Some(interval.end - interval.start),
                            ..reader.scan_options()
                        },
                    )
                    .unwrap();
                while let Some(batch) = scan.next_batch().unwrap() {
                    for n in 0..batch.len() {
                        let row = batch.row(n).unwrap();
                        let mut actual = format!("R\t{id}\t");
                        for (axis, d) in batch.key_dimensions().iter().enumerate() {
                            use std::fmt::Write;
                            write!(
                                actual,
                                "{}{}",
                                if axis == 0 { "" } else { "," },
                                row.key(*d).unwrap().get()
                            )
                            .unwrap();
                            let (low, high) = interval.key_bounds(*d).unwrap();
                            assert!((low.get()..=high.get()).contains(&row.key(*d).unwrap().get()));
                        }
                        actual.push('\t');
                        for (f, field) in batch.value_fields().iter().enumerate() {
                            if f != 0 {
                                actual.push(',');
                            }
                            actual.push_str(&common::value(row.value(*field).unwrap()));
                        }
                        assert_eq!(
                            actual,
                            expected[at],
                            "{} symbol {id} row {at}",
                            path.display()
                        );
                        at += 1;
                    }
                }
                assert_eq!(at as u64, interval.end);
                intervals += 1;
            }
            assert_eq!(at, expected.len());
        }
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
        checked += 1;
    }
    assert_eq!(checked, if cfg!(feature = "compression") { 10 } else { 8 });
    eprintln!("M5 sidecars: {checked} native fixtures, {intervals} exact intervals");
}
