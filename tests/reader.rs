mod common;
use gdxrs::*;

#[test]
fn complete_metadata_matches_every_authored_fixture() {
    let mut checked = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|v| v != "gdx") {
            continue;
        }
        let reader = match Reader::open(&path, OpenOptions::default()) {
            Ok(v) => v,
            Err(e)
                if !cfg!(feature = "compression")
                    && e.kind == ErrorKind::CompressionUnavailable =>
            {
                continue;
            }
            Err(e) => panic!("{}: {e}", path.display()),
        };
        let expected = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        let expected: String = expected
            .lines()
            .filter(|line| !line.starts_with("R\t") && !line.starts_with("E\t"))
            .map(|s| format!("{s}\n"))
            .collect();
        assert_eq!(
            common::metadata(&reader).unwrap(),
            expected,
            "{}",
            path.display()
        );
        let mut symbols = reader.symbols().unwrap();
        let mut count = 0;
        while let Some(symbol) = symbols.next().unwrap() {
            count += 1;
            assert_eq!(symbol.id.get(), count);
        }
        assert_eq!(count, reader.information().unwrap().symbol_count);
        checked += 1;
    }
    assert_eq!(checked, if cfg!(feature = "compression") { 10 } else { 8 });
}

#[test]
fn large_label_table_is_streamed_under_one_mib() {
    let options = OpenOptions {
        memory_budget_bytes: 1024 * 1024,
        metadata_cache_bytes: 0,
        ..OpenOptions::default()
    };
    let reader = Reader::open("tests/fixtures/v7/key-widths.gdx", options).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let mut uels = reader.uels().unwrap();
    let mut count = 0;
    while let Some(entry) = uels.next().unwrap() {
        count += 1;
        assert_eq!(entry.id.get(), count);
    }
    assert_eq!(count, 65536);
    assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
    drop(uels);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn label_bytes_handle_lifetimes_and_source_identity() {
    let reader = Reader::open(
        "tests/fixtures/v7/case-distinct-labels.gdx",
        OpenOptions::default(),
    )
    .unwrap();
    let mut labels = reader.uels().unwrap();
    assert_eq!(labels.next().unwrap().unwrap().label, b"z");
    assert_eq!(labels.next().unwrap().unwrap().label, b"a");
    assert_eq!(labels.next().unwrap().unwrap().label, b"A");
    assert!(std::str::from_utf8(labels.next().unwrap().unwrap().label).is_err());
    drop(labels);
    let handle = reader.find_symbol(b"I").unwrap().unwrap();
    assert_eq!(handle.metadata().unwrap().name, b"i");
    let other = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    assert_eq!(
        other.domains(&handle).err().unwrap().kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        other.comments(&handle).err().unwrap().kind,
        ErrorKind::SourceMismatch
    );
    drop(reader);
    assert_eq!(handle.metadata().unwrap().name, b"i");
    assert!(SymbolId::new(u32::MAX).is_err());
    assert!(UelId::new(0).is_err());
    assert!(AcronymId::new(0).is_err());
}

#[test]
fn cyclic_alias_and_outside_ids_are_errors() {
    let mut bytes = std::fs::read("tests/fixtures/v7/semantics-0.gdx").unwrap();
    let signature = [2, b'i', b'i', 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4];
    let at = bytes
        .windows(signature.len())
        .position(|v| v == signature)
        .unwrap()
        + 16;
    bytes[at..at + 4].copy_from_slice(&2i32.to_le_bytes());
    let temp = common::TempFile::new(&bytes);
    let reader = Reader::open(&temp.path, OpenOptions::default()).unwrap();
    let alias = reader.symbol(SymbolId::new(2).unwrap()).unwrap();
    assert_eq!(alias.metadata().err().unwrap().kind, ErrorKind::AliasCycle);
    assert_eq!(
        reader
            .symbol(SymbolId::new(i32::MAX as u32).unwrap())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidReference
    );
}

#[test]
fn data_error_counts_and_scalar_counts_are_distinct() {
    let reader = Reader::open("tests/fixtures/v7/operations.gdx", OpenOptions::default()).unwrap();
    let error = reader.find_symbol(b"errors").unwrap().unwrap();
    let meta = error.metadata().unwrap();
    assert_eq!(meta.error_count, 2);
    assert_eq!(meta.physical_record_count, 1);
    let reader = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let scalar = reader.find_symbol(b"default_scalar").unwrap().unwrap();
    let meta = scalar.metadata().unwrap();
    assert_eq!(meta.physical_record_count, 0);
    assert_eq!(meta.raw_record_count, 1);
}

#[cfg(not(feature = "compression"))]
#[test]
fn compressed_files_cannot_be_misread_as_raw() {
    assert_eq!(
        Reader::open("tests/fixtures/v7/blocks.gdx", OpenOptions::default())
            .err()
            .unwrap()
            .kind,
        ErrorKind::CompressionUnavailable
    );
}

#[test]
fn complete_public_reader_matches_all_ten_golden_streams() {
    let mut checked = 0;
    let mut rows = 0;
    for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "gdx") {
            continue;
        }
        let reader = match Reader::open(&path, OpenOptions::default()) {
            Ok(v) => v,
            Err(e)
                if !cfg!(feature = "compression")
                    && e.kind == ErrorKind::CompressionUnavailable =>
            {
                continue;
            }
            Err(e) => panic!("{e}"),
        };
        let expected = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
        let output = common::full_dump(&reader).unwrap();
        assert_eq!(output, expected, "{}", path.display());
        rows += output
            .lines()
            .filter(|line| line.starts_with("R\t"))
            .count();
        checked += 1;
    }
    assert_eq!(checked, if cfg!(feature = "compression") { 10 } else { 8 });
    if cfg!(feature = "compression") {
        assert_eq!(rows, 141358);
    }
}

#[test]
fn projection_limits_layout_reuse_and_source_binding() {
    let reader = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    let handle = reader.find_symbol(b"twenty").unwrap().unwrap();
    let mut projected = reader
        .scan(
            &handle,
            ScanOptions {
                key_projection: KeyProjection::Mask(1 << 19),
                value_projection: ValueProjection::Mask(1 << 4),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let mut full = reader.scan(&handle, reader.scan_options()).unwrap();
    let batch = projected.next_batch().unwrap().unwrap();
    let expected = full.next_batch().unwrap().unwrap();
    assert_eq!(batch.key_dimensions(), &[19]);
    assert_eq!(batch.value_fields(), &[ValueField::Scale]);
    assert_eq!(batch.key_column(19), expected.key_column(19));
    assert_eq!(
        batch.value_column(ValueField::Scale),
        expected.value_column(ValueField::Scale)
    );
    assert!(batch.key_column(0).is_none());
    assert!(batch.value_column(ValueField::Level).is_none());
    assert_eq!(
        batch.row(0).unwrap().key(19),
        Some(expected.key_column(19).unwrap()[0])
    );
    assert_eq!(
        batch.row(0).unwrap().value(ValueField::Scale),
        Some(expected.value_column(ValueField::Scale).unwrap()[0])
    );
    assert!(batch.row(batch.len()).is_none());
    assert!(!batch.is_empty());
    assert!(projected.next_batch().unwrap().is_none());
    let baseline = reader.budget_statistics().current_bytes;
    for (keys, values, bytes, kind) in [
        (
            KeyProjection::Mask(1 << 20),
            ValueProjection::All,
            65536,
            ErrorKind::InvalidProjection,
        ),
        (
            KeyProjection::All,
            ValueProjection::Mask(32),
            65536,
            ErrorKind::InvalidProjection,
        ),
        (
            KeyProjection::All,
            ValueProjection::All,
            1,
            ErrorKind::BatchTooSmall,
        ),
        (
            KeyProjection::All,
            ValueProjection::All,
            0,
            ErrorKind::InvalidOptions,
        ),
        (
            KeyProjection::All,
            ValueProjection::All,
            65537,
            ErrorKind::InvalidOptions,
        ),
    ] {
        assert_eq!(
            reader
                .scan(
                    &handle,
                    ScanOptions {
                        key_projection: keys,
                        value_projection: values,
                        batch_bytes: bytes,
                        limit: None
                    }
                )
                .err()
                .unwrap()
                .kind,
            kind
        );
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    let other = Reader::open("tests/fixtures/v7/semantics-0.gdx", OpenOptions::default()).unwrap();
    assert_eq!(
        other
            .scan(&handle, other.scan_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let universe = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let mut cursor = reader
        .scan(
            &universe,
            ScanOptions {
                batch_bytes: 28,
                limit: Some(3),
                ..reader.scan_options()
            },
        )
        .unwrap();
    let first = cursor.next_batch().unwrap().unwrap();
    let address = first.key_column(0).unwrap().as_ptr();
    assert_eq!(first.len(), 1);
    assert_eq!(first.key_column(0).unwrap()[0].get(), 1);
    let used = reader.budget_statistics().current_bytes;
    let second = cursor.next_batch().unwrap().unwrap();
    assert_eq!(second.key_column(0).unwrap().as_ptr(), address);
    assert_eq!(second.key_column(0).unwrap()[0].get(), 2);
    assert_eq!(reader.budget_statistics().current_bytes, used);
    assert_eq!(
        cursor.next_batch().unwrap().unwrap().key_column(0).unwrap()[0].get(),
        3
    );
    assert!(cursor.next_batch().unwrap().is_none());
    assert!(cursor.next_batch().unwrap().is_none());
}

#[test]
fn empty_projection_has_exact_limits_and_bounded_row_ceiling() {
    let reader = Reader::open("tests/fixtures/v7/key-widths.gdx", OpenOptions::default()).unwrap();
    let handle = reader.symbol(SymbolId::UNIVERSE).unwrap();
    for (limit, expected) in [(Some(12), 12), (None, 65536)] {
        let mut scan = reader
            .scan(
                &handle,
                ScanOptions {
                    key_projection: KeyProjection::Mask(0),
                    value_projection: ValueProjection::Mask(0),
                    limit,
                    batch_bytes: 1,
                },
            )
            .unwrap();
        let batch = scan.next_batch().unwrap().unwrap();
        assert_eq!(batch.len(), expected);
        assert!(batch.key_dimensions().is_empty());
        assert!(batch.value_fields().is_empty());
        assert!(batch.row(0).unwrap().key(0).is_none());
        assert!(scan.next_batch().unwrap().is_none());
    }
}

#[test]
fn retained_cursors_share_root_quota_and_release_after_drop() {
    let reader = Reader::open(
        "tests/fixtures/v7/key-widths.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 400 * 1024,
        },
    )
    .unwrap();
    let handle = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let mut cursors = Vec::new(); // Test-only at most three bounded cursors.
    let mut failed = false;
    for _ in 0..3 {
        let mut scan = reader.scan(&handle, reader.scan_options()).unwrap();
        match scan.next_batch() {
            Ok(Some(_)) => {}
            Err(e) => {
                assert_eq!(e.kind, ErrorKind::BudgetTooSmall);
                failed = true;
            }
            Ok(None) => panic!("empty universe"),
        }
        cursors.push(scan);
        if failed {
            break;
        }
    }
    assert!(failed);
    assert!(reader.budget_statistics().peak_bytes <= 1024 * 1024);
    drop(cursors);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn truncation_is_detected_even_with_buffered_data() {
    let bytes = std::fs::read("tests/fixtures/v7/semantics-0.gdx").unwrap();
    let copy = common::TempFile::new(&bytes);
    let reader = Reader::open(&copy.path, OpenOptions::default()).unwrap();
    let handle = reader.find_symbol(b"i").unwrap().unwrap();
    let mut cursor = reader
        .scan(
            &handle,
            ScanOptions {
                batch_bytes: 28,
                ..reader.scan_options()
            },
        )
        .unwrap();
    assert_eq!(cursor.next_batch().unwrap().unwrap().len(), 1);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&copy.path)
        .unwrap()
        .set_len(0)
        .unwrap();
    let error = cursor.next_batch().err().unwrap();
    assert_eq!(error.kind, ErrorKind::SourceChanged);
    assert_eq!(cursor.next_batch().err().unwrap().kind, error.kind);
}

#[test]
fn deterministic_byte_mutations_do_not_panic_or_grow_with_counts() {
    let original = std::fs::read("tests/fixtures/v7/optional-domains.gdx").unwrap();
    let mut mutations = 0;
    for offset in 0..original.len() {
        let mut bytes = original.clone();
        bytes[offset] ^= 0xff;
        let copy = common::TempFile::new(&bytes);
        let result = std::panic::catch_unwind(|| -> Result<()> {
            let reader = Reader::open(
                &copy.path,
                OpenOptions {
                    memory_budget_bytes: 1024 * 1024,
                    metadata_cache_bytes: 0,
                    ..OpenOptions::default()
                },
            )?;
            // Enumeration is bounded by physical source; arbitrary counts cannot allocate catalogs.
            let mut symbols = reader.symbols()?;
            while let Some(s) = symbols.next()? {
                let handle = reader.symbol(s.id)?;
                let mut scan = reader.scan(
                    &handle,
                    ScanOptions {
                        limit: Some(100),
                        ..reader.scan_options()
                    },
                )?;
                while scan.next_batch()?.is_some() {}
            }
            let mut uels = reader.uels()?;
            while uels.next()?.is_some() {}
            Ok(())
        });
        assert!(result.is_ok(), "panic at offset {offset}");
        mutations += 1;
    }
    assert_eq!(mutations, original.len());
}

#[test]
fn negative_artifacts_report_typed_errors_at_first_validation_boundary() {
    for (name, expected) in [
        ("version-5-probe", ErrorKind::UnsupportedVersion),
        ("version-6-probe", ErrorKind::UnsupportedVersion),
        ("future-version-probe", ErrorKind::UnsupportedVersion),
        ("bad-offset", ErrorKind::InvalidOffset),
        ("bad-dimension", ErrorKind::InvalidDimension),
        ("bad-key-tag", ErrorKind::InvalidKeyTag),
        ("bad-value-tag", ErrorKind::InvalidValueTag),
        ("bad-count", ErrorKind::InvalidCount),
        ("truncated-header", ErrorKind::UnexpectedEof),
        ("truncated-section", ErrorKind::UnexpectedEof),
        // This artifact removes the metadata as well as the block tail. Path
        // opening rejects its now out-of-file pointers before record decoding.
        ("truncated-block", ErrorKind::InvalidOffset),
        (
            "damaged-zlib-checksum",
            if cfg!(feature = "compression") {
                ErrorKind::InvalidCompression
            } else {
                ErrorKind::CompressionUnavailable
            },
        ),
    ] {
        let result = Reader::open(
            format!("tests/fixtures/invalid/{name}.gdx"),
            OpenOptions::default(),
        )
        .and_then(|reader| common::full_dump(&reader));
        assert_eq!(result.err().unwrap().kind, expected, "{name}");
    }
}

#[path = "cases/slicing.rs"]
mod slicing;

#[path = "cases/labels.rs"]
mod labels;

#[path = "cases/spill.rs"]
mod spill;

#[path = "cases/large.rs"]
mod large;
