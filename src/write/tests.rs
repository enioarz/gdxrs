use super::*;
use std::cell::RefCell;
#[derive(Default)]
struct Hooks {
    fault: Option<(Event, usize, bool, io::ErrorKind)>,
    chunk: usize,
    sequence: Option<u64>,
    deny_cleanup: bool,
    zero_write: bool,
}
thread_local! {static HOOKS:RefCell<Hooks>=RefCell::new(Hooks::default());}
pub(super) fn event(kind: Event) -> io::Result<()> {
    HOOKS.with(|h| {
        let mut h = h.borrow_mut();
        if kind == Event::Remove && h.deny_cleanup {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "writer cleanup denied",
            ));
        }
        if let Some((event, after, persistent, error)) = h.fault
            && event == kind
        {
            if after == 0 {
                if !persistent {
                    h.fault = None;
                }
                return Err(io::Error::new(error, "injected writer fault"));
            }
            h.fault = Some((event, after - 1, persistent, error));
        }
        Ok(())
    })
}
pub(super) fn io_chunk(length: usize) -> usize {
    HOOKS.with(|h| {
        if h.borrow().zero_write {
            return 0;
        }
        let n = h.borrow().chunk;
        if n == 0 { length } else { length.min(n) }
    })
}
pub(super) fn sequence_override() -> Option<u64> {
    HOOKS.with(|h| h.borrow_mut().sequence.take())
}

pub(super) struct Reset;
impl Reset {
    pub(super) fn new() -> Self {
        HOOKS.with(|h| *h.borrow_mut() = Hooks::default());
        Self
    }
}
impl Drop for Reset {
    fn drop(&mut self) {
        HOOKS.with(|h| *h.borrow_mut() = Hooks::default());
    }
}
pub(super) fn fault(event: Event, after: usize, persistent: bool, error: io::ErrorKind) {
    HOOKS.with(|h| h.borrow_mut().fault = Some((event, after, persistent, error)));
}
pub(super) struct Parent(PathBuf);
impl Parent {
    pub(super) fn new() -> Self {
        fs::create_dir_all(".work").unwrap();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(".work").join(format!(
            "writer-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub(super) fn destination(&self) -> PathBuf {
        self.0.join("output.gdx")
    }
    pub(super) fn empty(&self) {
        assert_eq!(fs::read_dir(&self.0).unwrap().count(), 0);
    }
}
impl Drop for Parent {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn options() -> WriteOptions {
    WriteOptions {
        memory_budget_bytes: 1024 * 1024,
        ..WriteOptions::default()
    }
}
fn io_kind(error: &Error) -> io::ErrorKind {
    use std::error::Error;
    error
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap()
        .kind()
}
fn verify(path: &Path) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len(), 197);
    assert_eq!(
        &bytes[..34],
        &[
            2, 0x34, 0x12, 4, 0x78, 0x56, 0x34, 0x12, 8, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 9,
            0x40, 123, 7, b'G', b'A', b'M', b'S', b'G', b'D', b'X', 7, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(&bytes[34..46], b"\x05gdxrs\x05gdxrs");
    assert_eq!(&bytes[46..50], &19510624i32.to_le_bytes());
    for (n, position) in [126u64, 163, 144, 179, 126, 0].iter().enumerate() {
        assert_eq!(&bytes[50 + n * 8..58 + n * 8], &position.to_le_bytes());
    }
    assert_eq!(&bytes[98..126], &[0; 28]);
    assert_eq!(&bytes[126..144], b"\x06_SYMB_\0\0\0\0\x06_SYMB_");
    assert_eq!(&bytes[144..163], b"\x06_SETT_\x01\0\0\0\0\x06_SETT_");
    assert_eq!(&bytes[163..179], b"\x05_UEL_\0\0\0\0\x05_UEL_");
    assert_eq!(&bytes[179..], b"\x06_ACRO_\0\0\0\0\x06_ACRO_");
    let r = Reader::open(path, OpenOptions::default()).unwrap();
    let info = r.information().unwrap();
    assert_eq!(
        (
            info.version,
            info.compression_level,
            info.symbol_count,
            info.uel_count
        ),
        (7, 0, 0, 0)
    );
    assert_eq!(info.audit, b"gdxrs");
    assert_eq!(info.producer, b"gdxrs");
    assert!(r.symbols().unwrap().next().unwrap().is_none());
    assert!(r.uels().unwrap().next().unwrap().is_none());
    assert!(r.acronyms().unwrap().next().unwrap().is_none());
    let mut text = r.set_texts().unwrap();
    let item = text.next().unwrap().unwrap();
    assert_eq!((item.id, item.text), (0, b"".as_slice()));
    assert!(text.next().unwrap().is_none());
    let symbol = r.symbol(SymbolId::UNIVERSE).unwrap();
    assert!(
        r.scan(&symbol, r.scan_options())
            .unwrap()
            .next_batch()
            .unwrap()
            .is_none()
    );
}
#[test]
fn fixed_v7_layout_short_interrupted_writes_and_idempotent_finish() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut opts = options();
    opts.maximum_output_bytes = 197;
    let mut w = Writer::create(p.destination(), opts).unwrap();
    let budget = Arc::clone(&w.budget);
    let temporary = w.temporary_path().to_path_buf();
    assert!(!p.destination().exists());
    assert!(temporary.exists());
    assert_eq!(w.statistics().output_bytes, 126);
    assert_eq!(w.statistics().bytes_written, 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&temporary).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    HOOKS.with(|h| h.borrow_mut().chunk = 3);
    fault(Event::Write, 4, false, io::ErrorKind::Interrupted);
    let stats = w.finish().unwrap();
    assert_eq!(
        stats,
        WriteStatistics {
            status: WriteStatus::Complete,
            output_bytes: 197,
            bytes_written: 249,
            published: true
        }
    );
    assert_eq!(w.finish().unwrap(), stats);
    assert_eq!(w.abort().unwrap(), stats);
    assert!(!temporary.exists());
    verify(&p.destination());
    assert!(budget.statistics().peak_bytes <= 1024 * 1024);
    drop(w);
    assert_eq!(budget.statistics().current_bytes, 0);
    fs::remove_file(p.destination()).unwrap();
    p.empty();
}
#[test]
fn create_options_paths_allocations_and_shared_root_are_admitted_before_files() {
    let _reset = Reset::new();
    let p = Parent::new();
    for (opts, kind) in [
        (
            WriteOptions {
                memory_budget_bytes: 1,
                ..options()
            },
            ErrorKind::BudgetTooSmall,
        ),
        (
            WriteOptions {
                maximum_output_bytes: 196,
                ..options()
            },
            ErrorKind::OutputLimitExceeded,
        ),
        (
            WriteOptions {
                maximum_output_bytes: u64::MAX,
                ..options()
            },
            ErrorKind::InvalidOptions,
        ),
    ] {
        assert_eq!(
            Writer::create(p.destination(), opts).err().unwrap().kind,
            kind
        );
        p.empty();
    }
    assert_eq!(
        Writer::create(p.0.join("missing/out.gdx"), options())
            .err()
            .map(|e| io_kind(&e)),
        Some(io::ErrorKind::NotFound)
    );
    assert_eq!(
        Writer::create(p.0.join("x".repeat(5000)), options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
    p.empty();
    let budget = Budget::new(1024 * 1024);
    crate::budget::inject_buffer_failure();
    assert_eq!(
        Writer::with_budget(&p.destination(), &options(), Arc::clone(&budget))
            .err()
            .unwrap()
            .kind,
        ErrorKind::AllocationFailed
    );
    assert_eq!(budget.statistics().current_bytes, 0);
    p.empty();
    let r = Reader::open(
        "tests/fixtures/v7/optional-domains.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let root = Arc::clone(&r.source.budget);
    let baseline = root.statistics().current_bytes;
    assert_eq!(
        r.create_writer(p.destination(), WriteOptions::default())
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
    let reservation = root.reserve(1024 * 1024 - baseline - 1024).unwrap();
    assert_eq!(
        r.create_writer(p.destination(), r.write_options())
            .err()
            .unwrap()
            .kind,
        ErrorKind::BudgetTooSmall
    );
    drop(reservation);
    assert_eq!(root.statistics().current_bytes, baseline);
    p.empty();
    let mut writer = r.create_writer(p.destination(), r.write_options()).unwrap();
    assert!(root.statistics().current_bytes > baseline);
    drop(r);
    writer.finish().unwrap();
    drop(writer);
    assert_eq!(root.statistics().current_bytes, 0);
    fs::remove_file(p.destination()).unwrap();
    p.empty();
}
#[test]
fn existing_destinations_late_collisions_and_foreign_temporary_names_are_preserved() {
    let _reset = Reset::new();
    let p = Parent::new();
    fs::write(p.destination(), b"keep").unwrap();
    let e = Writer::create(p.destination(), options()).err().unwrap();
    assert_eq!(io_kind(&e), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(p.destination()).unwrap(), b"keep");
    fs::remove_file(p.destination()).unwrap();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let temp = w.temporary_path().to_path_buf();
    fs::write(p.destination(), b"late owner").unwrap();
    let e = w.finish().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::AlreadyExists);
    assert!(!w.statistics().published);
    assert!(!temp.exists());
    assert_eq!(fs::read(p.destination()).unwrap(), b"late owner");
    w.abort().unwrap();
    drop(w);
    fs::remove_file(p.destination()).unwrap();
    let foreign = p.0.join(format!(
        ".gdxrs-write-{:08x}-{:016x}.part",
        std::process::id(),
        42
    ));
    fs::write(&foreign, b"foreign").unwrap();
    HOOKS.with(|h| h.borrow_mut().sequence = Some(42));
    let mut w = Writer::create(p.destination(), options()).unwrap();
    assert_ne!(
        fs::canonicalize(w.temporary_path()).unwrap(),
        fs::canonicalize(&foreign).unwrap()
    );
    w.abort().unwrap();
    drop(w);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
    fs::remove_file(foreign).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = p.0.join("missing-target");
        symlink(&target, p.destination()).unwrap();
        let e = Writer::create(p.destination(), options()).err().unwrap();
        assert_eq!(io_kind(&e), io::ErrorKind::AlreadyExists);
        assert!(
            fs::symlink_metadata(p.destination())
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(p.destination()).unwrap();
    }
    p.empty();
}
#[test]
fn every_output_stage_failure_is_terminal_and_prepublication_cleanup_is_exact() {
    let _reset = Reset::new();
    let p = Parent::new();
    fault(Event::Create, 0, false, io::ErrorKind::PermissionDenied);
    let e = Writer::create(p.destination(), options()).err().unwrap();
    assert_eq!(io_kind(&e), io::ErrorKind::PermissionDenied);
    p.empty();
    for event in [Event::Write, Event::Seek, Event::Sync, Event::Publish] {
        let mut w = Writer::create(p.destination(), options()).unwrap();
        let temp = w.temporary_path().to_path_buf();
        fault(event, 0, false, io::ErrorKind::StorageFull);
        let e = w.finish().unwrap_err();
        assert_eq!(e.kind, ErrorKind::Io);
        assert_eq!(io_kind(&e), io::ErrorKind::StorageFull);
        assert!(!w.statistics().published);
        assert_eq!(w.statistics().status, WriteStatus::Failed);
        assert!(!p.destination().exists());
        assert!(!temp.exists());
        assert_eq!(w.finish().unwrap_err().physical_offset, e.physical_offset);
        w.abort().unwrap();
        drop(w);
        p.empty();
    }
    let mut w = Writer::create(p.destination(), options()).unwrap();
    HOOKS.with(|h| {
        let mut h = h.borrow_mut();
        h.chunk = 5;
    });
    fault(Event::Write, 1, false, io::ErrorKind::StorageFull);
    let e = w.finish().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::StorageFull);
    assert_eq!(w.statistics().bytes_written, 5);
    assert_eq!(e.physical_offset, Some(5));
    drop(w);
    p.empty();
    HOOKS.with(|h| {
        let mut h = h.borrow_mut();
        h.chunk = 0;
        h.zero_write = true;
    });
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let e = w.finish().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::WriteZero);
    drop(w);
    p.empty();
}
#[test]
fn primary_and_secondary_failures_remain_visible_and_abort_retries_owned_cleanup() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let temp = w.temporary_path().to_path_buf();
    fault(Event::Write, 0, false, io::ErrorKind::StorageFull);
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = true);
    let e = w.finish().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::StorageFull);
    assert_eq!(
        e.cleanup_error().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(temp.exists());
    assert!(!p.destination().exists());
    assert!(w.abort().is_err());
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = false);
    w.abort().unwrap();
    assert!(!temp.exists());
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::Io);
    drop(w);
    p.empty();
}
#[test]
fn postpublication_cleanup_failure_never_deletes_valid_destination_or_claims_complete() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let temp = w.temporary_path().to_path_buf();
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = true);
    let e = w.finish().unwrap_err();
    assert_eq!(io_kind(&e), io::ErrorKind::PermissionDenied);
    assert!(w.statistics().published);
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    assert!(temp.exists());
    verify(&p.destination());
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = false);
    w.abort().unwrap();
    assert!(!temp.exists());
    assert!(p.destination().exists());
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    drop(w);
    verify(&p.destination());
    fs::remove_file(p.destination()).unwrap();
    p.empty();
}
#[test]
fn cancellation_drop_and_buffer_quota_never_publish() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let budget = Arc::clone(&w.budget);
    let temp = w.temporary_path().to_path_buf();
    assert_eq!(w.abort().unwrap().status, WriteStatus::Aborted);
    assert!(!temp.exists());
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidWriteState);
    drop(w);
    assert_eq!(budget.statistics().current_bytes, 0);
    p.empty();
    {
        let w = Writer::create(p.destination(), options()).unwrap();
        assert!(w.temporary_path().exists());
    }
    p.empty();
    let mut w = Writer::create(
        p.destination(),
        WriteOptions {
            maximum_output_bytes: 40_000,
            ..options()
        },
    )
    .unwrap();
    let before = w.statistics();
    let e = w.bytes(&[0; 40_000]).unwrap_err();
    assert_eq!(e.kind, ErrorKind::OutputLimitExceeded);
    assert_eq!(w.statistics(), before);
    w.fail(e);
    drop(w);
    p.empty();
    let mut w = Writer::create(
        p.destination(),
        WriteOptions {
            maximum_output_bytes: 40_000,
            ..options()
        },
    )
    .unwrap();
    w.bytes(&[0; 35_000]).unwrap();
    assert_eq!(w.statistics().output_bytes, 35_126);
    assert_eq!(w.statistics().bytes_written, 32768);
    w.abort().unwrap();
    drop(w);
    p.empty();
}

#[test]
fn explicit_abort_cleanup_failure_is_retryable_and_existing_directories_survive() {
    let _reset = Reset::new();
    let p = Parent::new();
    fs::create_dir(p.destination()).unwrap();
    let e = Writer::create(p.destination(), options()).err().unwrap();
    assert_eq!(io_kind(&e), io::ErrorKind::AlreadyExists);
    assert!(p.destination().is_dir());
    fs::remove_dir(p.destination()).unwrap();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let temp = w.temporary_path().to_path_buf();
    assert_eq!(fs::metadata(&temp).unwrap().len(), 0);
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = true);
    assert!(w.abort().is_err());
    assert_eq!(w.statistics().status, WriteStatus::Aborted);
    assert!(temp.exists());
    assert!(!p.destination().exists());
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidWriteState);
    HOOKS.with(|h| h.borrow_mut().deny_cleanup = false);
    w.abort().unwrap();
    drop(w);
    p.empty();
}

pub(super) fn short_chunks(n: usize) {
    HOOKS.with(|h| h.borrow_mut().chunk = n);
}
