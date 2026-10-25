use super::*;
use std::{
    cell::RefCell,
    io::{Seek, SeekFrom, Write},
    sync::atomic::AtomicUsize,
};
#[derive(Default)]
struct Faults {
    fault: Option<(Event, usize, bool, io::ErrorKind)>,
    read: usize,
    write: usize,
    namespace: Option<u64>,
    deny_cleanup: bool,
}
thread_local! {static FAULTS:RefCell<Faults>=RefCell::new(Faults::default());}
pub(super) fn namespace_override() -> Option<u64> {
    FAULTS.with(|s| s.borrow_mut().namespace.take())
}
pub(super) fn event(kind: Event) -> io::Result<()> {
    FAULTS.with(|state| {
        let mut state = state.borrow_mut();
        if kind == Event::Remove && state.deny_cleanup {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cleanup denied",
            ));
        }
        if let Some((event, after, persistent, error)) = state.fault
            && event == kind
        {
            if after == 0 {
                if !persistent {
                    state.fault = None;
                }
                return Err(io::Error::new(error, "injected spill failure"));
            }
            state.fault = Some((event, after - 1, persistent, error));
        }
        Ok(())
    })
}
pub(super) fn io_chunk(length: usize, write: bool) -> usize {
    FAULTS.with(|s| {
        let s = s.borrow();
        let limit = if write { s.write } else { s.read };
        if limit == 0 {
            length
        } else {
            length.min(limit)
        }
    })
}
fn fault(event: Event, after: usize, persistent: bool, error: io::ErrorKind) {
    FAULTS.with(|s| s.borrow_mut().fault = Some((event, after, persistent, error)));
}
struct Reset;
impl Reset {
    fn new() -> Self {
        FAULTS.with(|s| *s.borrow_mut() = Faults::default());
        Self
    }
}
impl Drop for Reset {
    fn drop(&mut self) {
        FAULTS.with(|s| *s.borrow_mut() = Faults::default());
    }
}
struct Parent(PathBuf);
impl Parent {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = PathBuf::from(".work").join(format!(
            "spill-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn empty(&self) {
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 0);
    }
}
impl Drop for Parent {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reader() -> Reader {
    Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
fn options(parent: &Parent) -> SpillOptions {
    SpillOptions {
        directory: parent.0.clone(),
        run_bytes: 2 * size_of::<Record>(),
        maximum_temporary_bytes: 2 * 1024 * 1024,
        maximum_open_files: 3,
        merge_fan_in: 2,
    }
}
fn rows(sorter: &mut SpillSorter) {
    let long = [b'x'; 255];
    for _ in 0..7 {
        for (key, id) in [
            (b"b".as_slice(), 2),
            (b"a", 3),
            (b"A", 1),
            (b"a", 1),
            (b"a", 3),
            (b"", 0),
            (b"\xff", u64::MAX),
            (b"\0", 4),
            (b"b", 0),
            (b"a", 1),
            (&long, 42),
        ] {
            sorter.push(key, id).unwrap();
        }
    }
}
#[test]
fn multipass_sorted_duplicates_limits_short_io_and_final_owner_drop() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let budget = Arc::clone(&reader.source.budget);
    FAULTS.with(|s| {
        let mut s = s.borrow_mut();
        s.read = 7;
        s.write = 11;
    });
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    let directory = sorter.directory().to_path_buf();
    rows(&mut sorter);
    let mut output = sorter.finish().unwrap();
    assert!(!output.statistics().complete);
    drop(reader);
    let long = [b'x'; 255];
    let groups = [
        (b"".as_slice(), 0, 7),
        (b"\0", 4, 7),
        (b"A", 1, 7),
        (b"a", 1, 14),
        (b"a", 3, 14),
        (b"b", 0, 7),
        (b"b", 2, 7),
        (&long, 42, 7),
        (b"\xff", u64::MAX, 7),
    ];
    for (label, id, count) in groups {
        for _ in 0..count {
            let entry = output.next().unwrap().unwrap();
            assert_eq!(entry.label, label);
            assert_eq!(entry.id, id);
        }
    }
    assert!(output.next().unwrap().is_none());
    assert!(output.next().unwrap().is_none());
    let stats = output.statistics();
    assert_eq!(stats.input_records, 77);
    assert_eq!(stats.returned_records, 77);
    assert!(stats.complete);
    assert!(stats.merge_passes >= 5);
    assert!(stats.published_runs > 39);
    assert!(stats.peak_temporary_bytes <= 2 * 1024 * 1024);
    assert_eq!(stats.peak_open_files, 3);
    assert_eq!(stats.open_files, 0);
    assert_eq!(stats.temporary_bytes, 0);
    assert!(!directory.exists());
    parent.empty();
    assert!(budget.statistics().peak_bytes <= 1024 * 1024);
    drop(output);
    assert_eq!(budget.statistics().current_bytes, 0);
    eprintln!(
        "M2 spill: record_bytes={}, passes={}, runs={}, peak_files={}, peak_temp={}, peak_root={}",
        size_of::<Record>(),
        stats.merge_passes,
        stats.published_runs,
        stats.peak_open_files,
        stats.peak_temporary_bytes,
        budget.statistics().peak_bytes
    );
}
#[test]
fn empty_cancel_and_early_close_are_distinguished_from_complete_results() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    let mut opts = options(&parent);
    opts.maximum_temporary_bytes = 0;
    let mut empty = reader.spill_sort(&opts).unwrap().finish().unwrap();
    assert!(empty.next().unwrap().is_none());
    assert!(empty.statistics().complete);
    assert_eq!(empty.statistics().published_runs, 0);
    drop(empty);
    parent.empty();
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    rows(&mut sorter);
    let stats = sorter.cancel().unwrap();
    assert!(!stats.complete);
    assert_eq!(stats.open_files, 0);
    assert_eq!(stats.temporary_bytes, 0);
    parent.empty();
    drop(sorter);
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    rows(&mut sorter);
    let mut cursor = sorter.finish().unwrap();
    assert!(cursor.next().unwrap().is_some());
    assert!(!cursor.close().unwrap().complete);
    assert!(cursor.next().unwrap().is_none());
    parent.empty();
    drop(cursor);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}
#[test]
fn byte_quota_boundaries_include_live_input_and_output_runs() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    // One empty-label record: header 8 + frame 10 + footer 26 = 44 bytes.
    for limit in [0, 43, 44] {
        let mut opts = options(&parent);
        opts.maximum_temporary_bytes = limit;
        let mut sorter = reader.spill_sort(&opts).unwrap();
        sorter.push(b"", 0).unwrap();
        if limit < 44 {
            assert_eq!(
                sorter.finish().err().unwrap().kind,
                ErrorKind::TemporarySpaceExceeded
            );
        } else {
            let mut cursor = sorter.finish().unwrap();
            assert_eq!(cursor.next().unwrap().unwrap().id, 0);
            assert!(cursor.next().unwrap().is_none());
            assert_eq!(cursor.statistics().peak_temporary_bytes, 44);
        }
        parent.empty();
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    let mut opts = options(&parent);
    opts.run_bytes = size_of::<Record>();
    opts.maximum_temporary_bytes = 88;
    let mut sorter = reader.spill_sort(&opts).unwrap();
    sorter.push(b"", 1).unwrap();
    sorter.push(b"", 2).unwrap();
    assert_eq!(
        sorter.finish().err().unwrap().kind,
        ErrorKind::TemporarySpaceExceeded
    );
    parent.empty();
}
#[test]
fn invalid_options_allocation_and_handle_quota_do_not_leak() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    for (run, fan, handles, kind) in [
        (0, 2, 3, ErrorKind::BudgetTooSmall),
        (528, 1, 3, ErrorKind::InvalidOptions),
        (528, 65, 66, ErrorKind::InvalidOptions),
        (528, 3, 3, ErrorKind::InvalidOptions),
    ] {
        let mut opts = options(&parent);
        opts.run_bytes = run;
        opts.merge_fan_in = fan;
        opts.maximum_open_files = handles;
        assert_eq!(reader.spill_sort(&opts).err().unwrap().kind, kind);
        parent.empty();
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    crate::budget::inject_buffer_failure();
    assert_eq!(
        reader.spill_sort(&options(&parent)).err().unwrap().kind,
        ErrorKind::AllocationFailed
    );
    parent.empty();
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    let handle = sorter.session.metrics.handle().unwrap();
    let h2 = sorter.session.metrics.handle().unwrap();
    let h3 = sorter.session.metrics.handle().unwrap();
    assert_eq!(
        sorter.session.metrics.handle().err().unwrap().kind,
        ErrorKind::FileLimitExceeded
    );
    drop(handle);
    drop(h2);
    drop(h3);
    sorter.cancel().unwrap();
    drop(sorter);
    parent.empty();
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}
#[test]
fn input_and_every_io_finalization_failure_is_terminal_and_cleans_up() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    for (event, kind) in [
        (Event::Create, io::ErrorKind::PermissionDenied),
        (Event::Write, io::ErrorKind::StorageFull),
        (Event::Flush, io::ErrorKind::Other),
        (Event::Publish, io::ErrorKind::PermissionDenied),
        (Event::Read, io::ErrorKind::PermissionDenied),
        (Event::Remove, io::ErrorKind::PermissionDenied),
    ] {
        let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
        sorter.push(b"b", 2).unwrap();
        sorter.push(b"a", 1).unwrap();
        fault(event, 0, false, kind);
        let error = sorter.finish().err().unwrap();
        assert_eq!(error.kind, ErrorKind::Io);
        assert!(std::error::Error::source(&error).is_some());
        parent.empty();
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    assert_eq!(
        sorter.push(&[0; 256], 0).err().unwrap().kind,
        ErrorKind::InvalidOptions
    );
    assert_eq!(
        sorter.push(b"a", 1).err().unwrap().kind,
        ErrorKind::InvalidOptions
    );
    parent.empty();
    drop(sorter);
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"a", 1).unwrap();
    sorter.push(b"b", 2).unwrap();
    fault(Event::Write, 0, false, io::ErrorKind::StorageFull);
    assert_eq!(sorter.push(b"c", 3).err().unwrap().kind, ErrorKind::Io);
    assert_eq!(sorter.push(b"c", 3).err().unwrap().kind, ErrorKind::Io);
    parent.empty();
    drop(sorter);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}
#[test]
fn explicit_cleanup_retry_and_secondary_failure_preserve_primary_error() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let mut io_sorter = reader.spill_sort(&options(&parent)).unwrap();
    io_sorter.push(b"a", 1).unwrap();
    io_sorter.push(b"b", 2).unwrap();
    FAULTS.with(|s| s.borrow_mut().deny_cleanup = true);
    fault(Event::Write, 0, false, io::ErrorKind::StorageFull);
    let io_error = io_sorter.push(b"c", 3).err().unwrap();
    assert_eq!(io_error.kind, ErrorKind::Io);
    assert_eq!(
        std::error::Error::source(&io_error)
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::StorageFull
    );
    assert_eq!(
        io_error.cleanup_error().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    FAULTS.with(|s| s.borrow_mut().deny_cleanup = false);
    io_sorter.cancel().unwrap();
    parent.empty();
    drop(io_sorter);
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"a", 1).unwrap();
    sorter.push(b"b", 2).unwrap();
    sorter.push(b"c", 3).unwrap();
    fault(Event::Remove, 0, true, io::ErrorKind::PermissionDenied);
    let error = sorter.push(&[0; 256], 9).err().unwrap();
    assert_eq!(error.kind, ErrorKind::InvalidOptions);
    assert!(error.cleanup_error().is_some());
    assert!(error.to_string().contains("cleanup also failed"));
    assert!(sorter.cancel().is_err());
    assert!(sorter.directory().exists());
    FAULTS.with(|s| s.borrow_mut().fault = None);
    assert_eq!(sorter.cancel().unwrap().temporary_bytes, 0);
    parent.empty();
    drop(sorter);
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"x", 1).unwrap();
    let mut cursor = sorter.finish().unwrap();
    cursor.next().unwrap();
    fault(Event::Remove, 0, true, io::ErrorKind::PermissionDenied);
    assert_eq!(cursor.next().err().unwrap().kind, ErrorKind::Io);
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.next().err().unwrap().kind, ErrorKind::Io);
    FAULTS.with(|s| s.borrow_mut().fault = None);
    cursor.close().unwrap();
    parent.empty();
}
#[test]
fn namespace_collisions_and_output_publication_never_overwrite_foreign_files() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let existing = fs::canonicalize(&parent.0).unwrap().join(format!(
        "gdxrs-spill-{:08x}-{:016x}",
        std::process::id(),
        999_999
    ));
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep"), b"preserve").unwrap();
    FAULTS.with(|s| s.borrow_mut().namespace = Some(999_999));
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    assert_ne!(sorter.directory(), existing);
    sorter.cancel().unwrap();
    assert_eq!(fs::read(existing.join("keep")).unwrap(), b"preserve");
    fs::remove_file(existing.join("keep")).unwrap();
    fs::remove_dir(existing).unwrap();
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"a", 1).unwrap();
    let path = sorter.session.path(0, false);
    fs::write(&path, b"not owned").unwrap();
    let error = sorter.flush().err().unwrap();
    assert_eq!(error.kind, ErrorKind::Io);
    let error = sorter.fail(error);
    assert!(error.cleanup_error().is_some());
    assert_eq!(fs::read(&path).unwrap(), b"not owned");
    fs::remove_file(path).unwrap();
    sorter.cancel().unwrap();
    parent.empty();
}
#[test]
fn malformed_spill_frames_are_bounded_and_never_report_complete() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    for mutation in 0..8 {
        let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
        sorter.push(b"a", 1).unwrap();
        sorter.push(b"b", 2).unwrap();
        sorter.flush().unwrap();
        let path = sorter.session.path(0, false);
        let mut bytes = fs::read(&path).unwrap();
        let len = bytes.len();
        match mutation {
            0 => bytes[0] ^= 1,
            1 => bytes[8..10].copy_from_slice(&256u16.to_le_bytes()),
            2 => bytes[18] = b'z',
            3 => bytes[len - 24] ^= 1,
            4 => bytes[len - 16] ^= 1,
            5 => bytes[len - 1] ^= 1,
            6 => {
                bytes.pop();
            }
            _ => {
                bytes.extend_from_slice(b"trailing");
            }
        }
        fs::write(&path, &bytes).unwrap();
        let result = RunReader::open(&sorter.session, 0).and_then(|mut r| {
            while r.advance()?.is_some() {}
            Ok(())
        });
        assert!(matches!(
            result.err().unwrap().kind,
            ErrorKind::InvalidSpill | ErrorKind::UnexpectedEof
        ));
        sorter.cancel().unwrap();
        parent.empty();
    }
}
#[test]
fn final_cursor_read_failure_and_corrupt_footer_preserve_partial_output() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    for read_fault in [true, false] {
        let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
        sorter.push(b"a", 1).unwrap();
        sorter.push(b"b", 2).unwrap();
        // Force incremental physical reads so the first entry escapes before failure.
        FAULTS.with(|s| s.borrow_mut().read = 1);
        let mut cursor = sorter.finish().unwrap();
        let path = cursor.session.path(0, false);
        let first = cursor.next().unwrap().unwrap();
        assert_eq!(first.label, b"a");
        if read_fault {
            fault(Event::Read, 0, false, io::ErrorKind::PermissionDenied);
        } else {
            let mut file = fs::OpenOptions::new().write(true).open(path).unwrap();
            file.seek(SeekFrom::End(-1)).unwrap();
            file.write_all(b"X").unwrap();
        }
        let error = loop {
            match cursor.next() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("accepted failure"),
                Err(e) => break e,
            }
        };
        assert_eq!(
            error.kind,
            if read_fault {
                ErrorKind::Io
            } else {
                ErrorKind::InvalidSpill
            }
        );
        assert!(!cursor.statistics().complete);
        let returned = cursor.statistics().returned_records;
        assert!(returned >= 1);
        assert_eq!(cursor.next().err().unwrap().kind, error.kind);
        assert_eq!(cursor.statistics().returned_records, returned);
        assert_eq!(cursor.statistics().open_files, 0);
        assert_eq!(cursor.statistics().temporary_bytes, 0);
        parent.empty();
    }
}

#[test]
fn partial_writes_allocation_and_merge_quota_failures_clean_every_owned_file() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    let mut opts = options(&parent);
    opts.run_bytes = 512 * size_of::<Record>();
    let mut sorter = reader.spill_sort(&opts).unwrap();
    for n in 0..300 {
        sorter.push(&[b'x'; 255], n).unwrap();
    }
    let metrics = Arc::clone(&sorter.session.metrics);
    FAULTS.with(|s| s.borrow_mut().write = 17);
    fault(Event::Write, 2, false, io::ErrorKind::StorageFull);
    let error = sorter.finish().err().unwrap();
    assert_eq!(error.kind, ErrorKind::Io);
    let stats = metrics.snapshot();
    assert!(stats.bytes_written > 0 && stats.bytes_written < IO_BYTES as u64);
    assert_eq!(stats.published_runs, 0);
    assert_eq!(stats.open_files, 0);
    assert_eq!(stats.temporary_bytes, 0);
    assert!(!stats.complete);
    parent.empty();
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
    FAULTS.with(|s| s.borrow_mut().write = 0);
    for phase in 0..3 {
        let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
        sorter.push(b"a", 1).unwrap();
        sorter.flush().unwrap();
        sorter.push(b"b", 2).unwrap();
        sorter.flush().unwrap();
        let held = if phase == 1 {
            Some(
                reader
                    .source
                    .budget
                    .reserve(
                        reader.source.budget.statistics().limit_bytes
                            - reader.budget_statistics().current_bytes
                            - 100,
                    )
                    .unwrap(),
            )
        } else {
            None
        };
        if phase == 0 {
            crate::budget::inject_buffer_failure();
        } else if phase == 2 {
            fault(Event::Read, 1, false, io::ErrorKind::PermissionDenied);
        }
        let error = sorter.finish().err().unwrap();
        assert_eq!(
            error.kind,
            match phase {
                0 => ErrorKind::AllocationFailed,
                1 => ErrorKind::BudgetTooSmall,
                _ => ErrorKind::Io,
            }
        );
        drop(held);
        parent.empty();
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
}

#[test]
fn drop_cancels_unfinished_sorters_and_partially_consumed_cursors() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let baseline = reader.budget_statistics().current_bytes;
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    for n in 0..4 {
        sorter.push(b"x", n).unwrap();
    }
    assert!(sorter.statistics().published_runs > 0);
    drop(sorter);
    parent.empty();
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    for n in 0..4 {
        sorter.push(b"x", n).unwrap();
    }
    let mut cursor = sorter.finish().unwrap();
    cursor.next().unwrap().unwrap();
    assert!(!cursor.statistics().complete);
    drop(cursor);
    parent.empty();
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn independent_wire_vector_and_interrupted_io_match_the_private_format() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"", 0).unwrap();
    fault(Event::Write, 0, false, io::ErrorKind::Interrupted);
    sorter.flush().unwrap();
    // Independently evaluated FNV-1a-64 of ten zero frame bytes, with Python
    // integer arithmetic modulo 2^64: 0x69d307cc20f6ef8d.
    let mut expected = b"GDXRUN01".to_vec();
    expected.extend_from_slice(&[0; 10]);
    expected.extend_from_slice(&[255, 255]);
    expected.extend_from_slice(&1u64.to_le_bytes());
    expected.extend_from_slice(&0x69d307cc20f6ef8du64.to_le_bytes());
    expected.extend_from_slice(b"GDXEND01");
    assert_eq!(fs::read(sorter.session.path(0, false)).unwrap(), expected);
    fault(Event::Read, 0, false, io::ErrorKind::Interrupted);
    let mut run = RunReader::open(&sorter.session, 0).unwrap();
    let row = run.advance().unwrap().unwrap();
    assert!(row.label.as_slice().is_empty());
    assert_eq!(row.id, 0);
    assert!(run.advance().unwrap().is_none());
    drop(run);
    sorter.cancel().unwrap();
    parent.empty();
}

#[test]
fn valid_empty_run_cannot_hide_loss_of_an_accepted_record() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let reader = reader();
    let mut sorter = reader.spill_sort(&options(&parent)).unwrap();
    sorter.push(b"a", 1).unwrap();
    sorter.flush().unwrap();
    let mut empty = b"GDXRUN01".to_vec();
    empty.extend_from_slice(&[255, 255]);
    empty.extend_from_slice(&0u64.to_le_bytes());
    empty.extend_from_slice(&14695981039346656037u64.to_le_bytes());
    empty.extend_from_slice(b"GDXEND01");
    fs::write(sorter.session.path(0, false), empty).unwrap();
    let mut cursor = sorter.finish().unwrap();
    assert_eq!(cursor.next().err().unwrap().kind, ErrorKind::InvalidSpill);
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.statistics().returned_records, 0);
    assert_eq!(cursor.statistics().input_records, 1);
    parent.empty();
}
