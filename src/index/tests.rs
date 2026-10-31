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

fn query_reader() -> Reader {
    Reader::open(
        if cfg!(feature = "compression") {
            "tests/fixtures/v7/blocks.gdx"
        } else {
            "tests/fixtures/v7/key-widths.gdx"
        },
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            metadata_cache_bytes: 0,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
fn query_index(reader: &Reader, symbol: &SymbolHandle, path: &Path) -> IndexOpen {
    let options = IndexOptions {
        record_stride: 8,
        decoded_byte_stride: u64::MAX,
        ..IndexOptions::default()
    };
    reader
        .index_builder(symbol, path, options)
        .unwrap()
        .finish()
        .unwrap();
    reader
        .open_index(symbol, path, options, IndexPolicy::UseIfValid)
        .unwrap()
}
fn query_keys(scan: &mut ScanCursor) -> Vec<u32> {
    let mut keys = Vec::new();
    while let Some(batch) = scan.next_batch().unwrap() {
        keys.extend(batch.key_column(0).unwrap().iter().map(|v| v.get()));
    }
    keys
}
#[test]
fn indexed_query_prunes_reuses_and_keeps_zero_limit_lazy() {
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(4000).unwrap()),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![4000]);
    let stats = scan.indexed_statistics().unwrap();
    assert!(stats.intervals_pruned > 400);
    assert_eq!(stats.checkpoint_restarts, 1);
    assert!(scan.statistics().decoded_rows <= 8);
    assert_eq!(stats.validation.source_bytes_hashed, reader.source.length);
    assert!(stats.index_pages_read > 0);
    if cfg!(feature = "compression") {
        assert!(stats.source_io.read_calls > 0);
    }
    let index = scan.into_index().unwrap();
    assert!(index.statistics().pages_read > index.validation_statistics().pages_read);
    let validation_pages = index.validation_statistics().pages_read;
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(0),
                ..reader.scan_options()
            },
            IndexOpen {
                status: IndexStatus::Ready,
                index: Some(index),
            },
        )
        .unwrap();
    let io = reader.io_statistics();
    assert!(scan.next_batch().unwrap().is_none());
    assert_eq!(reader.io_statistics().read_calls, io.read_calls);
    assert_eq!(scan.indexed_statistics().unwrap().intervals_considered, 0);
    assert_eq!(
        scan.indexed_statistics().unwrap().validation.pages_read,
        validation_pages
    );
    assert_eq!(scan.indexed_statistics().unwrap().index_pages_read, 0);
    assert_eq!(scan.statistics().decoded_rows, 0);
}
#[test]
fn indexed_integrity_fallback_rolls_back_unacknowledged_rows_and_global_limit() {
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let ids = [UelId::new(10).unwrap(), UelId::new(4000).unwrap()];
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::AnyOf(&ids),
            }],
        )
        .unwrap();
    // Fourth leaf follows three data pages, before the first directory flush.
    // It is reached after row 10 was accepted into the still-unpublished fill.
    let mut bytes = fs::read(temp.index()).unwrap();
    bytes[4 * PAGE + 48] ^= 1;
    fs::write(temp.index(), bytes).unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(2),
                batch_bytes: 4096,
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![10, 4000]);
    assert_eq!(scan.statistics().returned_rows, 2);
    assert!(scan.statistics().decoded_rows > 4000);
    assert_eq!(
        scan.indexed_statistics().unwrap().mode,
        IndexScanMode::Rejected
    );
    assert_eq!(
        scan.index_rejection().unwrap().kind,
        ErrorKind::InvalidIndex
    );
}
#[test]
fn indexed_integrity_after_output_is_terminal_and_progress_is_not_replayed() {
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let ids = [UelId::new(10).unwrap(), UelId::new(4000).unwrap()];
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::AnyOf(&ids),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                batch_bytes: std::mem::size_of::<UelId>() + std::mem::size_of::<Value>(),
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    assert_eq!(
        scan.next_batch()
            .unwrap()
            .unwrap()
            .row(0)
            .unwrap()
            .key(0)
            .unwrap()
            .get(),
        10
    );
    fs::OpenOptions::new()
        .write(true)
        .open(temp.index())
        .unwrap()
        .set_len(PAGE as u64)
        .unwrap();
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidIndex
    );
    let statistics = scan.statistics();
    assert_eq!(statistics.returned_rows, 1);
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidIndex
    );
    assert_eq!(scan.statistics(), statistics);
    assert!(scan.index_rejection().is_none());
}
#[test]
fn indexed_access_and_budget_errors_never_become_cache_misses() {
    let _reset = Reset::new();
    for budget_denial in [false, true] {
        let temp = Temp::new();
        let reader = query_reader();
        let symbol = reader
            .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
            .unwrap();
        let open = query_index(&reader, &symbol, &temp.index());
        let selection = reader
            .selection(
                &symbol,
                &[DimensionSelection {
                    dimension: 0,
                    keys: KeySelection::One(UelId::new(4000).unwrap()),
                }],
            )
            .unwrap();
        let mut scan = reader
            .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
            .unwrap();
        let held = if budget_denial {
            Some(
                reader
                    .source
                    .budget
                    .reserve(1024 * 1024 - reader.budget_statistics().current_bytes)
                    .unwrap(),
            )
        } else {
            fault(Event::Read, 0, false);
            None
        };
        let kind = if budget_denial {
            ErrorKind::BudgetTooSmall
        } else {
            ErrorKind::Io
        };
        assert_eq!(scan.next_batch().err().unwrap().kind, kind);
        assert_eq!(scan.statistics().returned_rows, 0);
        assert!(scan.index_rejection().is_none());
        assert_eq!(scan.next_batch().err().unwrap().kind, kind);
        drop(held);
    }
}

#[test]
fn indexed_poor_pruning_coalesces_and_short_limits_do_not_prescan() {
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Range(
                    UelRange::new(UelId::new(1).unwrap(), UelId::new(4000).unwrap()).unwrap(),
                ),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![1]);
    assert_eq!(scan.indexed_statistics().unwrap().intervals_considered, 1);
    let open = IndexOpen {
        status: IndexStatus::Ready,
        index: scan.into_index(),
    };
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(query_keys(&mut scan), (1..=4000).collect::<Vec<_>>());
    let stats = scan.indexed_statistics().unwrap();
    assert_eq!(stats.mode, IndexScanMode::Sequential);
    assert_eq!(stats.intervals_considered - stats.boundary_probes, 8);
    assert!(stats.boundary_probes < 40);
    assert_eq!(scan.statistics().decoded_rows, 4000);
    assert_eq!(stats.adjacent_intervals_coalesced, 7);
    assert_eq!(stats.checkpoint_restarts, 1);
}
#[test]
fn indexed_fallback_policies_identity_and_configured_batch_cap_are_explicit() {
    let temp = Temp::new();
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let selection = reader.selection(&symbol, &[]).unwrap();
    for (policy, mode) in [
        (IndexPolicy::Off, IndexScanMode::Off),
        (IndexPolicy::UseIfValid, IndexScanMode::Missing),
    ] {
        let open = reader
            .open_index(&symbol, temp.index(), options(), policy)
            .unwrap();
        let mut scan = reader
            .scan_selected_indexed(
                &symbol,
                &selection,
                ScanOptions {
                    batch_bytes: 64,
                    ..reader.scan_options()
                },
                open,
            )
            .unwrap();
        assert_eq!(query_keys(&mut scan), (1..=5).collect::<Vec<_>>());
        assert_eq!(scan.indexed_statistics().unwrap().mode, mode);
    }
    fs::write(temp.index(), b"corrupt").unwrap();
    let open = reader
        .open_index(&symbol, temp.index(), options(), IndexPolicy::UseIfValid)
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(query_keys(&mut scan), (1..=5).collect::<Vec<_>>());
    assert_eq!(
        scan.index_rejection().unwrap().kind,
        ErrorKind::InvalidIndex
    );
    fs::remove_file(temp.index()).unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let other = reader.symbol(SymbolId::new(2).unwrap()).unwrap();
    let other_selection = reader.selection(&other, &[]).unwrap();
    assert_eq!(
        reader
            .scan_selected_indexed(&other, &other_selection, reader.scan_options(), open)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    let open = reader
        .open_index(
            &symbol,
            temp.index(),
            IndexOptions {
                record_stride: 8,
                decoded_byte_stride: u64::MAX,
                ..IndexOptions::default()
            },
            IndexPolicy::UseIfValid,
        )
        .unwrap();
    let other = super::tests::reader();
    let other_symbol = other.symbol(SymbolId::new(1).unwrap()).unwrap();
    let other_selection = other.selection(&other_symbol, &[]).unwrap();
    assert_eq!(
        other
            .scan_selected_indexed(&other_symbol, &other_selection, other.scan_options(), open)
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert_eq!(
        reader
            .scan_selected_indexed(
                &symbol,
                &selection,
                reader.scan_options(),
                IndexOpen {
                    status: IndexStatus::Ready,
                    index: None
                }
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::InvalidOptions
    );
}
#[test]
fn indexed_source_changes_before_and_after_output_preserve_acknowledged_work() {
    for before in [true, false] {
        let temp = Temp::new();
        let source = temp.0.join("source.gdx");
        fs::copy("tests/fixtures/v7/semantics-0.gdx", &source).unwrap();
        let reader = Reader::open(&source, OpenOptions::default()).unwrap();
        let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
        let open = query_index(&reader, &symbol, &temp.index());
        let selection = reader
            .selection(
                &symbol,
                &[DimensionSelection {
                    dimension: 0,
                    keys: KeySelection::Range(
                        UelRange::new(UelId::new(1).unwrap(), UelId::new(5).unwrap()).unwrap(),
                    ),
                }],
            )
            .unwrap();
        let mut scan = reader
            .scan_selected_indexed(
                &symbol,
                &selection,
                ScanOptions {
                    batch_bytes: std::mem::size_of::<UelId>() + std::mem::size_of::<Value>(),
                    ..reader.scan_options()
                },
                open,
            )
            .unwrap();
        if !before {
            assert_eq!(scan.next_batch().unwrap().unwrap().len(), 1);
        }
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert_eq!(
            scan.next_batch().err().unwrap().kind,
            ErrorKind::SourceChanged
        );
        assert_eq!(scan.statistics().returned_rows, u64::from(!before));
        assert_eq!(
            scan.next_batch().err().unwrap().kind,
            ErrorKind::SourceChanged
        );
    }
}
#[test]
fn indexed_last_candidate_still_validates_pending_terminator() {
    use crate::source::{ReadAt, Source};
    use std::sync::Mutex;
    struct Mutable(Arc<Mutex<Vec<u8>>>);
    impl ReadAt for Mutable {
        fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
            let data = self.0.lock().unwrap();
            let rest = data.get(at as usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len());
            out[..n].copy_from_slice(&rest[..n]);
            Ok(n)
        }
        // Deliberately disable immutable-source detection to exercise decoder
        // framing independently of the source-change gate above.
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    let temp = Temp::new();
    let data = Arc::new(Mutex::new(
        fs::read("tests/fixtures/v7/semantics-0.gdx").unwrap(),
    ));
    let len = data.lock().unwrap().len() as u64;
    let reader =
        Reader::from_source(Source::testing(Box::new(Mutable(Arc::clone(&data))), len)).unwrap();
    let symbol = reader.symbol(SymbolId::new(1).unwrap()).unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let mut decoder = Decoder::new(&reader, &symbol.inner.record).unwrap();
    for _ in 0..5 {
        decoder.next().unwrap().unwrap();
    }
    // Version-1 raw checkpoint physical offset, after the fixed tuple/state.
    let encoded = decoder.position().unwrap().index_bytes().unwrap();
    let physical = u64::from_le_bytes(encoded[387..395].try_into().unwrap()) as usize;
    assert_eq!(data.lock().unwrap()[physical], 255);
    data.lock().unwrap()[physical] = 1;
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Range(
                    UelRange::new(UelId::new(1).unwrap(), UelId::new(5).unwrap()).unwrap(),
                ),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                batch_bytes: std::mem::size_of::<UelId>() + std::mem::size_of::<Value>(),
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    for key in 1..=5 {
        assert_eq!(
            scan.next_batch()
                .unwrap()
                .unwrap()
                .row(0)
                .unwrap()
                .key(0)
                .unwrap()
                .get(),
            key
        );
    }
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
    assert_eq!(scan.statistics().returned_rows, 5);
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
}

#[test]
fn ordered_leading_bounds_seek_late_without_directory_prescan_and_keep_gaps_exact() {
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let target = if cfg!(feature = "compression") {
        4800
    } else {
        60000
    };
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(target).unwrap()),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(1),
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![target]);
    let statistics = scan.indexed_statistics().unwrap();
    assert!(statistics.intervals_considered < 40);
    assert!(statistics.intervals_skipped_by_leading_bounds > 500);
    assert!(statistics.index_pages_read < 300);
    assert!(scan.statistics().decoded_rows <= 8);
    let open = IndexOpen {
        status: IndexStatus::Ready,
        index: scan.into_index(),
    };
    let ranges = [
        UelRange::new(
            UelId::new(target - 10).unwrap(),
            UelId::new(target - 9).unwrap(),
        )
        .unwrap(),
        UelRange::new(UelId::new(target).unwrap(), UelId::new(target).unwrap()).unwrap(),
    ];
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Ranges(&ranges),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![target - 10, target - 9, target]);
    assert!(scan.indexed_statistics().unwrap().intervals_considered < 40);
}
#[test]
fn ordered_leading_bounds_handle_empty_symbols_and_prove_suffix_without_decoding_it() {
    let temp = Temp::new();
    let reader = reader();
    let symbol = reader.symbol(SymbolId::new(4).unwrap()).unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::One(UelId::new(5).unwrap()),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert!(scan.next_batch().unwrap().is_none());
    assert_eq!(scan.statistics().decoded_rows, 0);
    assert_eq!(
        scan.indexed_statistics()
            .unwrap()
            .intervals_skipped_by_leading_bounds,
        1
    );
    let temp = Temp::new();
    let reader = query_reader();
    let symbol = reader
        .symbol(SymbolId::new(if cfg!(feature = "compression") { 1 } else { 0 }).unwrap())
        .unwrap();
    let open = query_index(&reader, &symbol, &temp.index());
    let total = open.index.as_ref().unwrap().interval_count();
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Range(
                    UelRange::new(UelId::new(1).unwrap(), UelId::new(2).unwrap()).unwrap(),
                ),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(&symbol, &selection, reader.scan_options(), open)
        .unwrap();
    assert_eq!(query_keys(&mut scan), vec![1, 2]);
    let statistics = scan.indexed_statistics().unwrap();
    assert_eq!(statistics.intervals_considered, 2);
    assert_eq!(statistics.intervals_skipped_by_leading_bounds, total - 1);
    assert_eq!(scan.statistics().decoded_rows, 8);
}

#[test]
fn ordered_sequential_window_ending_at_source_eof_still_validates_terminator() {
    use crate::source::{ReadAt, Source};
    use std::sync::Mutex;
    struct Mutable(Arc<Mutex<Vec<u8>>>);
    impl ReadAt for Mutable {
        fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
            let data = self.0.lock().unwrap();
            let rest = data.get(at as usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len());
            out[..n].copy_from_slice(&rest[..n]);
            Ok(n)
        }
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    let temp = Temp::new();
    let data = Arc::new(Mutex::new(
        fs::read("tests/fixtures/v7/semantics-0.gdx").unwrap(),
    ));
    let len = data.lock().unwrap().len() as u64;
    let reader =
        Reader::from_source(Source::testing(Box::new(Mutable(Arc::clone(&data))), len)).unwrap();
    let symbol = reader.symbol(SymbolId::new(7).unwrap()).unwrap();
    let options = IndexOptions {
        record_stride: 1,
        ..IndexOptions::default()
    };
    reader
        .index_builder(&symbol, temp.index(), options)
        .unwrap()
        .finish()
        .unwrap();
    let open = reader
        .open_index(&symbol, temp.index(), options, IndexPolicy::UseIfValid)
        .unwrap();
    let mut decoder = Decoder::new(&reader, &symbol.inner.record).unwrap();
    for _ in 0..16 {
        decoder.next().unwrap().unwrap();
    }
    let encoded = decoder.position().unwrap().index_bytes().unwrap();
    let physical = u64::from_le_bytes(encoded[387..395].try_into().unwrap()) as usize;
    assert_eq!(data.lock().unwrap()[physical], 255);
    data.lock().unwrap()[physical] = 1;
    let selection = reader
        .selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Range(
                    UelRange::new(UelId::new(1).unwrap(), UelId::new(5).unwrap()).unwrap(),
                ),
            }],
        )
        .unwrap();
    let mut scan = reader
        .scan_selected_indexed(
            &symbol,
            &selection,
            ScanOptions {
                batch_bytes: 2 * std::mem::size_of::<UelId>() + std::mem::size_of::<Value>(),
                ..reader.scan_options()
            },
            open,
        )
        .unwrap();
    for _ in 0..16 {
        assert_eq!(scan.next_batch().unwrap().unwrap().len(), 1);
    }
    assert_eq!(
        scan.indexed_statistics().unwrap().mode,
        IndexScanMode::Sequential
    );
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
    assert_eq!(scan.statistics().returned_rows, 16);
    assert_eq!(
        scan.next_batch().err().unwrap().kind,
        ErrorKind::InvalidCount
    );
}
