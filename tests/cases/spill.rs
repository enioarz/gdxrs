use gdxrs::*;
#[test]
fn public_scratch_sort_orders_bytes_preserves_duplicates_and_cleans_parent() {
    let reader = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions::default(),
    )
    .unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let parent = super::common::TempFile::new(b"marker");
    let directory = parent.path.with_extension("spill-dir");
    std::fs::create_dir(&directory).unwrap();
    let mut options = SpillOptions::new(&directory);
    options.run_bytes = 1024;
    options.merge_fan_in = 2;
    options.maximum_open_files = 3;
    options.maximum_temporary_bytes = 65536;
    let mut sorter = reader.spill_sort(&options).unwrap();
    let namespace = sorter.directory().to_path_buf();
    for (label, id) in [
        (b"z".as_slice(), 2),
        (b"A", 1),
        (b"a", 3),
        (b"A", 1),
        (b"", 0),
        (b"\xff", u64::MAX),
        (b"b", 4),
    ] {
        sorter.push(label, id).unwrap();
    }
    let mut cursor = sorter.finish().unwrap();
    for (label, id) in [
        (b"".as_slice(), 0),
        (b"A", 1),
        (b"A", 1),
        (b"a", 3),
        (b"b", 4),
        (b"z", 2),
        (b"\xff", u64::MAX),
    ] {
        let record = cursor.next().unwrap().unwrap();
        assert_eq!(record.label, label);
        assert_eq!(record.id, id);
    }
    assert!(cursor.next().unwrap().is_none());
    let statistics = cursor.statistics();
    assert!(statistics.complete);
    assert_eq!(statistics.returned_records, 7);
    assert_eq!(statistics.open_files, 0);
    assert_eq!(statistics.temporary_bytes, 0);
    assert!(statistics.peak_open_files <= 3);
    assert!(statistics.peak_temporary_bytes <= 65536);
    assert!(!namespace.exists());
    drop(cursor);
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir(directory).unwrap();
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}
