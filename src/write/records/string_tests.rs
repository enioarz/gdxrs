use super::*;
use crate::write::tests::{Parent, Reset, fault, options};
fn setup(p: &Parent) -> (Writer, WriterSymbolId, WriterSymbolId) {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut limits = MetadataOptions::new(p.destination().parent().unwrap());
    limits.maximum_open_files = 7;
    limits.run_bytes = 512;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits).unwrap();
    let i = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    let p = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    (w, i, p)
}
#[test]
fn first_encounters_across_symbols_preserve_existing_ids_bytes_and_shared_root() {
    let p = Parent::new();
    let (mut w, i, par) = setup(&p);
    w.register_uel(b"z").unwrap();
    w.register_uel(b"pre").unwrap();
    let root = Arc::clone(&w.budget);
    let mut rows = w.begin_string_records().unwrap();
    rows.begin_symbol(i).unwrap();
    for label in [b"a".as_slice(), b"A", b"\x80", b"z", b""] {
        rows.write(&[label], &[Value::ZERO]).unwrap();
    }
    rows.begin_symbol(par).unwrap();
    rows.write(&[b"\x80", b"a"], &[Value::NumberBits((-0.0f64).to_bits())])
        .unwrap();
    rows.write(&[b"new", b"A"], &[Value::Special(SpecialValue::Epsilon)])
        .unwrap();
    let stats = rows.finish().unwrap();
    assert_eq!(
        (
            stats.symbols,
            stats.records,
            stats.label_requests,
            stats.uels
        ),
        (2, 7, 9, 7)
    );
    assert!(stats.merge_passes >= 4);
    assert!(w.metadata_statistics().peak_open_files <= 7);
    w.finish().unwrap();
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let mut labels = r.uels().unwrap();
    for expected in [b"z".as_slice(), b"pre", b"a", b"A", b"\x80", b"", b"new"] {
        assert_eq!(labels.next().unwrap().unwrap().label, expected);
    }
    assert!(labels.next().unwrap().is_none());
    let h = r.symbol(SymbolId::new(par.get()).unwrap()).unwrap();
    let mut scan = r.scan(&h, r.scan_options()).unwrap();
    let b = scan.next_batch().unwrap().unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(b.key_column(0).unwrap()[0].get(), 5);
    assert_eq!(
        b.value_column(ValueField::Level).unwrap()[0],
        Value::NumberBits((-0.0f64).to_bits())
    );
}
#[test]
fn streamed_dictionary_growth_needs_no_retained_tokens() {
    let p = Parent::new();
    let (mut w, i, _) = setup(&p);
    let mut rows = w.begin_string_records().unwrap();
    rows.begin_symbol(i).unwrap();
    // Fixed caller buffer; thousands of labels, one-record runs and multiple merges.
    for n in 0u32..2048 {
        let text = format!("L{:08}", 2047 - n);
        rows.write(&[text.as_bytes()], &[Value::ZERO]).unwrap();
    }
    let stats = rows.finish().unwrap();
    assert_eq!((stats.records, stats.uels), (2048, 2048));
    assert!(stats.merge_passes >= 20);
    assert!(w.metadata_statistics().peak_open_files <= 7);
    assert!(w.budget.statistics().peak_bytes <= 1024 * 1024);
    w.finish().unwrap();
}
#[test]
fn duplicates_scalars_domains_and_invalid_values_fail_terminally() {
    for mode in 0..5 {
        let p = Parent::new();
        let (mut w, i, par) = setup(&p);
        let scalar = w
            .register_symbol(&WriteSymbol::new(b"S", SymbolType::Parameter, 0))
            .unwrap();
        if mode == 2 || mode == 3 {
            w.set_domains(
                par,
                &[
                    WriteDomain::Strict(SymbolReference::Registered(i)),
                    WriteDomain::Strict(SymbolReference::Universe),
                ],
            )
            .unwrap();
        }
        let mut rows = w.begin_string_records().unwrap();
        if mode == 3 {
            rows.begin_symbol(i).unwrap();
            rows.write(&[b"member"], &[Value::ZERO]).unwrap();
        }
        rows.begin_symbol(if mode == 1 { scalar } else { par })
            .unwrap();
        let keys = if mode == 1 {
            &[][..]
        } else {
            &[b"other".as_slice(), b"x"][..]
        };
        rows.write(
            keys,
            &[if mode == 4 {
                Value::Acronym {
                    id: AcronymId::new(7).unwrap(),
                    encoded_bits: 1,
                }
            } else {
                Value::ZERO
            }],
        )
        .unwrap();
        if mode <= 1 {
            rows.write(keys, &[Value::NumberBits(1.0f64.to_bits())])
                .unwrap();
        }
        assert!(rows.finish().is_err());
        assert!(!w.statistics().published);
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn admission_arity_labels_and_abandonment_are_terminal() {
    for mode in 0..5 {
        let p = Parent::new();
        let (mut w, i, _) = setup(&p);
        let mut rows = w.begin_string_records().unwrap();
        if mode == 0 {
            assert!(rows.write(&[b"a"], &[Value::ZERO]).is_err());
        } else {
            rows.begin_symbol(i).unwrap();
            if mode == 1 {
                assert!(rows.write(&[], &[Value::ZERO]).is_err());
            }
            if mode == 2 {
                assert!(rows.write(&[b"a"], &[]).is_err());
            }
            if mode == 3 {
                assert!(rows.write(&[b"\0"], &[Value::ZERO]).is_err());
            }
        }
        drop(rows);
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn spool_corruption_io_and_cleanup_retry_retain_ownership() {
    let _reset = Reset::new();
    for mode in 0..3 {
        let p = Parent::new();
        let (mut w, i, _) = setup(&p);
        let mut rows = w.begin_string_records().unwrap();
        rows.begin_symbol(i).unwrap();
        rows.write(&[b"x"], &[Value::ZERO]).unwrap();
        if mode == 0 {
            lock(&rows.store).unwrap().sync().unwrap();
            let dir = fs::read_dir(p.destination().parent().unwrap())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| p.is_dir())
                .unwrap();
            fs::write(dir.join("string-records"), b"bad").unwrap();
        } else if mode == 1 {
            fault(Event::MetadataWrite, 0, true, io::ErrorKind::StorageFull);
        } else {
            fault(
                Event::MetadataRemove,
                0,
                true,
                io::ErrorKind::PermissionDenied,
            );
        }
        let error = rows.finish().unwrap_err();
        assert!(!w.statistics().published);
        if mode == 2 {
            assert!(error.cleanup_error().is_some());
            assert!(w.abort().is_err());
        }
        Reset::new();
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn scratch_remap_root_and_handle_caps_are_enforced() {
    for mode in 0..3 {
        let p = Parent::new();
        let mut w = Writer::create(p.destination(), options()).unwrap();
        let mut limits = MetadataOptions::new(p.destination().parent().unwrap());
        limits.run_bytes = 512;
        limits.merge_fan_in = 2;
        limits.maximum_open_files = if mode == 0 { 6 } else { 7 };
        if mode == 1 {
            limits.maximum_temporary_bytes = 512;
        }
        w.begin_metadata(&limits).unwrap();
        let i = w
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        let held = if mode == 2 {
            Some(
                w.budget
                    .reserve(
                        w.budget.statistics().limit_bytes - w.budget.statistics().current_bytes,
                    )
                    .unwrap(),
            )
        } else {
            None
        };
        let result = w.begin_string_records();
        if mode != 1 {
            assert!(result.is_err());
            drop(result);
        } else {
            let mut rows = result.unwrap();
            rows.begin_symbol(i).unwrap();
            rows.write(&[b"a"], &[Value::ZERO]).unwrap();
            rows.write(&[b"b"], &[Value::ZERO]).unwrap();
            assert_eq!(
                rows.finish().unwrap_err().kind,
                ErrorKind::TemporarySpaceExceeded
            );
        }
        drop(held);
        w.abort().unwrap();
        p.empty();
    }
}

#[test]
fn scalar_empty_maximum_label_shapes_and_typed_payloads() {
    let p = Parent::new();
    let (mut w, _, _) = setup(&p);
    w.add_acronym(AcronymId::new(7).unwrap(), b"Acro", b"typed")
        .unwrap();
    let mut symbols = Vec::new(); // fixed 16-symbol matrix, test-only
    for dim in [0, 1, 2, 20] {
        for kind in [
            SymbolType::Set,
            SymbolType::Parameter,
            SymbolType::Variable,
            SymbolType::Equation,
        ] {
            let name = format!("S{}", symbols.len());
            let mut spec = WriteSymbol::new(name.as_bytes(), kind, dim);
            spec.user_information = if kind == SymbolType::Equation { 53 } else { 0 };
            symbols.push((w.register_symbol(&spec).unwrap(), kind, dim));
        }
    }
    let empty = w
        .register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Parameter, 0))
        .unwrap();
    let mut rows = w.begin_string_records().unwrap();
    for (id, kind, dim) in &symbols {
        rows.begin_symbol(*id).unwrap();
        let values = if *kind == SymbolType::Set {
            [Value::ZERO; 5]
        } else {
            [
                Value::NumberBits(0x7ff8_0000_0000_0042),
                Value::NumberBits((-0.0f64).to_bits()),
                Value::Special(SpecialValue::Undefined),
                Value::Special(SpecialValue::Epsilon),
                Value::Acronym {
                    id: AcronymId::new(7).unwrap(),
                    encoded_bits: (7.0 * 10.0e300f64).to_bits(),
                },
            ]
        };
        rows.write(
            &[&[b'x'; 63][..]; 20][..*dim as usize],
            &values[..kind.fields()],
        )
        .unwrap();
    }
    rows.begin_symbol(empty).unwrap();
    let stats = rows.finish().unwrap();
    assert_eq!(stats.records, 16);
    assert_eq!(stats.uels, 1);
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    for (id, kind, _) in symbols {
        let h = r.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
        let mut c = r.scan(&h, r.scan_options()).unwrap();
        let b = c.next_batch().unwrap().unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(
            b.value_column(ValueField::Level).unwrap()[0],
            if kind == SymbolType::Set {
                Value::ZERO
            } else {
                Value::NumberBits(0x7ff8_0000_0000_0042)
            }
        );
    }
}

#[test]
fn spill_io_secondary_errors_and_abort_retry_close_string_spool() {
    let _reset = Reset::new();
    for read in [false, true] {
        let p = Parent::new();
        let (mut w, i, _) = setup(&p);
        let mut rows = w.begin_string_records().unwrap();
        rows.begin_symbol(i).unwrap();
        rows.write(&[b"x"], &[Value::ZERO]).unwrap();
        let io = crate::spill::test_io_failure(read);
        let cleanup = crate::spill::test_cleanup_failure();
        fault(
            Event::MetadataRemove,
            0,
            true,
            io::ErrorKind::PermissionDenied,
        );
        let error = rows.finish().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Io);
        assert!(error.cleanup_error().is_some());
        assert!(w.abort().is_err());
        assert!(!w.statistics().published);
        drop(io);
        drop(cleanup);
        Reset::new();
        w.abort().unwrap();
        p.empty();
    }
}
