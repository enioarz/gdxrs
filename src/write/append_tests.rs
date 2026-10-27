use super::*;
use tests::{Parent, Reset, fault};
fn write_options(compression: WriteCompression) -> WriteOptions {
    WriteOptions {
        memory_budget_bytes: 4 * 1024 * 1024,
        maximum_output_bytes: 1024 * 1024,
        compression,
    }
}
fn read_options() -> OpenOptions {
    OpenOptions {
        memory_budget_bytes: 4 * 1024 * 1024,
        maximum_batch_bytes: 256,
        ..OpenOptions::default()
    }
}
fn limits(p: &Parent) -> MetadataOptions {
    let mut m = MetadataOptions::new(p.destination().parent().unwrap());
    m.maximum_temporary_bytes = 1024 * 1024;
    m.maximum_open_files = 8;
    m.run_bytes = 512;
    m.merge_fan_in = 2;
    m
}
fn source(p: &Parent, compression: WriteCompression) {
    let mut w = Writer::create(p.destination(), write_options(compression)).unwrap();
    w.begin_metadata(&limits(p)).unwrap();
    let a = w.register_uel(b"alpha").unwrap();
    let b = w.register_uel(b"beta ").unwrap();
    let text = w.add_set_text(b"member").unwrap();
    w.add_set_text(b"spare!").unwrap();
    w.add_acronym(AcronymId::new(7).unwrap(), b"Acro", b"original")
        .unwrap();
    // Parameter precedes its alias and set in the catalog: import must not
    // require topological domain order or resolve the alias to its base set ID.
    let param = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.description = b"Aliased with I";
    alias.alias_target = Some(SymbolReference::Name(b"I"));
    let j = w.register_symbol(&alias).unwrap();
    let i = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    w.register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Variable, 0))
        .unwrap();
    w.set_domains(
        param,
        &[WriteDomain::Strict(SymbolReference::Registered(j))],
    )
    .unwrap();
    w.set_relaxed_domains(param, &[b"Display"]).unwrap();
    w.add_comment(param, &[b'C'; 255]).unwrap();
    w.add_comment(param, b"second").unwrap();
    let mut rows = w.begin_symbol(i, None).unwrap();
    rows.write(&[a], &[Value::NumberBits(f64::from(text).to_bits())])
        .unwrap();
    rows.write(&[b], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    let mut rows = w.begin_symbol(param, None).unwrap();
    rows.write(&[a], &[AcronymId::new(7).unwrap().value().unwrap()])
        .unwrap();
    rows.write(&[b], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    rows.finish().unwrap();
    w.finish().unwrap();
}
fn pending(
    p: &Parent,
    destination: &Path,
    compression: WriteCompression,
    policy: AppendPolicy,
) -> Writer {
    let mut admission = AppendOptions::new(policy);
    admission.maximum_open_files = 10;
    admission.maximum_transaction_bytes = 2 * 1024 * 1024;
    Writer::open_append(
        p.destination(),
        destination,
        read_options(),
        write_options(compression),
        admission,
    )
    .unwrap()
}
fn verify(path: &Path, added: bool) {
    let r = Reader::open(path, read_options()).unwrap();
    assert_eq!(
        r.information().unwrap().symbol_count,
        if added { 5 } else { 4 }
    );
    assert_eq!(
        r.information().unwrap().uel_count,
        if added { 3 } else { 2 }
    );
    let h = r.symbol(SymbolId::new(1).unwrap()).unwrap();
    assert_eq!(h.metadata().unwrap().comment_count, 2);
    let mut domains = r.domains(&h).unwrap();
    assert_eq!(domains.mode(), DomainMode::Relaxed);
    let d = domains.next().unwrap().unwrap();
    assert_eq!(d.strict_symbol.unwrap().get(), 2);
    assert_eq!(d.relaxed_name.unwrap(), b"Display");
    drop(domains);
    let mut scan = r.scan(&h, r.scan_options()).unwrap();
    let mut count = 0;
    while let Some(batch) = scan.next_batch().unwrap() {
        for n in 0..batch.len() {
            let row = batch.row(n).unwrap();
            count += 1;
            assert_eq!(row.key(0).unwrap().get(), count);
            assert_eq!(
                row.value(ValueField::Level).unwrap(),
                if count == 1 {
                    AcronymId::new(7).unwrap().value().unwrap()
                } else {
                    Value::Special(SpecialValue::Epsilon)
                }
            );
        }
    }
    assert_eq!(count, 2);
    let s = r.symbol(SymbolId::new(3).unwrap()).unwrap();
    assert!(s.metadata().unwrap().has_set_text);
    assert_eq!(
        r.symbol(SymbolId::new(4).unwrap())
            .unwrap()
            .metadata()
            .unwrap()
            .physical_record_count,
        0
    );
    if added {
        let h = r.symbol(SymbolId::new(5).unwrap()).unwrap();
        assert_eq!(h.metadata().unwrap().name, b"New");
        assert_eq!(h.metadata().unwrap().physical_record_count, 1);
    }
}
#[test]
fn append_preserves_ids_metadata_physical_empty_and_forward_alias_domains_across_codecs() {
    let modes = [
        WriteCompression::None,
        #[cfg(feature = "compression")]
        WriteCompression::Zlib,
    ];
    for &input in &modes {
        for &output in &modes {
            for policy in [AppendPolicy::CreateNew, AppendPolicy::ReplaceSource] {
                let p = Parent::new();
                source(&p, input);
                let original = fs::read(p.destination()).unwrap();
                let dest = if policy == AppendPolicy::ReplaceSource {
                    p.destination()
                } else {
                    p.destination().with_file_name("appended.gdx")
                };
                let mut w = pending(&p, &dest, output, policy);
                w.begin_append(&limits(&p)).unwrap();
                let stats = w.append_statistics().unwrap();
                assert!(stats.initialized);
                assert_eq!(stats.rewritten_records, 4);
                assert_eq!(stats.inherited_symbols, 4);
                assert_eq!(stats.admitted_transaction_bytes, 2 * 1024 * 1024);
                assert_eq!(stats.admitted_open_files, 10);
                let i = w.append_symbol_id(SymbolId::new(2).unwrap()).unwrap();
                let a = w.append_uel_id(UelId::new(1).unwrap()).unwrap();
                assert_eq!(w.register_uel(b"gamma").unwrap().get(), 3);
                let n = w
                    .register_symbol(&WriteSymbol::new(b"New", SymbolType::Parameter, 1))
                    .unwrap();
                assert_eq!(n.get(), 5);
                w.set_domains(n, &[WriteDomain::Strict(SymbolReference::Registered(i))])
                    .unwrap();
                let mut rows = w.begin_symbol(n, None).unwrap();
                rows.write(&[a], &[Value::NumberBits((-0.0f64).to_bits())])
                    .unwrap();
                rows.finish().unwrap();
                assert_eq!(fs::read(p.destination()).unwrap(), original);
                let stats = w.finish().unwrap();
                assert!(stats.published);
                assert_eq!(w.finish().unwrap(), stats);
                verify(&dest, true);
                if policy == AppendPolicy::CreateNew {
                    assert_eq!(fs::read(p.destination()).unwrap(), original);
                }
                assert!(w.budget_statistics().peak_bytes <= 4 * 1024 * 1024);
                assert!(!w.temporary_path().exists());
            }
        }
    }
}
#[test]
fn append_policies_reject_existing_wrong_and_late_destination_collisions() {
    let p = Parent::new();
    source(&p, WriteCompression::None);
    let other = p.destination().with_file_name("other.gdx");
    for (destination, policy) in [
        (p.destination(), AppendPolicy::CreateNew),
        (other.clone(), AppendPolicy::ReplaceSource),
    ] {
        assert!(
            Writer::open_append(
                p.destination(),
                destination,
                read_options(),
                write_options(WriteCompression::None),
                AppendOptions::new(policy)
            )
            .is_err()
        );
    }
    fs::write(&other, b"existing").unwrap();
    assert!(
        Writer::open_append(
            p.destination(),
            &other,
            read_options(),
            write_options(WriteCompression::None),
            AppendOptions::new(AppendPolicy::CreateNew)
        )
        .is_err()
    );
    fs::remove_file(&other).unwrap();
    let mut w = pending(&p, &other, WriteCompression::None, AppendPolicy::CreateNew);
    w.begin_append(&limits(&p)).unwrap();
    fs::write(&other, b"late").unwrap();
    assert!(w.finish().is_err());
    assert!(!w.statistics().published);
    assert_eq!(fs::read(&other).unwrap(), b"late");
    verify(&p.destination(), false);
    w.abort().unwrap();
}
#[test]
fn append_source_changes_before_import_and_publication_never_overwrite_source() {
    for during_import in [true, false] {
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        if !during_import {
            w.begin_append(&limits(&p)).unwrap();
        }
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(p.destination())
            .unwrap();
        f.write_all(b"changed").unwrap();
        drop(f);
        let changed = fs::read(p.destination()).unwrap();
        let e = if during_import {
            w.begin_append(&limits(&p)).unwrap_err()
        } else {
            w.finish().unwrap_err()
        };
        assert_eq!(e.kind, ErrorKind::SourceChanged);
        assert_eq!(fs::read(p.destination()).unwrap(), changed);
        assert_eq!(w.finish().unwrap_err().kind, e.kind);
        assert!(!w.statistics().published);
        w.abort().unwrap();
    }
}
#[test]
fn append_initialization_admits_aggregate_output_scratch_and_handles_before_import() {
    for disk in [true, false] {
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut a = AppendOptions::new(AppendPolicy::ReplaceSource);
        a.maximum_transaction_bytes = if disk { 1024 * 1024 } else { 2 * 1024 * 1024 };
        a.maximum_open_files = if disk { 10 } else { 9 };
        let mut w = Writer::open_append(
            p.destination(),
            p.destination(),
            read_options(),
            write_options(WriteCompression::None),
            a,
        )
        .unwrap();
        assert_eq!(
            w.begin_append(&limits(&p)).unwrap_err().kind,
            if disk {
                ErrorKind::TemporarySpaceExceeded
            } else {
                ErrorKind::InvalidOptions
            }
        );
        assert_eq!(w.append_statistics().unwrap().rewritten_records, 0);
        w.abort().unwrap();
        verify(&p.destination(), false);
    }
}
#[test]
fn append_all_import_and_publication_faults_preserve_original_and_retry_cleanup() {
    for phase in [
        Event::AppendRead,
        Event::MetadataCreate,
        Event::MetadataRead,
        Event::MetadataWrite,
        Event::MetadataSync,
        Event::Write,
        Event::Seek,
        Event::Sync,
        Event::AppendClose,
        Event::Publish,
    ] {
        let reset = Reset::new();
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let original = fs::read(p.destination()).unwrap();
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        let import = matches!(
            phase,
            Event::AppendRead
                | Event::MetadataCreate
                | Event::MetadataRead
                | Event::MetadataWrite
                | Event::MetadataSync
        );
        if !import {
            w.begin_append(&limits(&p)).unwrap();
        }
        tests::deny_cleanup();
        fault(phase, 0, true, io::ErrorKind::PermissionDenied);
        let e = if import {
            w.begin_append(&limits(&p)).unwrap_err()
        } else {
            w.finish().unwrap_err()
        };
        assert_eq!(e.kind, ErrorKind::Io, "{phase:?}");
        assert!(e.cleanup_error().is_some(), "{phase:?}");
        assert_eq!(fs::read(p.destination()).unwrap(), original);
        assert!(!w.statistics().published);
        assert!(w.temporary_path().exists());
        assert!(w.abort().is_err());
        drop(reset);
        w.abort().unwrap();
        assert!(!w.temporary_path().exists());
        verify(&p.destination(), false);
    }
}
#[test]
fn append_does_not_allow_inherited_metadata_or_data_replacement() {
    for operation in 0..7 {
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        w.begin_append(&limits(&p)).unwrap();
        let s = w.append_symbol_id(SymbolId::new(1).unwrap()).unwrap();
        let u = w.append_uel_id(UelId::new(1).unwrap()).unwrap();
        let result = match operation {
            0 => w.rename_uel(u, b"renamed"),
            1 => w.add_comment(s, b"changed"),
            2 => w.set_relaxed_domains(s, &[b"Changed"]),
            3 => w.set_domains(s, &[WriteDomain::Strict(SymbolReference::Universe)]),
            4 => w.update_acronym(AcronymId::new(7).unwrap(), b"Changed", b"changed"),
            5 => w
                .register_symbol(&WriteSymbol::new(b"p", SymbolType::Parameter, 1))
                .map(|_| ()),
            _ => match w.begin_symbol(s, None) {
                Err(e) => Err(e),
                Ok(_) => panic!("cannot overwrite inherited data"),
            },
        };
        assert!(result.is_err());
        w.abort().unwrap();
        verify(&p.destination(), false);
    }
}
#[test]
fn append_pending_and_initialized_cancellation_clean_up_without_publication() {
    for initialize in [false, true] {
        for explicit in [false, true] {
            let p = Parent::new();
            source(&p, WriteCompression::None);
            let temp;
            {
                let mut w = pending(
                    &p,
                    &p.destination(),
                    WriteCompression::None,
                    AppendPolicy::ReplaceSource,
                );
                temp = w.temporary_path().to_path_buf();
                if initialize {
                    w.begin_append(&limits(&p)).unwrap();
                }
                if explicit {
                    w.abort().unwrap();
                }
            }
            assert!(!temp.exists());
            assert_eq!(
                fs::read_dir(p.destination().parent().unwrap())
                    .unwrap()
                    .count(),
                1
            );
            verify(&p.destination(), false);
        }
    }
}

#[test]
fn append_source_changes_during_import_and_after_output_close_block_publication() {
    for (phase, after) in [
        (Event::AppendRead, 2),
        (Event::AppendClose, 0),
        (Event::Publish, 0),
    ] {
        let _reset = Reset::new();
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let original = fs::read(p.destination()).unwrap();
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        if phase != Event::AppendRead {
            w.begin_append(&limits(&p)).unwrap();
        }
        tests::mutate_source(phase, after, &p.destination());
        let e = if phase == Event::AppendRead {
            w.begin_append(&limits(&p)).unwrap_err()
        } else {
            w.finish().unwrap_err()
        };
        assert_eq!(e.kind, ErrorKind::SourceChanged);
        let mut changed = original;
        changed.extend_from_slice(b"external mutation");
        assert_eq!(fs::read(p.destination()).unwrap(), changed);
        assert!(!w.statistics().published);
        w.abort().unwrap();
    }
}
#[test]
fn append_replaced_source_path_is_checked_independently_of_open_file() {
    let p = Parent::new();
    source(&p, WriteCompression::None);
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    w.begin_append(&limits(&p)).unwrap();
    let swap = p.destination().with_file_name("swap.gdx");
    fs::write(&swap, b"external replacement").unwrap();
    fs::rename(&swap, p.destination()).unwrap();
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::SourceChanged);
    assert_eq!(fs::read(p.destination()).unwrap(), b"external replacement");
    w.abort().unwrap();
}
#[test]
fn append_quota_and_journal_initialization_errors_retain_cleanup_ownership() {
    for operation in 0..3 {
        let reset = Reset::new();
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        let mut m = limits(&p);
        if operation == 0 {
            m.maximum_temporary_bytes = 128;
        }
        if operation == 1 {
            w.limit = 197;
        }
        if operation == 2 {
            fault(
                Event::MetadataCreate,
                1,
                true,
                io::ErrorKind::PermissionDenied,
            );
        }
        tests::deny_cleanup();
        let e = w.begin_append(&m).unwrap_err();
        assert_eq!(
            e.kind,
            [
                ErrorKind::TemporarySpaceExceeded,
                ErrorKind::OutputLimitExceeded,
                ErrorKind::Io
            ][operation]
        );
        assert!(e.cleanup_error().is_some());
        assert!(!w.statistics().published);
        assert!(w.abort().is_err());
        drop(reset);
        w.abort().unwrap();
        assert_eq!(
            fs::read_dir(p.destination().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
        verify(&p.destination(), false);
    }
}
#[test]
fn append_published_cleanup_failure_reports_publication_and_preserves_primary_result() {
    for policy in [AppendPolicy::CreateNew, AppendPolicy::ReplaceSource] {
        let reset = Reset::new();
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let dest = if policy == AppendPolicy::ReplaceSource {
            p.destination()
        } else {
            p.destination().with_file_name("new.gdx")
        };
        let mut w = pending(&p, &dest, WriteCompression::None, policy);
        w.begin_append(&limits(&p)).unwrap();
        fault(
            Event::MetadataRemove,
            0,
            true,
            io::ErrorKind::PermissionDenied,
        );
        let e = w.finish().unwrap_err();
        assert_eq!(e.kind, ErrorKind::Io);
        assert!(e.cleanup_error().is_some());
        assert!(w.statistics().published);
        assert_eq!(w.statistics().status, WriteStatus::Failed);
        verify(&dest, false);
        assert!(w.abort().is_err());
        drop(reset);
        w.abort().unwrap();
        verify(&dest, false);
    }
}
#[test]
fn append_rejects_pending_finish_reinitialization_and_wrong_root_without_publication() {
    let p = Parent::new();
    source(&p, WriteCompression::None);
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    assert_eq!(
        w.register_uel(b"too-early").unwrap_err().kind,
        ErrorKind::InvalidWriteState
    );
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::InvalidWriteState);
    w.abort().unwrap();
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    w.begin_append(&limits(&p)).unwrap();
    assert!(w.begin_append(&limits(&p)).is_err());
    w.abort().unwrap();
    assert!(
        Writer::open_append(
            p.destination(),
            p.destination(),
            OpenOptions::default(),
            write_options(WriteCompression::None),
            AppendOptions::new(AppendPolicy::ReplaceSource)
        )
        .is_err()
    );
    verify(&p.destination(), false);
}
#[test]
fn append_import_checks_corrupt_inherited_data_before_publication() {
    let p = Parent::new();
    source(&p, WriteCompression::None);
    let r = Reader::open(p.destination(), read_options()).unwrap();
    let position = r.lookup_record(SymbolId::new(1).unwrap()).unwrap().position;
    drop(r);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(p.destination())
        .unwrap();
    f.seek(SeekFrom::Start(position)).unwrap();
    f.write_all(&[0]).unwrap();
    drop(f);
    let corrupt = fs::read(p.destination()).unwrap();
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    assert!(w.begin_append(&limits(&p)).is_err());
    assert!(!w.statistics().published);
    assert_eq!(fs::read(p.destination()).unwrap(), corrupt);
    w.abort().unwrap();
}

#[test]
fn append_preserves_duplicate_label_and_text_slots_and_prior_error_counts() {
    let p = Parent::new();
    source(&p, WriteCompression::None);
    let r = Reader::open(p.destination(), read_options()).unwrap();
    let h = r.source.header().unwrap();
    let mut bytes = fs::read(p.destination()).unwrap();
    // Frozen layout primitives: length-prefixed _UEL_ + i32 count, then
    // length-prefixed labels. Keep all frame lengths/IDs unchanged.
    let second_uel = h.offsets[1] as usize + 6 + 4 + 6;
    assert_eq!(&bytes[second_uel..second_uel + 6], b"\x05beta ");
    bytes[second_uel + 1..second_uel + 6].copy_from_slice(b"alpha");
    let second_text = h.offsets[2] as usize + 7 + 4 + 1 + 7;
    assert_eq!(&bytes[second_text..second_text + 7], b"\x06spare!");
    bytes[second_text + 1..second_text + 7].copy_from_slice(b"member");
    let errors = h.offsets[0] as usize + 11 + 2 + 8 + 4 + 1 + 4 + 4;
    bytes[errors..errors + 4].copy_from_slice(&777u32.to_le_bytes());
    drop(r);
    fs::write(p.destination(), bytes).unwrap();
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    w.begin_append(&limits(&p)).unwrap();
    assert_eq!(w.append_uel_id(UelId::new(2).unwrap()).unwrap().get(), 2);
    assert_eq!(w.register_uel(b"alpha").unwrap().get(), 1);
    w.finish().unwrap();
    let r = Reader::open(p.destination(), read_options()).unwrap();
    let mut uels = r.uels().unwrap();
    for id in 1..=2 {
        let u = uels.next().unwrap().unwrap();
        assert_eq!(u.id.get(), id);
        assert_eq!(u.label, b"alpha");
    }
    assert!(uels.next().unwrap().is_none());
    let mut texts = r.set_texts().unwrap();
    assert_eq!(texts.next().unwrap().unwrap().text, b"");
    for id in 1..=2 {
        let t = texts.next().unwrap().unwrap();
        assert_eq!(t.id, id);
        assert_eq!(t.text, b"member");
    }
    assert!(texts.next().unwrap().is_none());
    assert_eq!(
        r.symbol(SymbolId::new(1).unwrap())
            .unwrap()
            .metadata()
            .unwrap()
            .error_count,
        777
    );
}
#[test]
fn append_preserves_maximum_length_header_strings_and_rejects_too_small_header_cap_before_creation()
{
    let p = Parent::new();
    Writer::create(p.destination(), write_options(WriteCompression::None))
        .unwrap()
        .finish()
        .unwrap();
    let original = fs::read(p.destination()).unwrap();
    // Empty raw fixture has no record pointers to relocate. Enlarge only its
    // header strings and shift the six major-index entries by exactly 500.
    let mut bytes = original[..34].to_vec();
    bytes.push(255);
    bytes.extend_from_slice(&[b'A'; 255]);
    bytes.push(255);
    bytes.extend_from_slice(&[b'P'; 255]);
    let mut index = original[46..126].to_vec();
    for n in 0..6 {
        let at = 4 + n * 8;
        let old = u64::from_le_bytes(index[at..at + 8].try_into().unwrap());
        if old != 0 {
            index[at..at + 8].copy_from_slice(&(old + 500).to_le_bytes());
        }
    }
    bytes.extend_from_slice(&index);
    bytes.extend_from_slice(&original[126..]);
    fs::write(p.destination(), bytes).unwrap();
    let mut small = write_options(WriteCompression::None);
    small.maximum_output_bytes = 197;
    assert!(
        Writer::open_append(
            p.destination(),
            p.destination(),
            read_options(),
            small,
            AppendOptions::new(AppendPolicy::ReplaceSource)
        )
        .is_err()
    );
    assert_eq!(
        fs::read_dir(p.destination().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    let mut w = pending(
        &p,
        &p.destination(),
        WriteCompression::None,
        AppendPolicy::ReplaceSource,
    );
    w.begin_append(&limits(&p)).unwrap();
    w.finish().unwrap();
    let r = Reader::open(p.destination(), read_options()).unwrap();
    assert_eq!(r.information().unwrap().audit, &[b'A'; 255]);
    assert_eq!(r.information().unwrap().producer, &[b'P'; 255]);
    assert_eq!(r.information().unwrap().symbol_count, 0);
}

#[test]
fn append_new_symbols_support_unsorted_string_mapped_and_deferred_adapters() {
    for adapter in 0..4 {
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        w.begin_append(&limits(&p)).unwrap();
        let n = w
            .register_symbol(&WriteSymbol::new(b"New", SymbolType::Parameter, 1))
            .unwrap();
        let old = w.append_symbol_id(SymbolId::new(2).unwrap()).unwrap();
        w.set_domains(n, &[WriteDomain::Strict(SymbolReference::Registered(old))])
            .unwrap();
        let a = w.append_uel_id(UelId::new(1).unwrap()).unwrap();
        let b = w.append_uel_id(UelId::new(2).unwrap()).unwrap();
        match adapter {
            0 => {
                let mut rows = w.begin_unsorted_symbol(n, None).unwrap();
                rows.write(&[b], &[Value::ZERO]).unwrap();
                rows.write(&[a], &[Value::ZERO]).unwrap();
                rows.finish().unwrap();
            }
            1 => {
                let mut rows = w.begin_string_records().unwrap();
                rows.begin_symbol(n).unwrap();
                rows.write(&[b"beta ".as_slice()], &[Value::ZERO]).unwrap();
                rows.write(&[b"alpha".as_slice()], &[Value::ZERO]).unwrap();
                rows.finish().unwrap();
            }
            2 => {
                w.register_mapped_uels(
                    [
                        (MappedUelId::new(0).unwrap(), b"alpha".as_slice()),
                        (MappedUelId::new(i32::MAX as i64).unwrap(), b"beta "),
                    ]
                    .map(Ok),
                )
                .unwrap();
                let mut rows = w.begin_mapped_symbol(n).unwrap();
                rows.write(
                    &[MappedUelId::new(i32::MAX as i64).unwrap()],
                    &[Value::ZERO],
                )
                .unwrap();
                rows.write(&[MappedUelId::new(0).unwrap()], &[Value::ZERO])
                    .unwrap();
                rows.finish().unwrap();
            }
            _ => {
                let mut rows = w.begin_deferred_records().unwrap();
                rows.begin_symbol(n).unwrap();
                rows.write(&[b], &[Value::ZERO]).unwrap();
                rows.write(&[a], &[Value::ZERO]).unwrap();
                rows.finish().unwrap();
            }
        }
        w.finish().unwrap();
        let r = Reader::open(p.destination(), read_options()).unwrap();
        let h = r.symbol(SymbolId::new(5).unwrap()).unwrap();
        assert_eq!(h.metadata().unwrap().physical_record_count, 2);
        assert_eq!(r.information().unwrap().uel_count, 2);
        let mut scan = r.scan(&h, r.scan_options()).unwrap();
        let mut count = 0;
        while let Some(batch) = scan.next_batch().unwrap() {
            for row in 0..batch.len() {
                count += 1;
                assert_eq!(batch.row(row).unwrap().key(0).unwrap().get(), count);
            }
        }
        assert_eq!(count, 2);
    }
}

#[test]
fn append_inflight_read_failure_stays_primary_and_retains_cleanup_diagnostics() {
    for changed_source in [false, true] {
        let reset = Reset::new();
        let p = Parent::new();
        source(&p, WriteCompression::None);
        let mut w = pending(
            &p,
            &p.destination(),
            WriteCompression::None,
            AppendPolicy::ReplaceSource,
        );
        tests::deny_cleanup();
        if changed_source {
            tests::mutate_source(Event::AppendRead, 2, &p.destination());
        } else {
            fault(Event::AppendRead, 1, true, io::ErrorKind::PermissionDenied);
        }
        let e = w.begin_append(&limits(&p)).unwrap_err();
        assert_eq!(
            e.kind,
            if changed_source {
                ErrorKind::SourceChanged
            } else {
                ErrorKind::Io
            }
        );
        assert!(e.cleanup_error().is_some());
        let sticky = w.finish().unwrap_err();
        assert_eq!(sticky.kind, e.kind);
        assert!(sticky.cleanup_error().is_some());
        assert_eq!(w.begin_append(&limits(&p)).unwrap_err().kind, e.kind);
        assert!(!w.statistics().published);
        assert!(w.temporary_path().exists());
        assert!(w.abort().is_err());
        drop(reset);
        w.abort().unwrap();
        assert_eq!(
            fs::read_dir(p.destination().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }
}
