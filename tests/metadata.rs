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
