use gdxrs::*;
fn id(n: u32) -> UelId {
    UelId::new(n).unwrap()
}
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn options(cache: usize) -> OpenOptions {
    OpenOptions {
        memory_budget_bytes: 1024 * 1024,
        metadata_cache_bytes: cache,
        ..OpenOptions::default()
    }
}

#[test]
fn frozen_label_bytes_resolve_once_and_page_in_original_id_order() {
    let mut fixtures = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|v| v != "gdx") {
            continue;
        }
        let reader = match Reader::open(&path, options(90_000)) {
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
            .map(|s| hex(s.split('\t').nth(2).unwrap()))
            .collect();
        let last = labels.len() - 1;
        let requested = [
            labels[last].as_slice(),
            labels[0].as_slice(),
            b"unknown-label-013",
            labels[last].as_slice(),
        ];
        let baseline = reader.budget_statistics().current_bytes;
        let matches = reader.lookup_labels(&requested).unwrap();
        assert_eq!(matches.len(), 4);
        assert!(!matches.is_empty());
        for (ordinal, expected) in [
            Some(id(last as u32 + 1)),
            Some(id(1)),
            None,
            Some(id(last as u32 + 1)),
        ]
        .into_iter()
        .enumerate()
        {
            let result = matches.get(ordinal).unwrap();
            assert_eq!(result.ordinal, ordinal);
            assert_eq!(result.id, expected);
            assert_eq!(result.label, requested[ordinal]);
        }
        assert!(matches.get(4).is_none());
        assert_eq!(
            matches.statistics(),
            LabelLookupStatistics {
                table_traversals: 1,
                uel_entries_decoded: labels.len() as u64
            }
        );
        drop(matches);
        let mut cursor = reader.uel_labels().unwrap();
        for (n, expected) in labels.iter().enumerate() {
            assert_eq!(
                cursor.get(id(n as u32 + 1)).unwrap(),
                expected,
                "{} ID {}",
                path.display(),
                n + 1
            );
        }
        let bytes_read = reader.io_statistics().bytes_read;
        assert_eq!(
            cursor.get(id(labels.len() as u32)).unwrap(),
            labels.last().unwrap()
        );
        assert_eq!(reader.io_statistics().bytes_read, bytes_read);
        assert_eq!(cursor.statistics().uel_entries_decoded, labels.len() as u64);
        assert!(cursor.statistics().cache_hits > 0);
        assert!(reader.metadata_cache_statistics().current_bytes <= 90_000);
        assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
        drop(cursor);
        assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
        fixtures += 1;
    }
    assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
}

#[test]
fn exact_case_non_utf8_duplicates_unknowns_and_label_selection() {
    let reader = Reader::open("tests/fixtures/v7/case-distinct-labels.gdx", options(0)).unwrap();
    let requests: [&[u8]; 7] = [b"a", b"A", b"\xff", b"a", b"missing", b"", b"z"];
    let results = reader.lookup_labels(&requests).unwrap();
    for (n, expected) in [Some(2), Some(3), Some(4), Some(2), None, None, Some(1)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(results.get(n).unwrap().id.map(UelId::get), expected);
    }
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let selection = reader.selection_from_labels(&symbol, 0, &results).unwrap();
    let mut cursor = reader
        .scan_selected(&symbol, &selection, reader.scan_options())
        .unwrap();
    let batch = cursor.next_batch().unwrap().unwrap();
    assert_eq!(
        batch
            .key_column(0)
            .unwrap()
            .iter()
            .map(|v| v.get())
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let other = Reader::open("tests/fixtures/v7/case-distinct-labels.gdx", options(0)).unwrap();
    let foreign = other.symbol(SymbolId::UNIVERSE).unwrap();
    assert_eq!(
        other
            .selection_from_labels(&foreign, 0, &results)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        reader
            .selection_from_labels(&foreign, 0, &results)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        reader
            .selection_from_labels(&symbol, 1, &results)
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidSelection
    );
    assert_eq!(
        reader
            .lookup_labels(&[&[b'x'; 255]])
            .unwrap()
            .get(0)
            .unwrap()
            .id,
        None
    );
    let baseline = reader.budget_statistics().current_bytes;
    assert_eq!(
        reader.lookup_labels(&[&[b'x'; 256]]).err().unwrap().kind,
        ErrorKind::InvalidOptions
    );
    let too_many = vec![b"x".as_slice(); SMALL_LABEL_LOOKUP_BYTES];
    let bytes = reader.io_statistics().bytes_read;
    assert_eq!(
        reader.lookup_labels(&too_many).err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    assert_eq!(reader.io_statistics().bytes_read, bytes);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn empty_unknown_requests_have_zero_record_reads_and_owned_results() {
    let reader = Reader::open("tests/fixtures/invalid/bad-value-tag.gdx", options(0)).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut owned = b"does-not-exist".to_vec();
    let matches = reader.lookup_labels(&[&owned]).unwrap();
    owned.fill(b'x');
    assert_eq!(matches.get(0).unwrap().label, b"does-not-exist");
    assert_eq!(matches.get(0).unwrap().id, None);
    let before = reader.io_statistics().bytes_read;
    let empty = reader.lookup_labels(&[]).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.statistics(), LabelLookupStatistics::default());
    assert_eq!(reader.io_statistics().bytes_read, before);
    for result in [&empty, &matches] {
        let selection = reader.selection_from_labels(&symbol, 0, result).unwrap();
        assert!(selection.is_empty());
        let mut cursor = reader
            .scan_selected(&symbol, &selection, reader.scan_options())
            .unwrap();
        let bytes = reader.io_statistics().bytes_read;
        assert!(cursor.next_batch().unwrap().is_none());
        assert_eq!(reader.io_statistics().bytes_read, bytes);
        assert_eq!(cursor.statistics(), ScanStatistics::default());
    }
}

#[test]
fn tiny_zero_caches_and_lru_eviction_have_bounded_restart_work() {
    for cap in [0, 1, 40_000, 90_000] {
        let reader = Reader::open("tests/fixtures/v7/key-widths.gdx", options(cap)).unwrap();
        let baseline = reader.budget_statistics().current_bytes;
        let mut cursor = reader.uel_labels().unwrap();
        for n in [1, 129, 1, 257, 129] {
            assert_eq!(cursor.get(id(n)).unwrap(), format!("u{n}").as_bytes());
        }
        let stats = cursor.statistics();
        if cap < 32_768 {
            assert_eq!(stats.cache_hits, 0);
            assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
        } else {
            assert!(stats.evictions > 0);
            assert!(stats.checkpoint_restarts > 0);
        }
        if cap == 90_000 {
            assert_eq!(stats.cache_hits, 1);
        }
        assert!(reader.metadata_cache_statistics().peak_bytes <= cap);
        drop(cursor);
        assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
}

#[test]
fn simultaneous_cursors_share_cache_cap_and_detect_buffered_truncation() {
    let bytes = std::fs::read("tests/fixtures/v7/key-widths.gdx").unwrap();
    let copy = super::common::TempFile::new(&bytes);
    let reader = Reader::open(&copy.path, options(40_000)).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let mut a = reader.uel_labels().unwrap();
    let mut b = reader.clone().uel_labels().unwrap();
    assert_eq!(a.get(id(1)).unwrap(), b"u1");
    let bytes = reader.metadata_cache_statistics().current_bytes;
    assert!(bytes > 0);
    assert_eq!(b.get(id(129)).unwrap(), b"u129");
    assert_eq!(reader.metadata_cache_statistics().current_bytes, bytes);
    assert_eq!(b.get(id(129)).unwrap(), b"u129");
    assert_eq!(b.statistics().cache_hits, 0);
    drop(a);
    assert_eq!(b.get(id(129)).unwrap(), b"u129");
    assert_eq!(b.get(id(129)).unwrap(), b"u129");
    assert_eq!(b.statistics().cache_hits, 1);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&copy.path)
        .unwrap()
        .set_len(0)
        .unwrap();
    assert_eq!(b.get(id(129)).err().unwrap().kind, ErrorKind::SourceChanged);
    assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
    assert_eq!(b.get(id(129)).err().unwrap().kind, ErrorKind::SourceChanged);
    drop(b);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

// Runtime-only bounded metadata probe: replace one compressed UEL section with
// independent stored blocks, preserving all other bytes and adjusting pointers.
// This is development fixture construction, not a production GDX writer.
#[cfg(feature = "compression")]
fn crossing_metadata() -> Vec<u8> {
    let mut bytes = std::fs::read("tests/fixtures/v7/semantics-1.gdx").unwrap();
    let pointers: [usize; 6] = std::array::from_fn(|n| {
        u64::from_le_bytes(bytes[118 + n * 8..126 + n * 8].try_into().unwrap()) as usize
    });
    let first = pointers[1];
    let end = pointers[3];
    let mut decoded = vec![5];
    decoded.extend_from_slice(b"_UEL_");
    decoded.extend_from_slice(&8192i32.to_le_bytes());
    for n in 1..=8192 {
        let label = format!("k{n:05}");
        decoded.push(label.len() as u8);
        decoded.extend_from_slice(label.as_bytes());
    }
    decoded.push(5);
    decoded.extend_from_slice(b"_UEL_");
    let mut encoded = Vec::new();
    for block in decoded.chunks(32768) {
        encoded.push(0);
        encoded.extend_from_slice(&(block.len() as u16).to_be_bytes());
        encoded.extend_from_slice(block);
    }
    let delta = encoded.len() as i64 - (end - first) as i64;
    for n in [3, 5] {
        let offset = pointers[n];
        if offset != 0 {
            bytes[118 + n * 8..126 + n * 8].copy_from_slice(&(offset as i64 + delta).to_le_bytes());
        }
    }
    bytes.splice(first..end, encoded);
    bytes
}
#[cfg(feature = "compression")]
#[test]
fn stored_compressed_metadata_string_crossing_and_resumed_page() {
    let bytes = crossing_metadata();
    let copy = super::common::TempFile::new(&bytes);
    let reader = Reader::open(&copy.path, options(90_000)).unwrap();
    let labels = reader
        .lookup_labels(&[b"k04680", b"k08192", b"k04680", b"missing"])
        .unwrap();
    assert_eq!(labels.statistics().uel_entries_decoded, 8192);
    assert_eq!(labels.get(0).unwrap().id, Some(id(4680)));
    assert_eq!(labels.get(1).unwrap().id, Some(id(8192)));
    let mut cursor = reader.uel_labels().unwrap();
    for n in [4481, 4609, 4680, 4737, 8192] {
        assert_eq!(cursor.get(id(n)).unwrap(), format!("k{n:05}").as_bytes());
    }
    assert!(cursor.statistics().checkpoint_restarts >= 2);
}
