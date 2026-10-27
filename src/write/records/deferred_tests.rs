use super::*;
use crate::write::tests::{Parent, Reset, fault, options};
fn setup(p: &Parent) -> (Writer, WriterSymbolId, WriterSymbolId, [WriterUelId; 3]) {
    setup_limit(p, 64 * 1024 * 1024)
}
fn setup_limit(
    p: &Parent,
    quota: u64,
) -> (Writer, WriterSymbolId, WriterSymbolId, [WriterUelId; 3]) {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut limits = MetadataOptions::new(p.destination().parent().unwrap());
    limits.maximum_temporary_bytes = quota;
    limits.maximum_open_files = 8;
    limits.run_bytes = 512;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits).unwrap();
    // Forward metadata reference and forward input group, preserving P's ID.
    let par = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    let set = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Registered(set));
    w.register_symbol(&alias).unwrap();
    w.set_domains(
        par,
        &[
            WriteDomain::Strict(SymbolReference::Name(b"J")),
            WriteDomain::Strict(SymbolReference::Registered(set)),
        ],
    )
    .unwrap();
    let keys = [
        w.register_uel(b"a").unwrap(),
        w.register_uel(b"b").unwrap(),
        w.register_uel(b"c").unwrap(),
    ];
    (w, par, set, keys)
}
fn scan(path: &Path, symbol: u32) -> (Vec<Vec<u32>>, Vec<Value>) {
    let r = Reader::open(path, OpenOptions::default()).unwrap();
    let h = r.symbol(SymbolId::new(symbol).unwrap()).unwrap();
    let dim = h.metadata().unwrap().dimension;
    let mut scan = r.scan(&h, r.scan_options()).unwrap();
    let (mut keys, mut values) = (vec![], vec![]);
    while let Some(b) = scan.next_batch().unwrap() {
        for row in 0..b.len() {
            keys.push(
                (0..dim)
                    .map(|d| b.key_column(d).unwrap()[row].get())
                    .collect(),
            );
            values.push(b.value_column(ValueField::Level).unwrap()[row]);
        }
    }
    (keys, values)
}
#[test]
fn forward_membership_alias_identity_all_bad_axes_and_first_valid_input_win() {
    let p = Parent::new();
    let (mut w, par, set, k) = setup(&p);
    let root = Arc::clone(&w.budget);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    rows.write(&[k[1], k[2]], &[Value::Special(SpecialValue::NotAvailable)])
        .unwrap();
    rows.write(&[k[0], k[0]], &[Value::NumberBits((-0.0f64).to_bits())])
        .unwrap();
    rows.write(&[k[0], k[0]], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    rows.write(&[k[1], k[2]], &[Value::Special(SpecialValue::Undefined)])
        .unwrap();
    rows.begin_symbol(set).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    let mut errors = vec![];
    let stats = rows
        .finish_reporting(ErrorLimits::default(), |r| {
            errors.push((
                r.input_ordinal,
                r.symbol_ordinal,
                r.kind,
                r.raw_keys.iter().map(|k| k.get()).collect::<Vec<_>>(),
                r.values[0],
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (
            stats.input_records,
            stats.accepted_records,
            stats.symbols,
            stats.errors.reported
        ),
        (5, 2, 2, 3)
    );
    assert_eq!(
        errors,
        vec![
            (
                3,
                3,
                DataErrorKind::Duplicate,
                vec![1, 1],
                Value::Special(SpecialValue::Epsilon)
            ),
            (
                1,
                1,
                DataErrorKind::DomainViolation { dimensions: 3 },
                vec![2, 3],
                Value::Special(SpecialValue::NotAvailable)
            ),
            (
                4,
                4,
                DataErrorKind::DomainViolation { dimensions: 3 },
                vec![2, 3],
                Value::Special(SpecialValue::Undefined)
            )
        ]
    );
    assert!(w.metadata_statistics().peak_open_files <= 8);
    w.finish().unwrap();
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
    assert_eq!(
        scan(&p.destination(), 1),
        (
            vec![vec![1, 1]],
            vec![Value::NumberBits((-0.0f64).to_bits())]
        )
    );
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let h = r.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut domains = r.domains(&h).unwrap();
    assert_eq!(
        domains
            .next()
            .unwrap()
            .unwrap()
            .strict_symbol
            .unwrap()
            .get(),
        3
    );
}
#[test]
fn default_reject_and_callback_and_both_output_quotas_block_publication() {
    for mode in 0..4 {
        let p = Parent::new();
        let (mut w, par, set, k) = setup(&p);
        let mut rows = w.begin_deferred_records().unwrap();
        rows.begin_symbol(par).unwrap();
        rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
        rows.write(&[k[1], k[2]], &[Value::ZERO]).unwrap();
        rows.begin_symbol(set).unwrap();
        rows.write(&[k[0]], &[Value::ZERO]).unwrap();
        let mut calls = 0;
        let result = match mode {
            0 => rows.finish(),
            _ => rows.finish_reporting(
                ErrorLimits {
                    maximum_records: if mode == 1 { 0 } else { 10 },
                    maximum_bytes: if mode == 2 { 1 } else { 10000 },
                },
                |_| {
                    calls += 1;
                    Err(Error::new(ErrorKind::InvalidOptions, "callback refused"))
                },
            ),
        };
        let err = result.unwrap_err();
        assert_eq!(
            err.kind,
            if mode == 0 {
                ErrorKind::InvalidReference
            } else if mode < 3 {
                ErrorKind::DataErrorLimitExceeded
            } else {
                ErrorKind::InvalidOptions
            },
            "{err:?}"
        );
        assert_eq!(calls, if mode == 3 { 1 } else { 0 });
        assert!(!p.destination().exists());
        assert_eq!(w.validation_statistics().accepted_records, 2);
        assert_eq!(w.validation_statistics().errors.reported, 0);
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn direct_self_domain_universe_relaxed_empty_sets_and_dependency_cycles() {
    for cycle in [false, true] {
        let p = Parent::new();
        let (mut w, par, set, k) = setup(&p);
        if !cycle {
            w.set_domains(
                set,
                &[WriteDomain::Strict(SymbolReference::Registered(set))],
            )
            .unwrap();
        }
        let other = w
            .register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Set, 1))
            .unwrap();
        let relaxed = w
            .register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 1))
            .unwrap();
        w.set_domains(relaxed, &[WriteDomain::Relaxed(b"anything")])
            .unwrap();
        if cycle {
            w.set_domains(
                set,
                &[WriteDomain::Strict(SymbolReference::Registered(other))],
            )
            .unwrap();
            w.set_domains(
                other,
                &[WriteDomain::Strict(SymbolReference::Registered(set))],
            )
            .unwrap();
        }
        let mut rows = w.begin_deferred_records().unwrap();
        rows.begin_symbol(par).unwrap();
        rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
        rows.begin_symbol(set).unwrap();
        rows.write(&[k[0]], &[Value::ZERO]).unwrap();
        rows.begin_symbol(relaxed).unwrap();
        rows.write(&[k[2]], &[Value::ZERO]).unwrap();
        let result = rows.finish();
        if cycle {
            assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidReference);
            w.abort().unwrap();
            p.empty();
        } else {
            let stats = result.unwrap();
            assert_eq!((stats.symbols, stats.auto_empty_sets), (4, 1));
            w.finish().unwrap();
            assert_eq!(scan(&p.destination(), relaxed.get()).0, vec![vec![3]]);
        }
    }
}
#[test]
fn mapped_and_exact_string_diagnostics_retain_input_provenance() {
    let p = Parent::new();
    let (mut w, par, set, k) = setup(&p);
    w.register_mapped_uels([
        Ok((MappedUelId::new(0).unwrap(), b"b".as_slice())),
        Ok((MappedUelId::new(9).unwrap(), b"c".as_slice())),
    ])
    .unwrap();
    let strings = w
        .register_symbol(&WriteSymbol::new(b"S", SymbolType::Parameter, 1))
        .unwrap();
    w.set_domains(
        strings,
        &[WriteDomain::Strict(SymbolReference::Registered(set))],
    )
    .unwrap();
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    rows.write_mapped(
        &[MappedUelId::new(0).unwrap(), MappedUelId::new(9).unwrap()],
        &[Value::Special(SpecialValue::Epsilon)],
    )
    .unwrap();
    rows.begin_symbol(strings).unwrap();
    rows.write_strings(&[b"c"], &[Value::ZERO]).unwrap();
    rows.begin_symbol(set).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    let mut calls = 0;
    rows.finish_reporting(ErrorLimits::default(), |r| {
        calls += 1;
        if r.origin == DataOrigin::MappedWrite {
            assert_eq!(r.mapped_key(0).unwrap().get(), 0);
            assert_eq!(r.mapped_key(1).unwrap().get(), 9);
            assert_eq!(r.raw_keys[0].get(), 2);
        } else {
            assert_eq!(r.origin, DataOrigin::StringWrite);
            assert_eq!(r.label(0), Some(b"c".as_slice()));
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 2);
    w.finish().unwrap();
}
#[test]
fn callback_failure_preserves_secondary_cleanup_error_and_abort_retry() {
    let _reset = Reset::new();
    let p = Parent::new();
    let (mut w, par, set, k) = setup(&p);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    rows.write(&[k[1], k[2]], &[Value::ZERO]).unwrap();
    rows.begin_symbol(set).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    let err = rows
        .finish_reporting(ErrorLimits::default(), |_| {
            fault(
                Event::MetadataRemove,
                0,
                true,
                io::ErrorKind::PermissionDenied,
            );
            Err(Error::new(ErrorKind::InvalidOptions, "sink failed"))
        })
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::InvalidOptions);
    assert!(err.cleanup_error().is_some());
    assert!(!p.destination().exists());
    assert!(w.abort().is_err());
    drop(_reset);
    w.abort().unwrap();
    p.empty();
}
#[test]
fn rejected_scalar_duplicate_and_abandonment_are_explicit() {
    let p = Parent::new();
    let (mut w, _, _, _) = setup(&p);
    let scalar = w
        .register_symbol(&WriteSymbol::new(b"Scalar", SymbolType::Parameter, 0))
        .unwrap();
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(scalar).unwrap();
    rows.write(&[], &[Value::ZERO]).unwrap();
    rows.write(&[], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    let mut n = 0;
    rows.finish_reporting(ErrorLimits::default(), |r| {
        n += 1;
        assert_eq!(r.kind, DataErrorKind::Duplicate);
        assert!(r.raw_keys.is_empty());
        Ok(())
    })
    .unwrap();
    assert_eq!(n, 1);
    w.finish().unwrap();
    let p = Parent::new();
    let (mut w, _, _, _) = setup(&p);
    drop(w.begin_deferred_records().unwrap());
    assert_eq!(w.statistics().status, WriteStatus::Failed);
    w.abort().unwrap();
    p.empty();
}

#[test]
fn stage_corruption_scratch_handle_and_root_failures_never_publish() {
    let _reset = Reset::new();
    for mode in 0..4 {
        let p = Parent::new();
        let (mut w, par, _, k) = setup_limit(&p, if mode == 1 { 4096 } else { 64 * 1024 * 1024 });
        if mode == 0 {
            let available = w.budget_statistics();
            let reservation = w
                .budget
                .reserve(available.limit_bytes - available.current_bytes)
                .unwrap();
            assert_eq!(
                w.begin_deferred_records().err().unwrap().kind,
                ErrorKind::BudgetTooSmall
            );
            drop(reservation);
        } else {
            let mut rows = w.begin_deferred_records().unwrap();
            rows.begin_symbol(par).unwrap();
            if mode == 1 {
                let error = (0..100)
                    .find_map(|_| rows.write(&[k[0], k[0]], &[Value::ZERO]).err())
                    .expect("bounded staging quota must exhaust");
                assert_eq!(error.kind, ErrorKind::TemporarySpaceExceeded);
                drop(rows);
            } else {
                rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
                lock(&rows.store).unwrap().sync().unwrap();
                if mode == 2 {
                    let dir = fs::read_dir(p.destination().parent().unwrap())
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .find(|p| p.is_dir())
                        .unwrap();
                    fs::write(dir.join("deferred-records"), b"bad").unwrap();
                } else {
                    fault(Event::MetadataRead, 0, false, io::ErrorKind::StorageFull);
                }
                assert!(rows.finish().is_err());
            }
        }
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
    }
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = MetadataOptions::new(p.destination().parent().unwrap());
    o.maximum_open_files = 6;
    o.merge_fan_in = 2;
    w.begin_metadata(&o).unwrap();
    assert!(w.begin_deferred_records().is_err());
    w.abort().unwrap();
    p.empty();
}
#[test]
fn replay_primary_and_cursor_cleanup_diagnostics_survive_successful_parent_cleanup() {
    let p = Parent::new();
    let (mut w, par, _, k) = setup(&p);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
    // Empty domain set replays first. Inject cursor removal denial only when the
    // rejected dependent row reaches the callback, then fail that callback.
    let mut cleanup = None;
    let error = rows
        .finish_reporting(ErrorLimits::default(), |_| {
            cleanup = Some(crate::spill::test_cleanup_failure());
            Err(Error::new(ErrorKind::InvalidOptions, "sink failure"))
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidOptions);
    assert!(error.cleanup_error().is_some());
    assert_eq!(w.finish().unwrap_err().kind, error.kind);
    p.empty();
    drop(cleanup);
    w.abort().unwrap();
}
#[test]
fn partial_callbacks_and_wide_typed_records_have_exact_logical_byte_limits() {
    let p = Parent::new();
    let (mut w, _, set, k) = setup(&p);
    w.add_acronym(AcronymId::new(7).unwrap(), b"A", b"typed")
        .unwrap();
    let wide = w
        .register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 20))
        .unwrap();
    w.set_domains(
        wide,
        &[WriteDomain::Strict(SymbolReference::Registered(set)); 20],
    )
    .unwrap();
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(wide).unwrap();
    let values = [
        Value::Acronym {
            id: AcronymId::new(7).unwrap(),
            encoded_bits: (7.0 * 10.0e300f64).to_bits(),
        },
        Value::NumberBits((-0.0f64).to_bits()),
        Value::Special(SpecialValue::Undefined),
        Value::Special(SpecialValue::PositiveInfinity),
        Value::Special(SpecialValue::Epsilon),
    ];
    rows.write(&[k[1]; 20], &values).unwrap();
    rows.write(&[k[2]; 20], &values).unwrap();
    let mut bytes = 0;
    let error = rows
        .finish_reporting(
            ErrorLimits {
                maximum_records: 1,
                maximum_bytes: 1024,
            },
            |e| {
                assert_eq!(e.values, values);
                assert_eq!(
                    e.kind,
                    DataErrorKind::DomainViolation {
                        dimensions: (1 << 20) - 1
                    }
                );
                bytes = e.logical_bytes()?;
                assert_eq!(bytes, 139);
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::DataErrorLimitExceeded);
    let s = w.validation_statistics();
    assert_eq!(
        (s.errors.errors, s.errors.reported, s.errors.bytes),
        (2, 1, bytes)
    );
    w.abort().unwrap();
    p.empty();
}

#[test]
fn growing_dependent_input_spills_and_reports_without_an_error_collection() {
    let p = Parent::new();
    let (mut w, par, set, k) = setup(&p);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    for _ in 0..4096 {
        rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
    }
    rows.begin_symbol(set).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    let mut ordinal = 1;
    let stats = rows
        .finish_reporting(
            ErrorLimits {
                maximum_records: 4095,
                maximum_bytes: 1024 * 1024,
            },
            |r| {
                ordinal += 1;
                assert_eq!(r.input_ordinal, ordinal);
                assert_eq!(r.kind, DataErrorKind::Duplicate);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        (
            stats.input_records,
            stats.accepted_records,
            stats.errors.reported
        ),
        (4097, 2, 4095)
    );
    assert!(w.budget_statistics().peak_bytes <= 1024 * 1024);
    assert!(w.metadata_statistics().peak_open_files <= 8);
    w.finish().unwrap();
}

#[test]
fn output_failure_during_symbol_initialization_closes_sorter_before_parent_cleanup() {
    let p = Parent::new();
    let (mut w, _, set, k) = setup(&p);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(set).unwrap();
    rows.write(&[k[0]], &[Value::ZERO]).unwrap();
    rows.writer.limit = 126;
    let cleanup = crate::spill::test_cleanup_failure();
    let error = rows.finish().unwrap_err();
    assert_eq!(error.kind, ErrorKind::OutputLimitExceeded);
    assert!(error.cleanup_error().is_some());
    assert_eq!(w.finish().unwrap_err().kind, error.kind);
    p.empty();
    drop(cleanup);
    w.abort().unwrap();
}

#[test]
fn parent_cleanup_does_not_replace_the_first_cursor_cleanup_diagnostic() {
    let reset = Reset::new();
    let p = Parent::new();
    let (mut w, par, _, k) = setup(&p);
    let mut rows = w.begin_deferred_records().unwrap();
    rows.begin_symbol(par).unwrap();
    rows.write(&[k[0], k[0]], &[Value::ZERO]).unwrap();
    let mut cursor_cleanup = None;
    let error = rows
        .finish_reporting(ErrorLimits::default(), |_| {
            cursor_cleanup = Some(crate::spill::test_cleanup_failure());
            fault(
                Event::MetadataRemove,
                0,
                true,
                io::ErrorKind::PermissionDenied,
            );
            Err(Error::new(ErrorKind::InvalidOptions, "sink failed"))
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidOptions);
    let secondary = error.cleanup_error().unwrap().to_string();
    assert!(secondary.contains("cleanup denied"), "{secondary}");
    assert!(!secondary.contains("injected writer fault"), "{secondary}");
    assert!(w.abort().is_err());
    assert!(!p.destination().exists());
    drop(cursor_cleanup);
    drop(reset);
    w.abort().unwrap();
    p.empty();
}
