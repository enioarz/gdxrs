use super::*;
use std::{
    sync::atomic::{AtomicU64, AtomicUsize},
    time::{Duration, Instant},
};

pub(super) struct Hooks {
    pub hold_first: AtomicBool,
    pub hold_pending: AtomicBool,
    pub fail_spawn: AtomicUsize,
    pub panic_step: AtomicUsize,
    pub panic_join: AtomicUsize,
    pub started: AtomicUsize,
    pub exited: AtomicUsize,
    pub coordinator_wait: AtomicBool,
    source_on_exit: Mutex<Option<Arc<Data>>>,
    waiting: [AtomicUsize; MAX_WORKERS],
}
impl Default for Hooks {
    fn default() -> Self {
        Self {
            hold_first: AtomicBool::new(false),
            hold_pending: AtomicBool::new(false),
            fail_spawn: AtomicUsize::new(usize::MAX),
            panic_step: AtomicUsize::new(usize::MAX),
            panic_join: AtomicUsize::new(usize::MAX),
            started: AtomicUsize::new(0),
            exited: AtomicUsize::new(0),
            coordinator_wait: AtomicBool::new(false),
            source_on_exit: Mutex::new(None),
            waiting: std::array::from_fn(|_| AtomicUsize::new(0)),
        }
    }
}
pub(super) fn after_exit(shared: &Shared, slot: usize) {
    if slot == 0
        && let Some(data) = shared.hooks.source_on_exit.lock().unwrap().as_ref()
    {
        data.changed.store(true, Ordering::SeqCst);
    }
}
pub(super) fn waiting(shared: &Shared, slot: usize, reason: usize) {
    shared.hooks.waiting[slot].store(reason, Ordering::SeqCst);
    shared.signal.notify();
}
pub(super) fn pause(shared: &Shared, slot: usize, pending: bool) {
    if slot != 0 {
        return;
    }
    let flag = if pending {
        &shared.hooks.hold_pending
    } else {
        &shared.hooks.hold_first
    };
    if !flag.load(Ordering::SeqCst) {
        return;
    }
    let mut guard = shared.lock();
    waiting(shared, slot, if pending { 4 } else { 2 });
    while flag.load(Ordering::SeqCst) && !shared.signal.stop.load(Ordering::Acquire) {
        guard = shared.wait(guard);
    }
    waiting(shared, slot, 0);
}
pub(super) fn before_step(shared: &Shared, slot: usize) {
    if shared
        .hooks
        .panic_step
        .compare_exchange(slot, usize::MAX, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        panic!("injected decode-step panic");
    }
}
pub(super) fn spawn_error(shared: &Shared, slot: usize) -> Option<Error> {
    (shared.hooks.fail_spawn.load(Ordering::SeqCst) == slot).then(|| {
        Error::io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected spawn denial",
        ))
        .with_cleanup(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "initial cleanup denial",
        ))
    })
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(".work").join(format!(
            "parallel-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(".work").unwrap();
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reader() -> Reader {
    Reader::open(
        "tests/fixtures/v7/key-widths.gdx",
        OpenOptions {
            memory_budget_bytes: 16 * 1024 * 1024,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 4096,
        },
    )
    .unwrap()
}
fn index(reader: &Reader, symbol: &SymbolHandle, stride: u64) -> (Temp, CheckpointIndex) {
    let temp = Temp::new();
    let path = temp.0.join("index");
    let opts = IndexOptions {
        record_stride: stride,
        decoded_byte_stride: u64::MAX,
        ..IndexOptions::default()
    };
    reader
        .index_builder(symbol, &path, opts)
        .unwrap()
        .finish()
        .unwrap();
    let idx = reader
        .open_index(symbol, &path, opts, IndexPolicy::UseIfValid)
        .unwrap()
        .index
        .unwrap();
    (temp, idx)
}
fn until(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "worker did not reach expected state"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
fn hooked(
    reader: &Reader,
    symbol: &SymbolHandle,
    selection: Option<&Selection>,
    index: CheckpointIndex,
    options: ParallelOptions,
    hooks: Arc<Hooks>,
) -> ParallelScanCursor {
    ParallelScanCursor::new_inner(
        reader,
        symbol,
        selection,
        index,
        ScanOptions {
            batch_bytes: 80,
            ..reader.scan_options()
        },
        options,
        hooks,
    )
    .unwrap()
}

#[test]
fn parallel_slow_frontier_backpressures_and_external_cancel_wakes_coordinator() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let (_temp, index) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        index,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    until(|| {
        hooks.waiting[0].load(Ordering::SeqCst) == 2 && hooks.waiting[1].load(Ordering::SeqCst) == 3
    });
    let s = cursor.statistics();
    assert_eq!(s.produced_rows, 4);
    assert_eq!(s.decoded_rows, 4);
    assert_eq!(s.peak_ready_batches, 1);
    let signal = cursor.cancellation_handle();
    let (send, receive) = std::sync::mpsc::channel();
    let consumer = thread::spawn(move || {
        assert!(cursor.next_batch().unwrap().is_none());
        let s = cursor.statistics();
        drop(cursor);
        send.send(s).unwrap();
    });
    until(|| hooks.coordinator_wait.load(Ordering::SeqCst));
    signal.request_cancel();
    let s = receive.recv_timeout(Duration::from_secs(15)).unwrap();
    consumer.join().unwrap();
    assert!(s.cancelled && !s.complete);
    assert_eq!(s.returned_rows, 0);
    assert_eq!(s.joined_workers, 2);
    assert_eq!(
        hooks.started.load(Ordering::SeqCst),
        hooks.exited.load(Ordering::SeqCst)
    );
    drop(signal);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_cancel_wakes_idle_task_wait_and_rejected_chunk_poll() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(1).unwrap()),
            }],
        )
        .unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        Some(&selection),
        idx,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    until(|| {
        hooks.waiting[1].load(Ordering::SeqCst) == 1 && cursor.statistics().tasks_completed == 1
    });
    cursor.cancel().unwrap();
    assert_eq!(cursor.statistics().joined_workers, 2);
    drop(cursor);
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(65536).unwrap()),
            }],
        )
        .unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_pending.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        Some(&selection),
        idx,
        ParallelOptions {
            intervals_per_task: 1_000_000,
            ..ParallelOptions::default()
        },
        Arc::clone(&hooks),
    );
    until(|| hooks.waiting[0].load(Ordering::SeqCst) == 4);
    assert_eq!(cursor.statistics().decoded_rows, 1024);
    assert_eq!(cursor.statistics().produced_rows, 0);
    cursor.cancel().unwrap();
    assert_eq!(cursor.statistics().joined_workers, 1);
}

#[test]
fn parallel_stop_between_predicate_check_and_wait_cannot_be_lost() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        hooks,
    );
    let shared = Arc::clone(&cursor.prepared.as_ref().unwrap().shared);
    let guard = shared.lock();
    assert!(!shared.signal.stop.load(Ordering::Acquire));
    // Reproduces the exact missed-notification window without sleeping: stop is
    // published after a predicate check but before registration with the CV.
    cursor.cancellation_handle().request_cancel();
    drop(shared.wait(guard));
    cursor.close().unwrap();
    assert_eq!(cursor.statistics().joined_workers, 2);
}

#[test]
fn parallel_partial_spawn_failure_joins_and_appends_secondary_diagnostics() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.fail_spawn.store(1, Ordering::SeqCst);
    hooks.panic_join.store(0, Ordering::SeqCst);
    let error = ParallelScanCursor::new_inner(
        &reader,
        &symbol,
        None,
        idx,
        reader.scan_options(),
        ParallelOptions::default(),
        Arc::clone(&hooks),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Io);
    assert_eq!(error.symbol_id, Some(0));
    let diagnostics = error.cleanup_error().unwrap().to_string();
    assert!(
        diagnostics.contains("initial cleanup denial")
            && diagnostics.contains("panicked while joining")
    );
    assert_eq!(hooks.started.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.exited.load(Ordering::SeqCst), 1);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_worker_panic_is_sticky_and_releases_shared_root() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    hooks.panic_step.store(0, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    hooks.hold_first.store(false, Ordering::SeqCst);
    cursor.signal.notify();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::WorkerFailure
    );
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::WorkerFailure
    );
    assert_eq!(cursor.close().err().unwrap().kind, ErrorKind::WorkerFailure);
    assert_eq!(cursor.statistics().joined_workers, 2);
    assert!(!cursor.statistics().complete);
    drop(cursor);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_exact_or_reduce_admission_is_explicit_and_charges_stacks() {
    let reader = Reader::open(
        "tests/fixtures/v7/key-widths.gdx",
        OpenOptions {
            memory_budget_bytes: 4 * 1024 * 1024,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 4096,
        },
    )
    .unwrap();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    assert_eq!(
        reader
            .scan_parallel(
                &symbol,
                idx,
                reader.scan_options(),
                ParallelOptions::default()
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::BudgetTooSmall
    );
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let mut cursor = reader
        .scan_parallel(
            &symbol,
            idx,
            reader.scan_options(),
            ParallelOptions {
                admission: ParallelAdmission::Reduce,
                ..ParallelOptions::default()
            },
        )
        .unwrap();
    let s = cursor.statistics();
    assert_eq!(s.requested_workers, 2);
    assert_eq!(s.admitted_workers, 1);
    assert_eq!(s.stack_reserved_bytes, 2 * 1024 * 1024 + GRANULE);
    assert!(s.control_reserved_bytes >= THREAD_CONTROL);
    cursor.cancel().unwrap();
    assert_eq!(cursor.statistics().joined_workers, 1);
}

#[test]
fn parallel_invalid_options_and_identity_do_not_spawn() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    for options in [
        ParallelOptions {
            workers: 0,
            ..ParallelOptions::default()
        },
        ParallelOptions {
            maximum_in_flight_tasks: 1,
            ..ParallelOptions::default()
        },
        ParallelOptions {
            worker_stack_bytes: usize::MAX,
            ..ParallelOptions::default()
        },
        ParallelOptions {
            intervals_per_task: 0,
            ..ParallelOptions::default()
        },
    ] {
        let (_temp, idx) = index(&reader, &symbol, 2048);
        assert_eq!(
            reader
                .scan_parallel(&symbol, idx, reader.scan_options(), options)
                .err()
                .unwrap()
                .kind,
            ErrorKind::InvalidOptions
        );
    }
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let other = super::tests::reader();
    let handle = other.symbol(SymbolId::UNIVERSE).unwrap();
    assert_eq!(
        other
            .scan_parallel(
                &handle,
                idx,
                other.scan_options(),
                ParallelOptions::default()
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
}

#[test]
fn parallel_cancel_handle_retains_only_small_signal_after_cursor_drop() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let cursor = reader
        .scan_parallel(
            &symbol,
            idx,
            reader.scan_options(),
            ParallelOptions::default(),
        )
        .unwrap();
    let signal = cursor.cancellation_handle();
    drop(cursor);
    assert_eq!(
        reader.budget_statistics().current_bytes,
        baseline + std::mem::size_of::<Signal>() + 64
    );
    signal.request_cancel();
    drop(signal);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_join_panic_on_completion_is_a_terminal_worker_failure() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 1);
    let hooks = Arc::new(Hooks::default());
    hooks.panic_join.store(0, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        hooks,
    );
    let error = loop {
        match cursor.next_batch() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("join failure lost"),
            Err(e) => break e,
        }
    };
    assert_eq!(error.kind, ErrorKind::WorkerFailure);
    assert!(std::error::Error::source(&error).is_some());
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.statistics().joined_workers, 2);
    assert_eq!(cursor.close().err().unwrap().kind, ErrorKind::WorkerFailure);
}

struct Data {
    bytes: Mutex<Vec<u8>>,
    changed: AtomicBool,
    fail_at: AtomicU64,
    block_at: AtomicU64,
    entered_read: AtomicBool,
    release_read: (Mutex<bool>, Condvar),
}
struct Backend(Arc<Data>);
impl crate::source::ReadAt for Backend {
    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
        if at >= self.0.block_at.load(Ordering::SeqCst) {
            let (lock, wake) = &self.0.release_read;
            let mut released = lock.lock().unwrap();
            self.0.entered_read.store(true, Ordering::SeqCst);
            while !*released {
                released = wake.wait(released).unwrap();
            }
        }
        if at >= self.0.fail_at.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected record read denial",
            ));
        }
        let bytes = self.0.bytes.lock().unwrap();
        let tail = bytes.get(at as usize..).unwrap_or(&[]);
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        Ok(n)
    }
    fn unchanged(&self) -> io::Result<bool> {
        Ok(!self.0.changed.load(Ordering::SeqCst))
    }
}
fn fault_reader(path: &str) -> (Reader, Arc<Data>) {
    let bytes = fs::read(path).unwrap();
    let length = bytes.len() as u64;
    let data = Arc::new(Data {
        bytes: Mutex::new(bytes),
        changed: AtomicBool::new(false),
        fail_at: AtomicU64::new(u64::MAX),
        block_at: AtomicU64::new(u64::MAX),
        entered_read: AtomicBool::new(false),
        release_read: (Mutex::new(false), Condvar::new()),
    });
    let source = crate::source::Source::testing(Box::new(Backend(Arc::clone(&data))), length);
    (Reader::from_source(source).unwrap(), data)
}

#[test]
fn parallel_checks_source_after_join_for_limit_and_full_completion() {
    for limit in [Some(1), None] {
        let (reader, data) = fault_reader("tests/fixtures/v7/semantics-0.gdx");
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let (_temp, idx) = index(&reader, &symbol, 2);
        let hooks = Arc::new(Hooks::default());
        *hooks.source_on_exit.lock().unwrap() = Some(data);
        let mut cursor = ParallelScanCursor::new_inner(
            &reader,
            &symbol,
            None,
            idx,
            ScanOptions {
                limit,
                ..reader.scan_options()
            },
            ParallelOptions::default(),
            hooks,
        )
        .unwrap();
        let mut acknowledged = 0;
        loop {
            match cursor.next_batch() {
                Ok(Some(batch)) => acknowledged += batch.len() as u64,
                Ok(None) => panic!("source change during joins lost"),
                Err(error) => {
                    assert_eq!(error.kind, ErrorKind::SourceChanged);
                    break;
                }
            }
        }
        assert_eq!(cursor.statistics().returned_rows, acknowledged);
        if limit.is_some() {
            assert_eq!(acknowledged, 0);
        }
        assert!(!cursor.statistics().complete);
        assert_eq!(cursor.statistics().joined_workers, 2);
        assert_eq!(cursor.close().err().unwrap().kind, ErrorKind::SourceChanged);
    }
}

#[cfg(feature = "compression")]
#[test]
fn parallel_cancel_during_source_read_joins_after_the_read_returns() {
    let (reader, data) = fault_reader("tests/fixtures/v7/blocks.gdx");
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let mut cursor = reader
        .scan_parallel(
            &symbol,
            idx,
            ScanOptions {
                batch_bytes: 80,
                ..reader.scan_options()
            },
            ParallelOptions {
                intervals_per_task: 1_000_000,
                ..ParallelOptions::default()
            },
        )
        .unwrap();
    assert!(cursor.next_batch().unwrap().is_some());
    data.block_at.store(25609, Ordering::SeqCst);
    let signal = cursor.cancellation_handle();
    let (send, receive) = std::sync::mpsc::channel();
    let consumer = thread::spawn(move || {
        while cursor.next_batch().unwrap().is_some() {}
        send.send(cursor.statistics()).unwrap();
        cursor.close().unwrap();
    });
    until(|| data.entered_read.load(Ordering::SeqCst));
    signal.request_cancel();
    // The controlled read cannot return before this thread releases it. A
    // cancellation request must not detach that worker or claim it has joined.
    assert!(matches!(
        receive.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    {
        let (lock, wake) = &data.release_read;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
    let stats = receive.recv_timeout(Duration::from_secs(15)).unwrap();
    consumer.join().unwrap();
    assert!(stats.cancelled && !stats.complete);
    assert_eq!(stats.joined_workers, 1);
    assert!(stats.returned_rows > 0);
}

#[test]
fn parallel_source_change_discards_unpublished_results_and_is_sticky() {
    let (reader, data) = fault_reader("tests/fixtures/v7/semantics-0.gdx");
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    data.changed.store(true, Ordering::SeqCst);
    hooks.hold_first.store(false, Ordering::SeqCst);
    cursor.signal.notify();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
    assert_eq!(cursor.statistics().returned_rows, 0);
    assert_eq!(cursor.statistics().joined_workers, 2);
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::SourceChanged
    );
}

#[test]
fn parallel_decode_failure_cannot_publish_a_later_partition() {
    let (reader, data) = fault_reader("tests/fixtures/v7/semantics-0.gdx");
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2);
    assert!(!symbol.inner.record.compressed);
    // Deliberately hidden same-length mutation isolates decoder failure after
    // initial strict source verification. Initial preparation reads only preamble.
    let at = symbol.inner.record.position as usize
        + 1
        + b"_DATA_".len()
        + 1
        + 4
        + 8 * usize::from(symbol.inner.record.dimension);
    assert_eq!(data.bytes.lock().unwrap()[at], 1);
    data.bytes.lock().unwrap()[at] = 0;
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    hooks.hold_first.store(false, Ordering::SeqCst);
    cursor.signal.notify();
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::InvalidKeyTag
    );
    assert_eq!(cursor.statistics().returned_rows, 0);
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.close().err().unwrap().kind, ErrorKind::InvalidKeyTag);
}

#[cfg(feature = "compression")]
#[test]
fn parallel_read_failure_preserves_the_acknowledged_prefix() {
    let (reader, data) = fault_reader("tests/fixtures/v7/blocks.gdx");
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let mut cursor = reader
        .scan_parallel(
            &symbol,
            idx,
            ScanOptions {
                batch_bytes: 80,
                ..reader.scan_options()
            },
            ParallelOptions {
                intervals_per_task: 1_000_000,
                ..ParallelOptions::default()
            },
        )
        .unwrap();
    let first = cursor.next_batch().unwrap().unwrap().len();
    let mut returned = first as u64;
    data.fail_at.store(25609, Ordering::SeqCst);
    let error = loop {
        match cursor.next_batch() {
            Ok(Some(batch)) => returned += batch.len() as u64,
            Ok(None) => panic!("record read fault was not reached"),
            Err(e) => break e,
        }
    };
    assert_eq!(error.kind, ErrorKind::Io);
    assert_eq!(error.symbol_id, Some(1));
    assert_eq!(cursor.statistics().returned_rows, returned);
    assert!(cursor.statistics().decoded_rows >= returned);
    assert_eq!(cursor.statistics().joined_workers, 1);
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.next_batch().err().unwrap().kind, ErrorKind::Io);
}

#[test]
fn parallel_drop_joins_a_slow_frontier_and_blocked_producer() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let baseline = reader.budget_statistics().current_bytes;
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        Arc::clone(&hooks),
    );
    until(|| {
        hooks.waiting[0].load(Ordering::SeqCst) == 2 && hooks.waiting[1].load(Ordering::SeqCst) == 3
    });
    drop(cursor);
    assert_eq!(hooks.started.load(Ordering::SeqCst), 2);
    assert_eq!(hooks.exited.load(Ordering::SeqCst), 2);
    assert_eq!(reader.budget_statistics().current_bytes, baseline);
}

#[test]
fn parallel_poisoned_shared_state_cancels_and_joins() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let (_temp, idx) = index(&reader, &symbol, 2048);
    let hooks = Arc::new(Hooks::default());
    hooks.hold_first.store(true, Ordering::SeqCst);
    let mut cursor = hooked(
        &reader,
        &symbol,
        None,
        idx,
        ParallelOptions::default(),
        hooks,
    );
    let shared = Arc::clone(&cursor.prepared.as_ref().unwrap().shared);
    assert!(
        thread::spawn(move || {
            let _guard = shared.state.lock().unwrap();
            panic!("injected state poison")
        })
        .join()
        .is_err()
    );
    assert_eq!(
        cursor.next_batch().err().unwrap().kind,
        ErrorKind::WorkerFailure
    );
    assert_eq!(cursor.statistics().joined_workers, 2);
    assert!(!cursor.statistics().complete);
}

#[test]
fn parallel_sidecar_failure_after_output_never_replays_serially() {
    let reader = reader();
    let symbol = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let (temp, idx) = index(&reader, &symbol, 2048);
    let mut cursor = reader
        .scan_parallel(
            &symbol,
            idx,
            ScanOptions {
                batch_bytes: 80,
                ..reader.scan_options()
            },
            ParallelOptions::default(),
        )
        .unwrap();
    let mut count = cursor.next_batch().unwrap().unwrap().len() as u64;
    fs::OpenOptions::new()
        .write(true)
        .open(temp.0.join("index"))
        .unwrap()
        .set_len(0)
        .unwrap();
    let error = loop {
        match cursor.next_batch() {
            Ok(Some(batch)) => {
                for key in batch.key_column(0).unwrap() {
                    count += 1;
                    assert_eq!(u64::from(key.get()), count);
                }
            }
            Ok(None) => panic!("sidecar corruption hidden"),
            Err(error) => break error,
        }
    };
    assert!(matches!(
        error.kind,
        ErrorKind::InvalidIndex | ErrorKind::Io | ErrorKind::UnexpectedEof
    ));
    assert_eq!(cursor.statistics().returned_rows, count);
    assert!(!cursor.statistics().complete);
    assert_eq!(cursor.statistics().joined_workers, 2);
    assert_eq!(cursor.next_batch().err().unwrap().kind, error.kind);
}
