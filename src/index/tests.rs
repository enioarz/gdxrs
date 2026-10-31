use super::*;
use store::{
    Event,
    tests::{Reset, deny, fault, short},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        fs::create_dir_all(".work").unwrap();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = PathBuf::from(".work").join(format!(
            "index-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn index(&self) -> PathBuf {
        self.0.join("index")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reader() -> Reader {
    Reader::open(
        "tests/fixtures/v7/semantics-0.gdx",
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
fn options() -> IndexOptions {
    IndexOptions {
        record_stride: 1,
        decoded_byte_stride: 17,
        ..IndexOptions::default()
    }
}
fn ready(open: IndexOpen) -> CheckpointIndex {
    open.index.expect("expected ready index")
}

#[test]
fn strict_replay_tree_proofs_and_directories_survive_reopen_without_collections() {
    let _reset = Reset::new();
    short();
    let temp = Temp::new();
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let base = reader.budget_statistics().current_bytes;
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    let built = builder.finish().unwrap();
    assert!(built.complete && built.published);
    assert_eq!(built.intervals, 5);
    assert_eq!(built.rows_decoded, 5);
    assert_eq!(builder.finish().unwrap().index_bytes, built.index_bytes);
    drop(builder);
    assert_eq!(reader.budget_statistics().current_bytes, base);
    let reopened = Reader::open(
        "tests/fixtures/v7/semantics-0.gdx",
        reader.source.options.clone(),
    )
    .unwrap();
    let symbol = reopened.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut index = ready(
        reopened
            .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
            .unwrap(),
    );
    assert_eq!(index.interval_count(), 5);
    assert_eq!(
        index.statistics().source_bytes_hashed,
        reopened.source.length
    );
    let before = index.statistics().source_bytes_hashed;
    for i in (0..5).rev() {
        let interval = index.interval(i).unwrap();
        assert_eq!((interval.start, interval.end), (i, i + 1));
        assert_eq!(interval.key_bounds(0).unwrap().0.get(), i as u32 + 1);
        let mut scan = reopened
            .scan_from_checkpoint(
                &symbol,
                &interval.checkpoint,
                ScanOptions {
                    limit: Some(1),
                    ..reopened.scan_options()
                },
            )
            .unwrap();
        assert_eq!(
            scan.next_batch().unwrap().unwrap().key_column(0).unwrap()[0].get(),
            i as u32 + 1
        );
    }
    assert_eq!(index.statistics().source_bytes_hashed, before);
    assert!(matches!(
        reopened
            .open_index(&symbol, temp.0.join("missing"), options(), IndexPolicy::Off)
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Off,
            ..
        }
    ));
    assert!(matches!(
        reopened
            .open_index(
                &symbol,
                temp.0.join("missing"),
                options(),
                IndexPolicy::UseIfValid
            )
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Missing,
            ..
        }
    ));
    assert!(reopened.budget_statistics().peak_bytes <= 1024 * 1024);
}
#[test]
fn corrupted_rechecksummed_pages_are_rejected_before_open_and_after_validation() {
    let temp = Temp::new();
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap()
        .finish()
        .unwrap();
    let mut session = ready(
        reader
            .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
            .unwrap(),
    );
    // Forge a data summary and recompute its digest, its directory and header.
    // No unkeyed checksum can authenticate the forged assertion. Source replay
    // rejects it at reopen; an existing session's trusted root rejects it too.
    let mut data = fs::read(temp.index()).unwrap();
    let pristine = data.clone();
    for length in [0, PAGE - 1, 2 * PAGE + 1, data.len() - 1] {
        fs::write(temp.index(), &data[..length]).unwrap();
        assert!(matches!(
            reader
                .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
                .unwrap(),
            IndexOpen {
                status: IndexStatus::Rejected(_),
                ..
            }
        ));
    }
    fs::write(temp.index(), &data).unwrap();
    let mut leaf = [0; PAGE];
    leaf.copy_from_slice(&data[PAGE..2 * PAGE]);
    put(&mut leaf, 48, &4u32.to_le_bytes());
    let leafhash = seal(&mut leaf);
    data[PAGE..2 * PAGE].copy_from_slice(&leaf);
    let mut header = [0; PAGE];
    header.copy_from_slice(&data[..PAGE]);
    let root = num(&header, 64) as usize;
    let mut directory = [0; PAGE];
    directory.copy_from_slice(&data[root..root + PAGE]);
    directory[56..88].copy_from_slice(&leafhash);
    let roothash = seal(&mut directory);
    data[root..root + PAGE].copy_from_slice(&directory);
    header[72..104].copy_from_slice(&roothash);
    seal(&mut header);
    data[..PAGE].copy_from_slice(&header);
    fs::write(temp.index(), &data).unwrap();
    assert_eq!(
        session.interval(0).err().unwrap().kind,
        ErrorKind::InvalidIndex
    );
    assert_eq!(
        session.interval(4).err().unwrap().kind,
        ErrorKind::InvalidIndex
    );
    assert!(matches!(
        reader
            .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Rejected(_),
            ..
        }
    ));
    // Every independently damaged small header/data/directory position fails.
    for at in [0, 8, 24, 56, 64, 104, 112, 640, PAGE + 200, root + 100] {
        let mut bad = pristine.clone();
        bad[at] ^= 1;
        fs::write(temp.index(), bad).unwrap();
        assert!(matches!(
            reader
                .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
                .unwrap(),
            IndexOpen {
                status: IndexStatus::Rejected(_),
                ..
            }
        ));
    }
}
#[test]
fn terminal_faults_preserve_cleanup_ownership_and_never_publish_partial_indexes() {
    let _reset = Reset::new();
    for (event, after) in [
        (Event::CreateDirectory, 0),
        (Event::Create, 0),
        (Event::Write, 0),
        (Event::Write, 2),
        (Event::Seek, 0),
        (Event::Sync, 0),
        (Event::Publish, 0),
    ] {
        let temp = Temp::new();
        let reader = reader();
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let mut builder = reader
            .index_builder(&symbol, temp.index(), options())
            .unwrap();
        fault(event, after, false);
        deny(true);
        let error = builder.finish().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Io);
        assert!(!temp.index().exists());
        if event == Event::Write {
            let cause = std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap();
            assert_eq!(cause.kind(), io::ErrorKind::StorageFull);
        }
        if event != Event::CreateDirectory {
            assert!(error.cleanup_error().is_some());
            assert!(builder.owned_directory);
        }
        assert_eq!(builder.finish().unwrap_err().kind, error.kind);
        deny(false);
        builder.abort().unwrap();
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    }
}
#[test]
fn cancellation_quota_existing_files_and_postpublication_cleanup_failures_are_explicit() {
    let _reset = Reset::new();
    let temp = Temp::new();
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut builder = reader
        .index_builder(
            &symbol,
            temp.index(),
            IndexOptions {
                maximum_index_bytes: 2048,
                ..options()
            },
        )
        .unwrap();
    assert_eq!(
        builder.finish().unwrap_err().kind,
        ErrorKind::TemporarySpaceExceeded
    );
    assert!(!temp.index().exists());
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    assert_eq!(
        builder
            .finish_with(|_| Err(Error::new(ErrorKind::InvalidOptions, "cancel")))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidOptions
    );
    assert!(!temp.index().exists());
    fs::write(temp.index(), b"foreign").unwrap();
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    assert!(builder.finish().is_err());
    assert_eq!(fs::read(temp.index()).unwrap(), b"foreign");
    let mut builder = reader
        .index_builder(
            &symbol,
            temp.index(),
            IndexOptions {
                publication: IndexPublication::Replace,
                ..options()
            },
        )
        .unwrap();
    assert!(builder.finish().is_err());
    assert_eq!(fs::read(temp.index()).unwrap(), b"foreign");
    fs::remove_file(temp.index()).unwrap();
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    deny(true);
    let error = builder.finish().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Io);
    assert!(builder.statistics().published);
    assert!(!builder.statistics().complete);
    assert!(temp.index().exists());
    deny(false);
    builder.abort().unwrap();
    assert!(matches!(
        reader
            .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Ready,
            ..
        }
    ));
    let mut replace = reader
        .index_builder(
            &symbol,
            temp.index(),
            IndexOptions {
                publication: IndexPublication::Replace,
                ..options()
            },
        )
        .unwrap();
    assert!(replace.finish().unwrap().published);
}
#[test]
fn source_identity_is_full_content_and_callbacks_cannot_publish_changed_sources() {
    let _reset = Reset::new();
    let temp = Temp::new();
    let source = temp.0.join("source.gdx");
    fs::copy("tests/fixtures/v7/semantics-0.gdx", &source).unwrap();
    let reader = Reader::open(&source, OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    builder.finish().unwrap();
    let mut data = fs::read(&source).unwrap();
    let last = data.len() - 1;
    data[last] ^= 1;
    fs::write(&source, &data).unwrap();
    let reopened = Reader::open(&source, OpenOptions::default()).unwrap();
    let symbol = reopened.symbol(SymbolId::new(1).unwrap()).unwrap();
    assert!(matches!(
        reopened
            .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Rejected(_),
            ..
        }
    ));
    let mut builder = reopened
        .index_builder(&symbol, temp.0.join("changed"), options())
        .unwrap();
    let error = builder
        .finish_with(|_| {
            fs::OpenOptions::new()
                .append(true)
                .open(&source)
                .unwrap()
                .write_all(b"change")
                .unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::SourceChanged);
    assert!(!temp.0.join("changed").exists());
}
#[test]
fn directory_levels_are_disk_paged_and_reuse_never_accepts_another_symbol() {
    let _reset = Reset::new();
    let temp = Temp::new();
    let reader = reader();
    let universe = reader.symbol(SymbolId::UNIVERSE).unwrap();
    let opts = IndexOptions {
        record_stride: 2,
        ..options()
    };
    reader
        .index_builder(&universe, temp.index(), opts)
        .unwrap()
        .finish()
        .unwrap();
    let wrong = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    assert!(matches!(
        reader
            .open_index(&wrong, temp.index(), opts, IndexPolicy::UseIfValid)
            .unwrap(),
        IndexOpen {
            status: IndexStatus::Rejected(_),
            ..
        }
    ));
    fault(Event::Read, 0, false);
    assert!(
        reader
            .open_index(&universe, temp.index(), opts, IndexPolicy::UseIfValid)
            .is_err()
    );
    assert_eq!(
        hash(b"abc"),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad
        ]
    );
    assert!(std::mem::size_of::<Sha256>() <= 256);
}

#[test]
fn replacement_cannot_target_gdx_and_failed_digest_allocation_retains_cleanup() {
    let _reset = Reset::new();
    let temp = Temp::new();
    let source = temp.0.join("source.gdx");
    fs::copy("tests/fixtures/v7/semantics-0.gdx", &source).unwrap();
    let reader = Reader::open(&source, OpenOptions::default()).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let original = fs::read(&source).unwrap();
    let mut builder = reader
        .index_builder(
            &symbol,
            &source,
            IndexOptions {
                publication: IndexPublication::Replace,
                ..options()
            },
        )
        .unwrap();
    assert_eq!(
        builder.finish().unwrap_err().kind,
        ErrorKind::InvalidOptions
    );
    assert_eq!(fs::read(&source).unwrap(), original);
    drop(builder);
    let mut builder = reader
        .index_builder(&symbol, temp.index(), options())
        .unwrap();
    let stats = reader.budget_statistics();
    let block = reader
        .source
        .budget
        .reserve(stats.limit_bytes - stats.current_bytes - 1024)
        .unwrap();
    deny(true);
    let error = builder.finish().unwrap_err();
    assert_eq!(error.kind, ErrorKind::BudgetTooSmall);
    assert!(error.cleanup_error().is_some());
    assert!(builder.owned_directory);
    drop(block);
    deny(false);
    builder.abort().unwrap();
    assert!(!temp.index().exists());
}
