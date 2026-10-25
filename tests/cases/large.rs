use gdxrs::*;
fn reader() -> Reader {
    Reader::open(
        "tests/fixtures/v7/case-distinct-labels.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
fn limits() -> SpillOptions {
    let mut o = SpillOptions::new(std::env::temp_dir());
    o.run_bytes = 528;
    o.merge_fan_in = 2;
    o.maximum_open_files = 9;
    o
}
fn requests(reader: &Reader, o: &SpillOptions) -> LargeLabelCursor {
    let mut b = reader.large_label_lookup(o).unwrap();
    for label in [b"z".as_slice(), b"a", b"A", b"missing", b"a", &[0xff, 0xfe]] {
        b.push(label).unwrap();
    }
    b.finish().unwrap()
}
#[test]
fn large_lookup_exact_bytes_ordinals_and_shared_selection_lifecycle() {
    let r = reader();
    let o = limits();
    let baseline = r.budget_statistics().current_bytes;
    let mut cursor = requests(&r, &o);
    let expected = [
        (2, b"A".as_slice(), Some(3)),
        (1, b"a", Some(2)),
        (4, b"a", Some(2)),
        (3, b"missing", None),
        (0, b"z", Some(1)),
        (5, &[0xff, 0xfe], None),
    ];
    for (ordinal, label, id) in expected {
        let e = cursor.next().unwrap().unwrap();
        assert_eq!(
            (e.ordinal, e.label, e.id.map(UelId::get)),
            (ordinal, label, id)
        );
    }
    assert!(cursor.next().unwrap().is_none());
    let stats = cursor.statistics();
    assert!(stats.0.complete);
    assert!(stats.1.unwrap().complete);
    assert!(stats.0.merge_passes >= 2);
    drop(cursor);
    assert_eq!(r.budget_statistics().current_bytes, baseline);
    let symbol = r.find_symbol(b"I").unwrap().unwrap();
    let base = r.selection(&symbol, &[]).unwrap();
    let mut disk = r
        .selection_from_large_labels(&symbol, 0, &base, requests(&r, &o), &o)
        .unwrap();
    let mut scan = r.scan_selected(&symbol, &disk, r.scan_options()).unwrap();
    assert_eq!(
        disk.close_spill().unwrap_err().kind,
        ErrorKind::InvalidOptions
    );
    let mut ids = Vec::new();
    while let Some(b) = scan.next_batch().unwrap() {
        ids.extend(b.key_column(0).unwrap().iter().map(|v| v.get()));
    }
    assert_eq!(ids, vec![1, 2, 3]);
    drop(scan);
    disk.close_spill().unwrap();
    let mut closed = r.scan_selected(&symbol, &disk, r.scan_options()).unwrap();
    assert!(closed.next_batch().unwrap().is_none());
    drop(closed);
    drop(disk);
    drop(base);
    drop(symbol);
    assert_eq!(r.budget_statistics().current_bytes, baseline);
}
#[test]
fn large_lookup_stream_exceeds_root_payload_and_remains_bounded() {
    let r = reader();
    let mut o = limits();
    o.run_bytes = 4096;
    let baseline = r.budget_statistics().current_bytes;
    let mut lookup = r.large_label_lookup(&o).unwrap();
    let label = [b'x'; 255];
    for _ in 0..5000 {
        lookup.push(&label).unwrap();
    }
    let mut c = lookup.finish().unwrap();
    let mut n = 0;
    while let Some(e) = c.next().unwrap() {
        assert_eq!(e.ordinal, n);
        assert_eq!(e.label, label);
        assert_eq!(e.id, None);
        n += 1;
    }
    assert_eq!(n, 5000);
    let stats = c.statistics();
    assert!(stats.0.merge_passes >= 8);
    assert!(stats.0.peak_open_files <= 3);
    assert!(stats.0.peak_temporary_bytes <= o.maximum_temporary_bytes / 3);
    assert!(r.budget_statistics().peak_bytes <= 1024 * 1024);
    eprintln!(
        "015 oversized request: records={n}, passes={}, peak_root={}, peak_request_temp={}",
        stats.0.merge_passes,
        r.budget_statistics().peak_bytes,
        stats.0.peak_temporary_bytes
    );
    drop(c);
    assert_eq!(r.budget_statistics().current_bytes, baseline);
}
#[test]
fn unique_domains_match_independent_frozen_records_and_limits() {
    let r = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let text = std::fs::read_to_string("tests/fixtures/v7/semantics-0.expected.tsv").unwrap();
    let mut symbols = r.symbols().unwrap();
    let mut checked = 0;
    while let Some(meta) = symbols.next().unwrap() {
        let sid = meta.id;
        let dimension = meta.dimension;
        let symbol = r.symbol(sid).unwrap();
        let base = r.selection(&symbol, &[]).unwrap();
        for limit in [0, 1, 3, 1000] {
            let dimensions: Vec<_> = (0..dimension).rev().collect();
            let mut expected = std::collections::BTreeSet::new();
            let prefix = format!("R\t{}\t", sid.get());
            for row in text.lines().filter(|s| s.starts_with(&prefix)).take(limit) {
                let fields: Vec<_> = row.split('\t').collect();
                for (d, key) in fields[2].split(',').filter(|s| !s.is_empty()).enumerate() {
                    expected.insert((d as u8, key.parse::<u32>().unwrap()));
                }
            }
            let mut c = r
                .unique_domains(
                    &symbol,
                    &base,
                    &dimensions,
                    ScanOptions {
                        limit: Some(limit as u64),
                        ..r.scan_options()
                    },
                    &limits(),
                )
                .unwrap();
            let mut actual = Vec::new();
            while let Some(e) = c.next().unwrap() {
                actual.push((e.dimension, e.id.get()));
            }
            assert_eq!(
                actual,
                expected.into_iter().collect::<Vec<_>>(),
                "symbol {} limit {limit}",
                sid.get()
            );
            assert!(c.statistics().complete);
            checked += 1;
        }
    }
    assert!(checked > 20);
    eprintln!("015 independent domain/limit comparisons: {checked}");
}
#[test]
fn lookup_validation_quota_empty_unknown_foreign_and_cancel() {
    let r = reader();
    let o = limits();
    let baseline = r.budget_statistics().current_bytes;
    let mut b = r.large_label_lookup(&o).unwrap();
    assert_eq!(
        b.push(&[0; 256]).unwrap_err().kind,
        ErrorKind::InvalidOptions
    );
    assert_eq!(b.push(b"a").unwrap_err().kind, ErrorKind::InvalidOptions);
    b.cancel().unwrap();
    drop(b);
    let mut empty = r.large_label_lookup(&o).unwrap().finish().unwrap();
    assert!(empty.next().unwrap().is_none());
    assert!(empty.statistics().1.is_none());
    drop(empty);
    let mut tiny = o.clone();
    tiny.maximum_temporary_bytes = 1;
    let mut b = r.large_label_lookup(&tiny).unwrap();
    b.push(b"a").unwrap();
    assert_eq!(
        b.finish().err().unwrap().kind,
        ErrorKind::TemporarySpaceExceeded
    );
    let other = reader();
    let s = other.find_symbol(b"I").unwrap().unwrap();
    let base = other.selection(&s, &[]).unwrap();
    assert_eq!(
        other
            .selection_from_large_labels(&s, 0, &base, requests(&r, &o), &o)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    drop(base);
    drop(s);
    drop(other);
    let s = r.find_symbol(b"I").unwrap().unwrap();
    let base = r.selection(&s, &[]).unwrap();
    let mut b = r.large_label_lookup(&o).unwrap();
    b.push(b"unknown").unwrap();
    let mut disk = r
        .selection_from_large_labels(&s, 0, &base, b.finish().unwrap(), &o)
        .unwrap();
    assert!(disk.is_empty());
    disk.close_spill().unwrap();
    assert_eq!(
        r.unique_domains(&s, &base, &[0, 0], r.scan_options(), &o)
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidSelection
    );
    drop(disk);
    drop(base);
    drop(s);
    assert_eq!(r.budget_statistics().current_bytes, baseline);
}

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn frozen_lookup_small_and_spill_agree_with_independent_ids() {
    let mut checked = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|s| s != "gdx") {
            continue;
        }
        let r = match Reader::open(&path, OpenOptions::default()) {
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
        let labels: Vec<_> = text
            .lines()
            .filter(|s| s.starts_with("U\t"))
            .map(|s| unhex(s.split('\t').nth(2).unwrap()))
            .collect();
        let chosen = [labels.len() - 1, 0, labels.len() / 2, 0];
        let mut b = r
            .large_label_lookup(&SpillOptions::new(std::env::temp_dir()))
            .unwrap();
        let request: Vec<_> = chosen
            .iter()
            .map(|&i| labels[i].as_slice())
            .chain(std::iter::once(b"missing-015".as_slice()))
            .collect();
        let small = r.lookup_labels(&request).unwrap();
        for label in &request {
            b.push(label).unwrap();
        }
        let mut c = b.finish().unwrap();
        let mut count = 0;
        while let Some(e) = c.next().unwrap() {
            let expected = if e.ordinal < 4 {
                Some(UelId::new(chosen[e.ordinal as usize] as u32 + 1).unwrap())
            } else {
                None
            };
            assert_eq!(e.id, expected);
            assert_eq!(e.id, small.get(e.ordinal as usize).unwrap().id);
            assert_eq!(e.label, request[e.ordinal as usize]);
            count += 1;
        }
        assert_eq!(count, 5);
        assert!(c.is_complete());
        checked += 1;
    }
    assert_eq!(checked, if cfg!(feature = "compression") { 10 } else { 8 });
}
#[test]
fn nonleading_membership_wide_projection_limits_and_domains() {
    let r = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let text = std::fs::read_to_string("tests/fixtures/v7/semantics-0.expected.tsv").unwrap();
    let labels: Vec<_> = text
        .lines()
        .filter(|s| s.starts_with("U\t"))
        .map(|s| unhex(s.split('\t').nth(2).unwrap()))
        .collect();
    let mut symbols = r.symbols().unwrap();
    let mut checked = 0;
    let mut widest = 0;
    while let Some(meta) = symbols.next().unwrap() {
        if meta.dimension < 2 {
            continue;
        }
        let sid = meta.id;
        let dimension = meta.dimension;
        widest = widest.max(dimension);
        let symbol = r.symbol(sid).unwrap();
        let base = r
            .selection(
                &symbol,
                &[DimensionSelection {
                    dimension: 0,
                    keys: KeySelection::Range(
                        UelRange::new(UelId::new(1).unwrap(), UelId::new(2).unwrap()).unwrap(),
                    ),
                }],
            )
            .unwrap();
        let o = limits();
        let mut b = r.large_label_lookup(&o).unwrap();
        for label in [&labels[0], &labels[2], &labels[0]] {
            b.push(label).unwrap();
        }
        b.push(b"missing").unwrap();
        let mut disk = r
            .selection_from_large_labels(&symbol, dimension - 1, &base, b.finish().unwrap(), &o)
            .unwrap();
        let memory = r
            .selection(
                &symbol,
                &[
                    DimensionSelection {
                        dimension: 0,
                        keys: KeySelection::Range(
                            UelRange::new(UelId::new(1).unwrap(), UelId::new(2).unwrap()).unwrap(),
                        ),
                    },
                    DimensionSelection {
                        dimension: dimension - 1,
                        keys: KeySelection::AnyOf(&[
                            UelId::new(1).unwrap(),
                            UelId::new(3).unwrap(),
                        ]),
                    },
                ],
            )
            .unwrap();
        for limit in [0, 1, 3, 1000] {
            let prefix = format!("R\t{}\t", sid.get());
            let expected: Vec<_> = text
                .lines()
                .filter(|s| s.starts_with(&prefix))
                .filter(|s| {
                    let keys = s.split('\t').nth(2).unwrap();
                    let last = keys.split(',').next_back().unwrap().parse::<u32>().unwrap();
                    (last == 1 || last == 3)
                        && keys.split(',').next().unwrap().parse::<u32>().unwrap() <= 2
                })
                .take(limit)
                .collect();
            let mut results = Vec::new();
            for selection in [&disk, &memory] {
                let mut scan = r
                    .scan_selected(
                        &symbol,
                        selection,
                        ScanOptions {
                            limit: Some(limit as u64),
                            batch_bytes: 512,
                            ..r.scan_options()
                        },
                    )
                    .unwrap();
                let mut actual = Vec::new();
                while let Some(batch) = scan.next_batch().unwrap() {
                    for i in 0..batch.len() {
                        let row = batch.row(i).unwrap();
                        let keys = (0..dimension)
                            .map(|d| row.key(d).unwrap().get().to_string())
                            .collect::<Vec<_>>()
                            .join(",");
                        let values = batch
                            .value_fields()
                            .iter()
                            .map(|&f| super::common::value(row.value(f).unwrap()))
                            .collect::<Vec<_>>()
                            .join(",");
                        actual.push(format!("R\t{}\t{keys}\t{values}", sid.get()));
                    }
                }
                assert_eq!(actual, expected);
                results.push(actual);
            }
            assert_eq!(results[0], results[1]);
            let dims = [dimension - 1, 0];
            let mut domains = r
                .unique_domains(
                    &symbol,
                    &disk,
                    &dims,
                    ScanOptions {
                        limit: Some(limit as u64),
                        batch_bytes: 512,
                        ..r.scan_options()
                    },
                    &o,
                )
                .unwrap();
            let mut expected_domains = std::collections::BTreeSet::new();
            for line in &expected {
                let keys: Vec<u32> = line
                    .split('\t')
                    .nth(2)
                    .unwrap()
                    .split(',')
                    .map(|s| s.parse().unwrap())
                    .collect();
                for d in dims {
                    expected_domains.insert((d, keys[d as usize]));
                }
            }
            let mut actual = Vec::new();
            while let Some(e) = domains.next().unwrap() {
                actual.push((e.dimension, e.id.get()));
            }
            assert_eq!(actual, expected_domains.into_iter().collect::<Vec<_>>());
            checked += 1;
        }
        disk.close_spill().unwrap();
    }
    assert_eq!(widest, 20);
    assert!(checked >= 8);
    eprintln!("015 non-leading full-row and domain comparisons: {checked}, widest={widest}");
}

#[test]
fn partially_consumed_lookup_is_rejected_and_empty_domain_projection_reads_no_records() {
    let r = reader();
    let o = limits();
    let s = r.find_symbol(b"I").unwrap().unwrap();
    let base = r.selection(&s, &[]).unwrap();
    let mut c = requests(&r, &o);
    assert!(c.next().unwrap().is_some());
    assert_eq!(
        r.selection_from_large_labels(&s, 0, &base, c, &o)
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
    let mut c = r
        .unique_domains(&s, &base, &[], r.scan_options(), &o)
        .unwrap();
    let before = r.io_statistics();
    assert!(c.next().unwrap().is_none());
    assert_eq!(r.io_statistics().read_calls, before.read_calls);
    assert_eq!(r.io_statistics().bytes_read, before.bytes_read);
    let mut c = r
        .unique_domains(
            &s,
            &base,
            &[0],
            ScanOptions {
                limit: Some(0),
                ..r.scan_options()
            },
            &o,
        )
        .unwrap();
    let before = r.io_statistics();
    assert!(c.next().unwrap().is_none());
    assert_eq!(r.io_statistics().read_calls, before.read_calls);
    assert_eq!(r.io_statistics().bytes_read, before.bytes_read);
    let mut wrong = o.clone();
    wrong.maximum_temporary_bytes -= 3;
    assert_eq!(
        r.selection_from_large_labels(&s, 0, &base, requests(&r, &o), &wrong)
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
}
