use super::*;
use crate::write::tests::{Parent, Reset, fault, options};
fn limits(p: &Parent) -> MetadataOptions {
    let mut o = MetadataOptions::new(p.destination().parent().unwrap());
    o.run_bytes = 1024;
    o.maximum_open_files = 6;
    o.merge_fan_in = 2;
    o
}
fn writer(p: &Parent) -> Writer {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    w.begin_metadata(&limits(p)).unwrap();
    w
}
#[test]
fn raw_registration_ids_case_bytes_and_full_metadata_roundtrip() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = writer(&p);
    let root = Arc::clone(&w.budget);
    for (label, id) in [
        (b"z".as_slice(), 1),
        (b"a", 2),
        (b"z", 1),
        (b"A", 3),
        (b"\x80label", 4),
        (b"", 5),
    ] {
        assert_eq!(w.register_uel(label).unwrap().get(), id);
    }
    assert_eq!(w.add_set_text(b"").unwrap(), 0);
    assert_eq!(w.add_set_text(b"note").unwrap(), 1);
    assert_eq!(w.add_set_text(b"note").unwrap(), 1);
    w.add_acronym(AcronymId::new(7).unwrap(), b"Acro", b"text")
        .unwrap();
    let mut alias = WriteSymbol::new(b"alias", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Name(b"set"));
    w.register_symbol(&alias).unwrap();
    let s = w
        .register_symbol(&WriteSymbol {
            description: &[b'd'; 255],
            ..WriteSymbol::new(b"SET", SymbolType::Set, 1)
        })
        .unwrap();
    w.add_comment(s, b"one").unwrap();
    w.add_comment(s, b"two").unwrap();
    let param = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    w.set_domains(
        param,
        &[
            WriteDomain::Strict(SymbolReference::Registered(s)),
            WriteDomain::Strict(SymbolReference::Universe),
        ],
    )
    .unwrap();
    let relaxed = w
        .register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 2))
        .unwrap();
    w.set_domains(
        relaxed,
        &[
            WriteDomain::Relaxed(b"future"),
            WriteDomain::Relaxed(b"future"),
        ],
    )
    .unwrap();
    w.register_symbol(&WriteSymbol::new(b"Scalar", SymbolType::Parameter, 0))
        .unwrap();
    w.freeze_metadata().unwrap();
    w.freeze_metadata().unwrap();
    assert!(w.metadata_statistics().frozen);
    w.finish().unwrap();
    assert_eq!(w.metadata_statistics().temporary_bytes, 0);
    assert_eq!(w.metadata_statistics().open_files, 0);
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    assert_eq!(r.information().unwrap().uel_count, 5);
    let s = r.find_symbol(b"set").unwrap().unwrap();
    assert_eq!(s.metadata().unwrap().description, &[b'd'; 255]);
    let mut comments = r.comments(&s).unwrap();
    assert_eq!(comments.next().unwrap().unwrap().text, b"one");
    assert_eq!(comments.next().unwrap().unwrap().text, b"two");
    assert!(comments.next().unwrap().is_none());
    let s = r.find_symbol(b"P").unwrap().unwrap();
    let mut domains = r.domains(&s).unwrap();
    assert_eq!(
        domains
            .next()
            .unwrap()
            .unwrap()
            .strict_symbol
            .unwrap()
            .get(),
        2
    );
    assert_eq!(domains.next().unwrap().unwrap().name, b"*");
    let s = r.find_symbol(b"V").unwrap().unwrap();
    let mut domains = r.domains(&s).unwrap();
    assert_eq!(domains.next().unwrap().unwrap().name, b"future");
    assert_eq!(domains.next().unwrap().unwrap().name, b"future");
    let s = r.find_symbol(b"Scalar").unwrap().unwrap();
    let mut rows = r.scan(&s, r.scan_options()).unwrap();
    assert_eq!(rows.next_batch().unwrap().unwrap().len(), 1);
    assert!(rows.next_batch().unwrap().is_none());
}
#[test]
fn bulk_first_registration_order_tiny_runs_and_streaming_mapping() {
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = limits(&p);
    o.run_bytes = 8192;
    o.maximum_temporary_bytes = 32 * 1024 * 1024;
    w.begin_metadata(&o).unwrap();
    let requests = std::env::var("GDXRS_METADATA_SCALE_REQUESTS")
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(10000);
    assert!((1000..=200000).contains(&requests) && requests.is_multiple_of(2));
    let unique = requests / 2;
    let mut n = 0u64;
    let labels = (0..requests).map(|i| {
        let mut bytes = [b'0'; 7];
        bytes[0] = b'u';
        let mut number = unique - 1 - i % unique;
        for pos in (1..7).rev() {
            bytes[pos] = b'0' + (number % 10) as u8;
            number /= 10;
        }
        Ok(bytes)
    });
    w.register_uels_fresh(labels, |ordinal, id| {
        n += 1;
        assert_eq!(ordinal, n);
        assert_eq!(u64::from(id.get()), (ordinal - 1) % unique + 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(n, requests);
    let stats = w.metadata_statistics();
    if requests >= 100000 {
        assert!(stats.temporary_bytes > w.budget_statistics().limit_bytes as u64);
    }
    assert_eq!(u64::from(stats.uels), unique);
    assert!(stats.merge_passes > 1);
    assert!(stats.peak_temporary_bytes <= o.maximum_temporary_bytes);
    assert!(stats.peak_open_files <= 6);
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let mut uels = r.uels().unwrap();
    let mut seen = 0;
    while let Some(uel) = uels.next().unwrap() {
        let expected = format!("u{:06}", unique - 1 - seen);
        assert_eq!(uel.label, expected.as_bytes());
        seen += 1;
    }
    assert_eq!(seen, unique);
    eprintln!(
        "requests={requests} unique={unique} tracked_peak={} scratch_peak={} handles={} merge_passes={} journal_bytes={}",
        w.budget_statistics().peak_bytes,
        stats.peak_temporary_bytes,
        stats.peak_open_files,
        stats.merge_passes,
        stats.temporary_bytes
    );
}
#[test]
fn forward_alias_domains_cycles_missing_refs_and_foreign_writer_ids() {
    let _reset = Reset::new();
    for missing in [false, true] {
        let p = Parent::new();
        let mut w = writer(&p);
        let mut a = WriteSymbol::new(b"A", SymbolType::Alias, 1);
        a.alias_target = Some(SymbolReference::Name(b"B"));
        w.register_symbol(&a).unwrap();
        if !missing {
            a.name = b"B";
            a.alias_target = Some(SymbolReference::Name(b"a"));
            w.register_symbol(&a).unwrap();
        }
        assert_eq!(
            w.freeze_metadata().unwrap_err().kind,
            if missing {
                ErrorKind::InvalidReference
            } else {
                ErrorKind::AliasCycle
            }
        );
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
    }
    let p = Parent::new();
    let mut a = writer(&p);
    let token = a
        .register_symbol(&WriteSymbol::new(b"i", SymbolType::Set, 1))
        .unwrap();
    let q = Parent::new();
    let mut b = writer(&q);
    assert_eq!(
        b.add_comment(token, b"foreign").unwrap_err().kind,
        ErrorKind::SourceMismatch
    );
    q.empty();
    a.abort().unwrap();
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    let mut a = WriteSymbol::new(b"A", SymbolType::Alias, 1);
    a.alias_target = Some(SymbolReference::Name(b"B"));
    w.register_symbol(&a).unwrap();
    a.name = b"B";
    a.alias_target = Some(SymbolReference::Name(b"I"));
    w.register_symbol(&a).unwrap();
    w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let s = r.find_symbol(b"A").unwrap().unwrap();
    // Native GDX preserves A -> B, even when B resolves to set I.
    assert_eq!(s.metadata().unwrap().user_information, 2);
}
#[test]
fn invalid_registration_and_frozen_mutation_are_terminal_without_publication() {
    let _reset = Reset::new();
    for bytes in [b"bad\0".as_slice(), b"has'and\"", &[b'x'; 64]] {
        let p = Parent::new();
        let mut w = writer(&p);
        assert_eq!(
            w.register_uel(bytes).unwrap_err().kind,
            ErrorKind::InvalidOptions
        );
        assert_eq!(w.statistics().status, WriteStatus::Failed);
        p.empty();
    }
    let p = Parent::new();
    let mut w = writer(&p);
    w.register_symbol(&WriteSymbol::new(b"Hello", SymbolType::Set, 1))
        .unwrap();
    assert_eq!(
        w.register_symbol(&WriteSymbol::new(b"hELLo", SymbolType::Parameter, 0))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidOptions
    );
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    w.freeze_metadata().unwrap();
    assert_eq!(
        w.register_uel(b"late").unwrap_err().kind,
        ErrorKind::InvalidWriteState
    );
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    w.register_uel(b"x").unwrap();
    assert_eq!(
        w.register_uels_fresh([Ok(b"y".as_slice())], |_, _| Ok(()))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidOptions
    );
    p.empty();
}
#[test]
fn scratch_quotas_corruption_allocation_and_metadata_io_faults_cleanup() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = limits(&p);
    o.maximum_temporary_bytes = 2;
    w.begin_metadata(&o).unwrap();
    assert_eq!(
        w.register_uel(b"x").unwrap_err().kind,
        ErrorKind::TemporarySpaceExceeded
    );
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    w.register_uel(b"x").unwrap();
    let path = w.metadata.as_ref().unwrap().path(0);
    w.metadata.as_mut().unwrap().j().unwrap().flush().unwrap();
    let mut f = fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(3)).unwrap();
    f.write_all(&[255]).unwrap();
    drop(f);
    assert_eq!(
        w.freeze_metadata().unwrap_err().kind,
        ErrorKind::InvalidSpill
    );
    p.empty();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    crate::budget::inject_buffer_failure();
    assert_eq!(
        w.begin_metadata(&limits(&p)).unwrap_err().kind,
        ErrorKind::AllocationFailed
    );
    p.empty();
    for event in [
        Event::MetadataRead,
        Event::MetadataWrite,
        Event::MetadataSeek,
        Event::MetadataSync,
    ] {
        let p = Parent::new();
        let mut w = writer(&p);
        w.register_uel(b"x").unwrap();
        fault(event, 0, false, io::ErrorKind::StorageFull);
        assert_eq!(w.finish().unwrap_err().kind, ErrorKind::Io);
        assert!(!w.statistics().published);
        p.empty();
    }
}
#[test]
fn metadata_cleanup_retry_drop_and_bulk_callback_failure_do_not_publish() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = writer(&p);
    w.register_uel(b"x").unwrap();
    fault(
        Event::MetadataRemove,
        0,
        false,
        io::ErrorKind::PermissionDenied,
    );
    assert_eq!(w.abort().unwrap_err().kind, ErrorKind::Io);
    w.abort().unwrap();
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    assert_eq!(
        w.register_uels_fresh([Ok(b"x".as_slice()), Ok(b"x")], |_, _| Err(
            io::Error::other("consumer failed").into()
        ))
        .unwrap_err()
        .kind,
        ErrorKind::Io
    );
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    p.empty();
    let p = Parent::new();
    {
        let mut w = writer(&p);
        w.register_uel(b"x").unwrap();
    }
    p.empty();
}

#[test]
fn failed_spill_cleanup_retains_writer_ownership_and_retries() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = writer(&p);
    let root = Arc::clone(&w.budget);
    let _spill_fault = crate::spill::test_cleanup_failure();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = w
        .register_uels_fresh(
            (0u32..30).map(|i| Ok(i.to_be_bytes().map(|b| b % 26 + b'a'))),
            |_, _| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Io);
    assert!(error.cleanup_error().is_some());
    assert!(!w.statistics().published);
    assert!(
        w.metadata
            .as_ref()
            .unwrap()
            .sort_directory
            .as_ref()
            .unwrap()
            .exists()
    );
    let _reset = Reset::new();
    w.abort().unwrap();
    assert_eq!(w.metadata_statistics().temporary_bytes, 0);
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
    p.empty();
}
#[test]
fn namespace_collisions_input_errors_and_postpublication_cleanup_are_visible() {
    let _reset = Reset::new();
    for symbol_first in [false, true] {
        let p = Parent::new();
        let mut w = writer(&p);
        if symbol_first {
            w.register_symbol(&WriteSymbol::new(b"Name", SymbolType::Parameter, 0))
                .unwrap();
            assert_eq!(
                w.add_acronym(AcronymId::new(1).unwrap(), b"nAME", b"")
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidOptions
            );
        } else {
            w.add_acronym(AcronymId::new(1).unwrap(), b"Name", b"")
                .unwrap();
            assert_eq!(
                w.register_symbol(&WriteSymbol::new(b"nAME", SymbolType::Set, 1))
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidOptions
            );
        }
        p.empty();
    }
    let p = Parent::new();
    let mut w = writer(&p);
    let input = [
        Ok(b"a".as_slice()),
        Err(io::Error::other("producer failed").into()),
    ];
    assert_eq!(
        w.register_uels_fresh(input, |_, _| panic!("input must complete before mapping"))
            .unwrap_err()
            .kind,
        ErrorKind::Io
    );
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    w.register_uel(b"x").unwrap();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::Io);
    assert!(w.statistics().published);
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    let _reset = Reset::new();
    w.abort().unwrap();
    assert!(p.destination().exists());
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    assert_eq!(r.information().unwrap().uel_count, 1);
}

#[test]
fn corrupt_framing_extent_and_late_ordinal_damage_prevent_publication() {
    let _reset = Reset::new();
    for mode in 0..5 {
        let p = Parent::new();
        let mut w = writer(&p);
        w.register_uel(b"x").unwrap();
        let m = w.metadata.as_mut().unwrap();
        m.j().unwrap().flush().unwrap();
        let path = m.path(0);
        let length = m.j().unwrap().length;
        let mut file = fs::OpenOptions::new().write(true).open(path).unwrap();
        match mode {
            0 => file.write_all(&[0]).unwrap(),
            1 => {
                file.seek(SeekFrom::Start(1)).unwrap();
                file.write_all(&[255, 255]).unwrap();
            }
            2 => {
                file.seek(SeekFrom::Start(3)).unwrap();
                file.write_all(&[255]).unwrap();
            }
            3 => file.set_len(length - 1).unwrap(),
            _ => file.set_len(length + 1).unwrap(),
        };
        drop(file);
        assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidSpill);
        p.empty();
    }
    let p = Parent::new();
    let mut w = writer(&p);
    let map = w.metadata.as_ref().unwrap().path(2);
    let mut seen = 0;
    let error = w
        .register_uels_fresh([Ok(b"a".as_slice()), Ok(b"b")], |_, _| {
            seen += 1;
            if seen == 1 {
                let mut f = fs::OpenOptions::new().write(true).open(&map)?;
                f.seek(SeekFrom::Start(22))?;
                f.write_all(&[0])?;
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidSpill);
    assert_eq!(seen, 1);
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidSpill);
    p.empty();
    let p = Parent::new();
    let mut w = writer(&p);
    w.metadata.as_mut().unwrap().stats.uels = i32::MAX as u32;
    assert_eq!(
        w.register_uel(b"overflow").unwrap_err().kind,
        ErrorKind::InvalidCount
    );
    p.empty();
}

#[test]
fn strict_domains_preserve_direct_forward_and_chained_alias_ids() {
    let mut modes = vec![WriteCompression::None];
    if cfg!(feature = "compression") {
        modes.push(WriteCompression::Zlib);
    }
    for mode in modes {
        let p = Parent::new();
        let mut w = Writer::create(
            p.destination(),
            WriteOptions {
                compression: mode,
                ..options()
            },
        )
        .unwrap();
        w.begin_metadata(&limits(&p)).unwrap();
        let i = w
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        let named = w
            .register_symbol(&WriteSymbol::new(b"Named", SymbolType::Parameter, 1))
            .unwrap();
        w.set_domains(named, &[WriteDomain::Strict(SymbolReference::Name(b"K"))])
            .unwrap();
        let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
        alias.alias_target = Some(SymbolReference::Registered(i));
        let j = w.register_symbol(&alias).unwrap();
        alias.name = b"K";
        alias.alias_target = Some(SymbolReference::Registered(j));
        let k = w.register_symbol(&alias).unwrap();
        alias.name = b"All";
        alias.alias_target = Some(SymbolReference::Universe);
        let all = w.register_symbol(&alias).unwrap();
        for (name, reference) in [
            (b"Direct".as_slice(), SymbolReference::Registered(j)),
            (b"Chain", SymbolReference::Registered(k)),
            (b"UniverseAlias", SymbolReference::Registered(all)),
            (b"Universe", SymbolReference::Universe),
        ] {
            let p = w
                .register_symbol(&WriteSymbol::new(name, SymbolType::Parameter, 1))
                .unwrap();
            w.set_domains(p, &[WriteDomain::Strict(reference)]).unwrap();
        }
        w.freeze_metadata().unwrap();
        w.finish().unwrap();
        let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
        for (name, id, label) in [
            (b"Named".as_slice(), k.get(), b"K".as_slice()),
            (b"Direct", j.get(), b"J"),
            (b"Chain", k.get(), b"K"),
            (b"UniverseAlias", all.get(), b"All"),
            (b"Universe", 0, b"*"),
        ] {
            let symbol = r.find_symbol(name).unwrap().unwrap();
            let mut domains = r.domains(&symbol).unwrap();
            let d = domains.next().unwrap().unwrap();
            assert_eq!(d.strict_symbol.unwrap().get(), id, "{name:?}");
            assert_eq!(d.name, label, "{name:?}");
            assert!(domains.next().unwrap().is_none());
        }
    }
}

#[test]
fn initialization_allocation_and_cleanup_failure_keep_retryable_owner() {
    let _reset = Reset::new();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let root = Arc::clone(&w.budget);
    crate::budget::inject_buffer_failure();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = w.begin_metadata(&limits(&p)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::AllocationFailed);
    assert_eq!(
        error.cleanup_error().map(io::Error::kind),
        Some(io::ErrorKind::PermissionDenied)
    );
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    assert!(!w.statistics().published);
    let directory = w.metadata.as_ref().unwrap().directory.clone();
    assert!(directory.is_dir());
    assert!(!p.destination().exists());
    assert!(w.abort().is_err());
    assert!(directory.is_dir());
    let _reset = Reset::new();
    let stats = w.abort().unwrap();
    assert_eq!(stats.status, WriteStatus::Failed);
    assert!(!directory.exists());
    p.empty();
    // A successful cleanup retry does not erase or replay the original failure.
    let repeated = w.finish().unwrap_err();
    assert_eq!(repeated.kind, ErrorKind::AllocationFailed);
    assert!(repeated.cleanup_error().is_some());
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
}

#[test]
fn finalization_rejects_corrupt_completed_counts_positions_and_membership_extents() {
    for (position, count, start) in [
        (0, 0, 0),
        (u64::MAX, 0, 0),
        (126, u32::MAX, 0),
        (126, 1, u64::MAX),
    ] {
        let p = Parent::new();
        let mut w = writer(&p);
        let s = w
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        w.freeze_metadata().unwrap();
        w.metadata
            .as_mut()
            .unwrap()
            .complete_records(s, position, count, start)
            .unwrap();
        // Occupy a valid data extent so invalid membership is checked too.
        w.bytes(&[0; 30]).unwrap();
        assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidSpill);
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
    }
}
