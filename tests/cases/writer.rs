use gdxrs::*;
#[test]
fn public_empty_writer_shared_budget_unicode_paths_and_no_clobber() {
    let marker = super::common::TempFile::new(b"caller");
    let parent = marker.path.with_extension("writer-é-日本語");
    std::fs::create_dir(&parent).unwrap();
    let output = parent.join("новый.gdx");
    let r = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = r.budget_statistics().current_bytes;
    let mut writer = r.create_writer(&output, r.write_options()).unwrap();
    assert!(!output.exists());
    let temp = writer.temporary_path().to_path_buf();
    let stats = writer.finish().unwrap();
    assert_eq!(stats.status, WriteStatus::Complete);
    assert!(stats.published);
    assert_eq!((stats.output_bytes, stats.bytes_written), (197, 249));
    assert!(!temp.exists());
    drop(writer);
    assert_eq!(r.budget_statistics().current_bytes, before);
    drop(r);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let info = copy.information().unwrap();
    assert_eq!(
        (
            info.version,
            info.compression_level,
            info.uel_count,
            info.symbol_count
        ),
        (7, 0, 0, 0)
    );
    assert_eq!(info.producer, b"gdxrs");
    assert_eq!(info.audit, b"gdxrs");
    assert!(copy.symbols().unwrap().next().unwrap().is_none());
    let universe = copy.symbol(SymbolId::UNIVERSE).unwrap();
    assert!(
        copy.scan(&universe, copy.scan_options())
            .unwrap()
            .next_batch()
            .unwrap()
            .is_none()
    );
    drop(universe);
    drop(copy);
    assert!(Writer::create(&output, WriteOptions::default()).is_err());
    assert_eq!(std::fs::metadata(&output).unwrap().len(), 197);
    std::fs::remove_file(&output).unwrap();
    std::fs::remove_dir(parent).unwrap();
}

#[cfg(feature = "compression")]
#[test]
fn public_compressed_empty_writer_shares_root_and_validates_metadata() {
    let marker = super::common::TempFile::new(b"caller");
    let output = marker.path.with_extension("zlib.gdx");
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let before = reader.budget_statistics().current_bytes;
    let mut writer = reader
        .create_writer(
            &output,
            WriteOptions {
                compression: WriteCompression::Zlib,
                ..reader.write_options()
            },
        )
        .unwrap();
    assert!(reader.budget_statistics().current_bytes > before + 320 * 1024);
    assert!(!output.exists());
    let stats = writer.finish().unwrap();
    assert_eq!(stats.status, WriteStatus::Complete);
    assert_eq!(stats.bytes_written, stats.output_bytes + 52);
    drop(writer);
    assert_eq!(reader.budget_statistics().current_bytes, before);
    let copy = Reader::open(&output, OpenOptions::default()).unwrap();
    let info = copy.information().unwrap();
    assert_eq!(
        (
            info.version,
            info.compression_level,
            info.symbol_count,
            info.uel_count
        ),
        (7, 1, 0, 0)
    );
    assert_eq!(
        (info.producer, info.audit),
        (b"gdxrs".as_slice(), b"gdxrs".as_slice())
    );
    assert!(copy.symbols().unwrap().next().unwrap().is_none());
    assert!(copy.uels().unwrap().next().unwrap().is_none());
    assert!(copy.acronyms().unwrap().next().unwrap().is_none());
    let mut texts = copy.set_texts().unwrap();
    assert_eq!(texts.next().unwrap().unwrap().text, b"");
    assert!(texts.next().unwrap().is_none());
    drop(texts);
    drop(copy);
    std::fs::remove_file(output).unwrap();
}
