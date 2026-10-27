use super::*;
use tests::{Parent, Reset, fault, options};
fn limits(p: &Parent) -> MetadataOptions {
    let mut o = MetadataOptions::new(p.destination().parent().unwrap());
    o.maximum_open_files = 8;
    o.run_bytes = 512;
    o.merge_fan_in = 2;
    o
}
fn writer(p: &Parent) -> Writer {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    w.begin_metadata(&limits(p)).unwrap();
    w
}
fn setup(p: &Parent) -> (Writer, [WriterUelId; 3], [WriterSymbolId; 5], u32) {
    let mut w = writer(p);
    w.register_mapped_uels(
        [(0, b"a".as_slice()), (9, b"b"), (i32::MAX as i64, b"c")]
            .map(|(id, label)| Ok((MappedUelId::new(id).unwrap(), label))),
    )
    .unwrap();
    let keys = [
        w.raw_id_for_mapped(MappedUelId::new(0).unwrap()).unwrap(),
        w.raw_id_for_mapped(MappedUelId::new(9).unwrap()).unwrap(),
        w.raw_id_for_mapped(MappedUelId::new(i32::MAX as i64).unwrap())
            .unwrap(),
    ];
    let i = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Registered(i));
    let j = w.register_symbol(&alias).unwrap();
    let k = w
        .register_symbol(&WriteSymbol::new(b"K", SymbolType::Set, 1))
        .unwrap();
    let p = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    let r = w
        .register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 0))
        .unwrap();
    w.set_domains(
        p,
        &[
            WriteDomain::Strict(SymbolReference::Registered(j)),
            WriteDomain::Strict(SymbolReference::Registered(i)),
        ],
    )
    .unwrap();
    let text = w.add_set_text(b"element text").unwrap();
    (w, keys, [i, j, k, p, r], text)
}
fn source(p: &Parent) -> Reader {
    let (mut w, k, s, text) = setup(p);
    let mut rows = w.begin_symbol(s[0], None).unwrap();
    rows.write(&[k[0]], &[Value::NumberBits(f64::from(text).to_bits())])
        .unwrap();
    rows.write(&[k[1]], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    let mut rows = w.begin_symbol(s[3], None).unwrap();
    rows.write(&[k[0], k[1]], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    rows.write(&[k[1], k[0]], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    w.finish().unwrap();
    Reader::open(
        p.destination(),
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap()
}
#[test]
fn post_record_renames_relocate_memberships_and_preserve_strict_aliases_display_domains_and_flags()
{
    let p = Parent::new();
    let (mut w, k, s, text) = setup(&p);
    w.rename_uel(k[1], b"B-long-name").unwrap();
    let mut rows = w.begin_symbol(s[0], None).unwrap();
    rows.write(&[k[0]], &[Value::NumberBits(f64::from(text).to_bits())])
        .unwrap();
    rows.write(&[k[1]], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    assert!(w.has_set_text(s[0]).unwrap());
    assert!(!w.has_set_text(s[1]).unwrap());
    w.add_comment(s[0], b"after records").unwrap();
    w.set_relaxed_domains(s[3], &[b"Shown", b"*"]).unwrap();
    assert!(w.set_text_node(text, i32::MIN).unwrap());
    assert!(!w.set_text_node(text, 7).unwrap());
    w.rename_uel(k[0], &[b'L'; 63]).unwrap();
    let mut rows = w.begin_symbol(s[2], None).unwrap();
    rows.write(&[k[2]], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    w.rename_uel(k[2], b"C").unwrap();
    let mut rows = w.begin_symbol(s[3], None).unwrap();
    rows.write(&[k[0], k[1]], &[Value::NumberBits((-0.0f64).to_bits())])
        .unwrap();
    rows.finish().unwrap();
    w.set_relaxed_domains(s[3], &[b"NewShown", b"*"]).unwrap();
    let mut rows = w.begin_symbol(s[4], None).unwrap();
    rows.write(&[], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    w.rename_uel(k[2], b"final").unwrap();
    w.add_comment(s[3], b"late P").unwrap();
    assert_eq!(w.uel_information(k[0]).unwrap().id.get(), 1);
    assert_eq!(w.uel_information(k[0]).unwrap().mapped.unwrap().get(), 0);
    assert!(w.find_uel(b"a").unwrap().is_none());
    assert_eq!(w.find_uel(b"final").unwrap().unwrap(), k[2]);
    assert_eq!(w.set_text_information(text).unwrap().node, i32::MIN);
    assert!(w.metadata_statistics().peak_open_files <= 8);
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    assert_eq!(r.set_text_information(text).unwrap().node, 0);
    assert_eq!(
        r.uel_information(UelId::new(1).unwrap()).unwrap().label(),
        &[b'L'; 63]
    );
    assert_eq!(r.find_symbol(b"p").unwrap().unwrap().id().get(), 4);
    let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
    let mut domains = r.domains(&h).unwrap();
    assert_eq!(domains.mode(), DomainMode::Relaxed);
    let d = domains.next().unwrap().unwrap();
    assert_eq!(d.strict_symbol.unwrap().get(), 2);
    assert_eq!(d.name, b"NewShown");
    let d = domains.next().unwrap().unwrap();
    assert_eq!(d.strict_symbol.unwrap().get(), 1);
    assert_eq!(d.name, b"*");
    assert!(
        r.symbol(SymbolId::new(1).unwrap())
            .unwrap()
            .metadata()
            .unwrap()
            .has_set_text
    );
    assert!(
        !r.symbol(SymbolId::new(2).unwrap())
            .unwrap()
            .metadata()
            .unwrap()
            .has_set_text
    );
    let mut scan = r.scan(&h, r.scan_options()).unwrap();
    let b = scan.next_batch().unwrap().unwrap();
    assert_eq!(b.key_column(0).unwrap()[0].get(), 1);
    assert_eq!(
        b.value_column(ValueField::Level).unwrap()[0],
        Value::NumberBits((-0.0f64).to_bits())
    );
    assert_eq!(r.uel_max_length().unwrap(), 63);
    assert_eq!(r.symbol_max_length().unwrap(), 1);
    let maxima = r.symbol_index_max_lengths(&h).unwrap();
    assert_eq!(&maxima[..2], &[63, 11]);
}
#[test]
fn display_override_does_not_disable_strict_checks_and_frozen_identity_schema_is_immutable() {
    let p = Parent::new();
    let (mut w, k, s, _) = setup(&p);
    let mut rows = w.begin_symbol(s[0], None).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    rows.finish().unwrap();
    w.set_relaxed_domains(s[3], &[b"anything", b"*"]).unwrap();
    let mut rows = w.begin_symbol(s[3], None).unwrap();
    assert!(rows.write(&[k[2], k[0]], &[Value::ZERO]).is_err());
    drop(rows);
    assert!(!p.destination().exists());
    w.abort().unwrap();
    p.empty();
    for kind in 0..3 {
        let p = Parent::new();
        let (mut w, _, s, _) = setup(&p);
        w.begin_symbol(s[4], None).unwrap().finish().unwrap();
        let e = match kind {
            0 => w.register_uel(b"new").unwrap_err(),
            1 => w.add_set_text(b"new").unwrap_err(),
            _ => w
                .set_domains(s[2], &[WriteDomain::Strict(SymbolReference::Universe)])
                .unwrap_err(),
        };
        assert_eq!(e.kind, ErrorKind::InvalidWriteState);
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn exact_case_rename_collisions_invalid_labels_and_wrong_owner_are_terminal() {
    for mode in 0..6 {
        let p = Parent::new();
        let (mut w, k, _, _) = setup(&p);
        let other = Parent::new();
        let mut foreign = writer(&other);
        let foreign_key = foreign.register_uel(b"x").unwrap();
        let new: &[u8] = match mode {
            0 => b"a",
            1 => b"b",
            2 => &[b'x'; 64],
            3 => b"bad\0label",
            _ => b"A",
        };
        let result = w.rename_uel(if mode == 5 { foreign_key } else { k[0] }, new);
        if mode == 4 {
            result.unwrap();
            assert!(w.find_uel(b"a").unwrap().is_none());
            assert_eq!(w.find_uel(b"A").unwrap().unwrap(), k[0]);
            w.finish().unwrap();
        } else {
            assert!(result.is_err());
            assert!(w.finish().is_err());
            w.abort().unwrap();
            p.empty();
        }
        foreign.abort().unwrap();
    }
}
#[test]
fn reader_overlay_renames_nodes_and_visits_are_private_and_id_preserving() {
    let p = Parent::new();
    let r = source(&p);
    let mut a = r.metadata_session(&limits(&p)).unwrap();
    let mut b = r.metadata_session(&limits(&p)).unwrap();
    a.rename_uel(UelId::new(1).unwrap(), b"A").unwrap();
    a.rename_uel(UelId::new(1).unwrap(), b"Again").unwrap();
    assert!(a.find_uel(b"A").unwrap().is_none());
    assert_eq!(a.find_uel(b"Again").unwrap().unwrap().get(), 1);
    assert_eq!(b.find_uel(b"a").unwrap().unwrap().get(), 1);
    assert_eq!(r.find_uel(b"a").unwrap().unwrap().get(), 1);
    assert!(a.set_text_node(0, 0).unwrap());
    assert!(a.set_text_node(0, i32::MAX).unwrap());
    assert!(!a.set_text_node(0, 7).unwrap());
    assert!(a.set_text_node(1, i32::MIN).unwrap());
    assert_eq!(a.set_text_information(1).unwrap().node, i32::MIN);
    assert_eq!(b.set_text_information(1).unwrap().node, 0);
    assert_eq!(r.set_text_information(1).unwrap().node, 0);
    let mut seen = 0;
    let visit = a
        .visit_uels(|info| {
            seen += 1;
            assert_eq!(info.id.get(), seen);
            if seen == 1 {
                assert_eq!(info.label(), b"Again");
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(
        visit,
        MetadataVisitStatistics {
            visited: 3,
            complete: true
        }
    );
    a.close().unwrap();
    b.close().unwrap();
}
#[test]
fn sparse_mapping_information_direct_labels_and_explicit_overlay_join() {
    let p = Parent::new();
    let r = source(&p);
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register(
        [(1, 9), (2, 0), (3, i32::MAX as i64)]
            .map(|(raw, mapped)| Ok((UelId::new(raw).unwrap(), MappedUelId::new(mapped).unwrap()))),
    )
    .unwrap();
    assert_eq!(
        map.information().unwrap(),
        UelMappingInformation {
            uel_count: 3,
            mapped_count: 3,
            highest_mapped: Some(MappedUelId::new(i32::MAX as i64).unwrap())
        }
    );
    assert_eq!(
        map.mapped_uel(MappedUelId::new(0).unwrap())
            .unwrap()
            .unwrap()
            .label(),
        b"b"
    );
    assert!(
        map.mapped_uel(MappedUelId::new(8).unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        map.find_uel(b"a").unwrap().unwrap().mapped.unwrap().get(),
        9
    );
    assert!(map.find_uel(b"A").unwrap().is_none());
    let mut session = r.metadata_session(&limits(&p)).unwrap();
    session.rename_uel(UelId::new(1).unwrap(), b"A").unwrap();
    assert_eq!(
        map.uel_in_session(&mut session, UelId::new(1).unwrap())
            .unwrap()
            .label(),
        b"A"
    );
    assert_eq!(
        map.mapped_uel(MappedUelId::new(9).unwrap())
            .unwrap()
            .unwrap()
            .label(),
        b"a"
    );
    session.close().unwrap();
    map.close().unwrap();
}
#[test]
fn domain_callbacks_use_mapped_order_original_ids_and_terminal_acknowledgments() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register(
        [(1, 9), (2, 0)]
            .map(|(raw, mapped)| Ok((UelId::new(raw).unwrap(), MappedUelId::new(mapped).unwrap()))),
    )
    .unwrap();
    let mut seen = vec![];
    let stats = map
        .visit_domain(&h, 0, |raw, mapped| {
            seen.push((raw.get(), mapped.get()));
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, vec![(2, 0), (1, 9)]);
    assert_eq!(
        stats,
        MetadataVisitStatistics {
            visited: 2,
            complete: true
        }
    );
    let mut cleanup = None;
    let mut calls = 0;
    let error = map
        .visit_domain(&h, 0, |_, _| {
            calls += 1;
            if calls == 2 {
                cleanup = Some(crate::spill::test_cleanup_failure());
                fault(
                    Event::MetadataRemove,
                    0,
                    true,
                    io::ErrorKind::PermissionDenied,
                );
                Err(Error::new(ErrorKind::InvalidOptions, "callback failed"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidOptions);
    assert!(error.cleanup_error().is_some());
    assert_eq!(
        map.domain_visit_statistics(),
        MetadataVisitStatistics {
            visited: 1,
            complete: false
        }
    );
    assert!(
        map.visit_domain(&h, 0, |_, _| panic!("must not replay"))
            .is_err()
    );
    assert!(map.abort().is_err());
    drop(cleanup);
    drop(reset);
    map.abort().unwrap();
}
#[test]
fn partial_uel_callback_fault_keeps_cleanup_diagnostics_and_never_replays() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let mut s = r.metadata_session(&limits(&p)).unwrap();
    s.rename_uel(UelId::new(1).unwrap(), b"A").unwrap();
    let error = s
        .visit_uels(|info| {
            if info.id.get() == 2 {
                fault(
                    Event::MetadataRemove,
                    0,
                    true,
                    io::ErrorKind::PermissionDenied,
                );
                return Err(Error::new(ErrorKind::InvalidOptions, "sink failed"));
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidOptions);
    assert!(error.cleanup_error().is_some());
    assert_eq!(
        s.visit_statistics(),
        MetadataVisitStatistics {
            visited: 1,
            complete: false
        }
    );
    assert!(s.visit_uels(|_| panic!("must not replay")).is_err());
    assert!(s.close().is_err());
    drop(reset);
    s.abort().unwrap();
}
#[test]
fn rewrite_io_quota_and_reopen_failures_block_publication_and_preserve_ownership() {
    for (event, after) in [
        (Event::MetadataCreate, 0),
        (Event::MetadataWrite, 0),
        (Event::MetadataSync, 0),
        (Event::MetadataRename, 0),
        (Event::MetadataCreate, 1),
    ] {
        let reset = Reset::new();
        let p = Parent::new();
        let (mut w, k, s, _) = setup(&p);
        let mut rows = w.begin_symbol(s[0], None).unwrap();
        rows.write(&[k[0]], &[Value::ZERO]).unwrap();
        rows.finish().unwrap();
        fault(event, after, false, io::ErrorKind::StorageFull);
        assert_eq!(
            w.rename_uel(k[0], b"longer").unwrap_err().kind,
            ErrorKind::Io
        );
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
        drop(reset);
    }
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = limits(&p);
    o.maximum_temporary_bytes = 128;
    w.begin_metadata(&o).unwrap();
    let id = w.register_uel(b"a").unwrap();
    assert_eq!(
        w.rename_uel(id, &[b'x'; 63]).unwrap_err().kind,
        ErrorKind::TemporarySpaceExceeded
    );
    w.abort().unwrap();
    p.empty();
}
#[test]
fn overlay_lazy_allocation_quota_and_corruption_errors_keep_abortable_owner() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let mut s = r.metadata_session(&limits(&p)).unwrap();
    let b = r.budget_statistics();
    let reservation = r
        .source
        .budget
        .reserve(b.limit_bytes - b.current_bytes)
        .unwrap();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = s.rename_uel(UelId::new(1).unwrap(), b"A").unwrap_err();
    assert_eq!(error.kind, ErrorKind::BudgetTooSmall);
    assert!(error.cleanup_error().is_some());
    drop(reservation);
    drop(reset);
    s.abort().unwrap();
    let mut o = limits(&p);
    o.maximum_temporary_bytes = 64;
    let mut s = r.metadata_session(&o).unwrap();
    s.rename_uel(UelId::new(1).unwrap(), b"A").unwrap();
    assert_eq!(
        s.rename_uel(UelId::new(1).unwrap(), b"A-long-enough-name")
            .unwrap_err()
            .kind,
        ErrorKind::TemporarySpaceExceeded
    );
    s.abort().unwrap();
    let mut s = r.metadata_session(&limits(&p)).unwrap();
    s.rename_uel(UelId::new(1).unwrap(), b"A").unwrap();
    s.uel_information(UelId::new(1).unwrap()).unwrap();
    let dir = fs::read_dir(p.destination().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap();
    fs::write(dir.join("journal"), b"bad").unwrap();
    assert!(s.uel_information(UelId::new(1).unwrap()).is_err());
    s.abort().unwrap();
}

#[test]
fn invalid_read_ids_nodes_missing_maps_and_cross_source_callbacks_are_terminal() {
    for mode in 0..3 {
        let p = Parent::new();
        let r = source(&p);
        let mut session = r.metadata_session(&limits(&p)).unwrap();
        let error = match mode {
            0 => session.set_text_node(99, 1).unwrap_err(),
            1 => session
                .rename_uel(UelId::new(4).unwrap(), b"new")
                .unwrap_err(),
            _ => session
                .rename_uel(UelId::new(1).unwrap(), b"b")
                .unwrap_err(),
        };
        assert_eq!(session.find_uel(b"a").unwrap_err().kind, error.kind);
        session.abort().unwrap();
    }
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register([Ok((UelId::new(1).unwrap(), MappedUelId::new(0).unwrap()))])
        .unwrap();
    let error = map
        .visit_domain(&h, 0, |_, _| panic!("prescan must fail before callbacks"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidReference);
    assert_eq!(map.domain_visit_statistics().visited, 0);
    map.abort().unwrap();
    let other = Parent::new();
    let other_r = source(&other);
    let mut session = other_r.metadata_session(&limits(&other)).unwrap();
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register([Ok((UelId::new(1).unwrap(), MappedUelId::new(0).unwrap()))])
        .unwrap();
    assert_eq!(
        map.uel_in_session(&mut session, UelId::new(1).unwrap())
            .err()
            .unwrap()
            .kind,
        ErrorKind::SourceMismatch
    );
    assert!(session.find_uel(b"a").unwrap().is_some());
    session.close().unwrap();
    map.abort().unwrap();
}
#[test]
fn growing_long_dictionary_edits_and_visits_keep_one_root_and_exact_bytes() {
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = limits(&p);
    o.run_bytes = 16384;
    w.begin_metadata(&o).unwrap();
    w.register_uels_fresh(
        (0..2048).map(|n| Ok(format!("L{n:08}{}", "x".repeat(54)))),
        |_, _| Ok(()),
    )
    .unwrap();
    let key = w
        .find_uel(b"L00001024xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
        .unwrap()
        .unwrap();
    assert_eq!(key.get(), 1025);
    w.rename_uel(key, b"\x80exact ").unwrap();
    assert_eq!(w.find_uel(b"\x80exact ").unwrap().unwrap(), key);
    assert!(w.find_uel(b"\x80exact").unwrap().is_none());
    w.finish().unwrap();
    let r = Reader::open(
        p.destination(),
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let mut session = r.metadata_session(&limits(&p)).unwrap();
    session.rename_uel(UelId::new(1).unwrap(), b"").unwrap();
    session
        .rename_uel(UelId::new(2048).unwrap(), b"final")
        .unwrap();
    let mut count = 0u32;
    let stats = session
        .visit_uels(|info| {
            count += 1;
            assert_eq!(info.id.get(), count);
            if count == 1 {
                assert_eq!(info.label(), b"");
            }
            if count == 1025 {
                assert_eq!(info.label(), b"\x80exact ");
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(stats.visited, 2048);
    assert!(r.budget_statistics().peak_bytes <= 1024 * 1024);
    assert_eq!(session.statistics().peak_open_files, 1);
    session.close().unwrap();
}

#[test]
fn unmapped_writer_ids_after_map_registration_keep_optional_mapping_and_original_ids() {
    let p = Parent::new();
    let (mut w, _, _, _) = setup(&p);
    let id = w.register_uel(b"not mapped").unwrap();
    assert_eq!(id.get(), 4);
    let info = w.uel_information(id).unwrap();
    assert_eq!(info.id.get(), 4);
    assert_eq!(info.label(), b"not mapped");
    assert_eq!(info.mapped, None);
    let stats = w.uel_mapping_information().unwrap();
    assert_eq!((stats.uel_count, stats.mapped_count), (4, 3));
    w.rename_uel(id, b"changed").unwrap();
    assert_eq!(w.uel_information(id).unwrap().mapped, None);
    w.finish().unwrap();
}
#[test]
fn domain_prepare_io_fault_retains_parent_cleanup_ownership_before_callbacks() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register(
        [(1, 9), (2, 0)].map(|(r, m)| Ok((UelId::new(r).unwrap(), MappedUelId::new(m).unwrap()))),
    )
    .unwrap();
    let io = crate::spill::test_io_failure(false);
    let cleanup = crate::spill::test_cleanup_failure();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = map
        .visit_domain(&h, 0, |_, _| panic!("prepare must fail before output"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Io);
    assert!(error.cleanup_error().is_some());
    assert_eq!(map.domain_visit_statistics().visited, 0);
    assert!(map.abort().is_err());
    drop(io);
    drop(cleanup);
    drop(reset);
    map.abort().unwrap();
}
#[test]
fn metadata_callbacks_detect_source_changes_and_keep_partial_acknowledgments_sticky() {
    let p = Parent::new();
    let r = source(&p);
    let mut session = r.metadata_session(&limits(&p)).unwrap();
    let error = session
        .visit_uels(|info| {
            if info.id.get() == 1 {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(p.destination())
                    .map_err(Error::from)?;
                file.write_all(&[0]).map_err(Error::from)?;
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::SourceChanged);
    assert_eq!(
        session.visit_statistics(),
        MetadataVisitStatistics {
            visited: 1,
            complete: false
        }
    );
    assert_eq!(session.find_uel(b"a").unwrap_err().kind, error.kind);
    session.abort().unwrap();
}

#[test]
fn domain_callbacks_respect_small_reader_batch_cap() {
    let p = Parent::new();
    drop(source(&p));
    let r = Reader::open(
        p.destination(),
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            maximum_batch_bytes: 64,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
    let mut map = r.mapped_uels(&limits(&p)).unwrap();
    map.register(
        [(1, 9), (2, 0)].map(|(r, m)| Ok((UelId::new(r).unwrap(), MappedUelId::new(m).unwrap()))),
    )
    .unwrap();
    for _ in 0..2 {
        let mut seen = vec![];
        let stats = map
            .visit_domain(&h, 0, |raw, mapped| {
                seen.push((raw.get(), mapped.get()));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, vec![(2, 0), (1, 9)]);
        assert_eq!(
            stats,
            MetadataVisitStatistics {
                visited: 2,
                complete: true
            }
        );
    }
    assert_eq!(
        map.mapped(UelId::new(1).unwrap()).unwrap().unwrap().get(),
        9
    );
    map.close().unwrap();
}

#[test]
fn domain_callbacks_detect_source_changes_during_replay_and_at_completion() {
    for (change_after, deny_cleanup) in [(1, false), (2, false), (1, true), (2, true)] {
        let reset = Reset::new();
        let p = Parent::new();
        let r = source(&p);
        let h = r.symbol(SymbolId::new(4).unwrap()).unwrap();
        let mut map = r.mapped_uels(&limits(&p)).unwrap();
        map.register(
            [(1, 9), (2, 0)]
                .map(|(r, m)| Ok((UelId::new(r).unwrap(), MappedUelId::new(m).unwrap()))),
        )
        .unwrap();
        let mut calls = 0;
        let mut cleanup = None;
        let error = map
            .visit_domain(&h, 0, |_, _| {
                calls += 1;
                if calls == change_after {
                    let mut file = fs::OpenOptions::new()
                        .append(true)
                        .open(p.destination())
                        .map_err(Error::from)?;
                    file.write_all(&[0]).map_err(Error::from)?;
                    if deny_cleanup {
                        cleanup = Some(crate::spill::test_cleanup_failure());
                        fault(
                            Event::MetadataRemove,
                            0,
                            true,
                            io::ErrorKind::PermissionDenied,
                        );
                    }
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::SourceChanged);
        assert_eq!(error.cleanup_error().is_some(), deny_cleanup);
        assert_eq!(calls, change_after);
        assert_eq!(
            map.domain_visit_statistics(),
            MetadataVisitStatistics {
                visited: change_after,
                complete: false
            }
        );
        assert_eq!(
            map.visit_domain(&h, 0, |_, _| panic!("must not replay"))
                .unwrap_err()
                .kind,
            error.kind
        );
        assert_eq!(map.domain_visit_statistics().visited, change_after);
        assert_eq!(
            map.mapped(UelId::new(1).unwrap()).unwrap_err().kind,
            error.kind
        );
        if deny_cleanup {
            assert!(map.abort().is_err());
        }
        drop(cleanup);
        drop(reset);
        map.abort().unwrap();
    }
}
