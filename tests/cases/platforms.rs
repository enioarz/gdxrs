use gdxrs::*;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct UnicodeArea {
    path: PathBuf,
    _marker: super::common::TempFile,
}
impl UnicodeArea {
    fn new() -> Self {
        let marker = super::common::TempFile::new(b"caller marker");
        let path = marker.path.with_extension("café-日本語-Δ");
        fs::create_dir(&path).unwrap();
        Self {
            path,
            _marker: marker,
        }
    }
}
impl Drop for UnicodeArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn unicode_paths_spill_membership_and_explicit_handle_cleanup() {
    let area = UnicodeArea::new();
    let input = area.path.join("данные-資料.gdx");
    fs::copy("tests/fixtures/v7/case-distinct-labels.gdx", &input).unwrap();
    let reader = Reader::open(&input, OpenOptions::default()).unwrap();
    assert_eq!(
        super::common::full_dump(&reader).unwrap(),
        fs::read_to_string("tests/fixtures/v7/case-distinct-labels.expected.tsv").unwrap()
    );
    let baseline = reader.budget_statistics().current_bytes;
    let parent = area.path.join("scratch-é-作業");
    fs::create_dir(&parent).unwrap();
    let sentinel = parent.join("caller-owned.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let mut options = SpillOptions::new(&parent);
    options.run_bytes = 528;
    options.merge_fan_in = 2;
    options.maximum_open_files = 3;
    let mut sort = reader.spill_sort(&options).unwrap();
    let private = sort.directory().to_path_buf();
    for n in (0..23).rev() {
        sort.push(b"label", n).unwrap();
    }
    let mut cursor = sort.finish().unwrap();
    for n in 0..23 {
        let entry = cursor.next().unwrap().unwrap();
        assert_eq!((entry.label, entry.id), (b"label".as_slice(), n));
    }
    assert!(cursor.next().unwrap().is_none());
    assert!(cursor.statistics().complete);
    assert!(cursor.statistics().merge_passes >= 3);
    drop(cursor);
    assert!(!private.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
    options.maximum_open_files = 9;
    let mut lookup = reader.large_label_lookup(&options).unwrap();
    for bytes in [b"a".as_slice(), b"A", b"unknown"] {
        lookup.push(bytes).unwrap();
    }
    let symbol = reader.find_symbol(b"I").unwrap().unwrap();
    let base = reader.selection(&symbol, &[]).unwrap();
    let mut selection = reader
        .selection_from_large_labels(&symbol, 0, &base, lookup.finish().unwrap(), &options)
        .unwrap();
    let namespace = selection.spill_directory().unwrap().to_path_buf();
    let mut scan = reader
        .scan_selected(&symbol, &selection, reader.scan_options())
        .unwrap();
    assert_eq!(
        selection.close_spill().unwrap_err().kind,
        ErrorKind::InvalidOptions
    );
    let mut actual = Vec::new();
    while let Some(batch) = scan.next_batch().unwrap() {
        actual.extend(batch.key_column(0).unwrap().iter().map(|id| id.get()));
    }
    assert_eq!(actual, vec![2, 3]);
    drop(scan);
    selection.close_spill().unwrap();
    assert!(!namespace.exists());
    drop(selection);
    drop(base);
    drop(symbol);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
    drop(reader);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_file(&sentinel).unwrap();
    fs::remove_dir(&parent).unwrap();
    fs::remove_file(&input).unwrap();
    assert_eq!(fs::read_dir(&area.path).unwrap().count(), 0);
}

#[test]
fn cloned_readers_use_independent_positions_on_native_file_handles() {
    let mut paths = vec!["tests/fixtures/v7/key-widths.gdx"];
    if cfg!(feature = "compression") {
        paths.push("tests/fixtures/v7/blocks.gdx");
    }
    for path in paths {
        let reader = Reader::open(path, OpenOptions::default()).unwrap();
        let baseline = reader.budget_statistics().current_bytes;
        let expected = Arc::new(
            fs::read_to_string(PathBuf::from(path).with_extension("expected.tsv")).unwrap(),
        );
        assert!(expected.len() < 8 * 1024 * 1024); // Frozen development expectation only.
        let barrier = Arc::new(Barrier::new(4));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let shared = reader.clone();
            let expected = Arc::clone(&expected);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..2 {
                    assert_eq!(super::common::full_dump(&shared).unwrap(), *expected);
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
}
