use super::*;
use crate::write::tests::{Parent, Reset, fault, options};
fn setup(
    p: &Parent,
    compression: WriteCompression,
) -> (Writer, WriterUelId, WriterUelId, WriterSymbolId) {
    let mut w = Writer::create(
        p.destination(),
        WriteOptions {
            compression,
            ..options()
        },
    )
    .unwrap();
    let mut limits = MetadataOptions::new(p.destination().parent().unwrap());
    limits.run_bytes = 512;
    limits.maximum_open_files = 6;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits).unwrap();
    let a = w.register_uel(b"a").unwrap();
    let b = w.register_uel(b"b").unwrap();
    let id = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))
        .unwrap();
    (w, a, b, id)
}
#[test]
fn tiny_runs_merge_complete_tuples_in_raw_order_and_release_shared_root() {
    let _reset = Reset::new();
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if compression == WriteCompression::Zlib && !cfg!(feature = "compression") {
            continue;
        }
        let p = Parent::new();
        let (mut w, a, b, id) = setup(&p, compression);
        let root = Arc::clone(&w.budget);
        let mut rows = w
            .begin_unsorted_symbol(
                id,
                Some(
                    &[KeyBounds {
                        minimum: 1,
                        maximum: 2,
                    }; 2],
                ),
            )
            .unwrap();
        for (keys, value) in [([b, b], 4.0f64), ([a, b], 2.), ([b, a], 3.), ([a, a], 1.)] {
            rows.write(&keys, &[Value::NumberBits(value.to_bits())])
                .unwrap();
        }
        let stats = rows.finish().unwrap();
        assert_eq!(stats.records, 4);
        assert!(stats.spill.complete);
        assert!(stats.spill.merge_passes >= 2);
        assert_eq!(stats.spill.temporary_bytes, 0);
        assert_eq!(stats.spill.open_files, 0);
        assert!(stats.spill.peak_open_files <= 3);
        w.finish().unwrap();
        drop(w);
        assert_eq!(root.statistics().current_bytes, 0);
        let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
        let h = r.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
        let mut scan = r.scan(&h, r.scan_options()).unwrap();
        let mut ordinal = 0;
        while let Some(batch) = scan.next_batch().unwrap() {
            for n in 0..batch.len() {
                ordinal += 1;
                assert_eq!(batch.key_column(0).unwrap()[n].get(), (ordinal - 1) / 2 + 1);
                assert_eq!(batch.key_column(1).unwrap()[n].get(), (ordinal - 1) % 2 + 1);
                assert_eq!(
                    batch.value_column(ValueField::Level).unwrap()[n],
                    Value::NumberBits((ordinal as f64).to_bits())
                );
            }
        }
        assert_eq!(ordinal, 4);
    }
}
#[test]
fn all_shapes_subtypes_max_payload_and_typed_bits_roundtrip() {
    let _reset = Reset::new();
    let p = Parent::new();
    let (mut w, a, _, _) = setup(&p, WriteCompression::None);
    let acro = AcronymId::new(7).unwrap();
    w.add_acronym(acro, b"Acro", b"typed").unwrap();
    let mut symbols = Vec::new(); // fixed test matrix: 76 symbols
    for dim in [0, 1, 2, 20] {
        for (kind, start, end) in [
            (SymbolType::Set, 0, 0),
            (SymbolType::Parameter, 0, 0),
            (SymbolType::Variable, 0, 9),
            (SymbolType::Equation, 53, 59),
        ] {
            for user in start..=end {
                let name = format!("S{}", symbols.len());
                let mut spec = WriteSymbol::new(name.as_bytes(), kind, dim);
                spec.user_information = user;
                symbols.push((w.register_symbol(&spec).unwrap(), kind, dim));
            }
        }
    }
    for (id, kind, dim) in &symbols {
        let values = if *kind == SymbolType::Set {
            [Value::ZERO; 5]
        } else {
            [Value::Acronym {
                id: acro,
                encoded_bits: (7.0 * 10.0e300f64).to_bits(),
            }; 5]
        };
        let mut rows = w.begin_unsorted_symbol(*id, None).unwrap();
        rows.write(&[a; 20][..*dim as usize], &values[..kind.fields()])
            .unwrap();
        assert_eq!(rows.finish().unwrap().records, 1);
    }
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    for (id, kind, dim) in symbols {
        let h = r.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
        assert_eq!(h.metadata().unwrap().dimension, dim);
        let mut c = r.scan(&h, r.scan_options()).unwrap();
        let b = c.next_batch().unwrap().unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(
            b.value_column(ValueField::Level).unwrap()[0],
            if kind == SymbolType::Set {
                Value::ZERO
            } else {
                Value::Acronym {
                    id: acro,
                    encoded_bits: (7.0 * 10.0e300f64).to_bits(),
                }
            }
        );
    }
}
#[test]
fn duplicate_keys_different_values_and_scalar_multiplicity_are_terminal() {
    for scalar in [false, true] {
        let p = Parent::new();
        let (mut w, a, _, id) = setup(&p, WriteCompression::None);
        let id = if scalar {
            w.register_symbol(&WriteSymbol::new(b"Scalar", SymbolType::Parameter, 0))
                .unwrap()
        } else {
            id
        };
        let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
        let keys = [a, a];
        let keys = if scalar { &keys[..0] } else { &keys[..] };
        rows.write(keys, &[Value::ZERO]).unwrap();
        rows.write(keys, &[Value::NumberBits(9f64.to_bits())])
            .unwrap();
        assert_eq!(rows.finish().unwrap_err().kind, ErrorKind::InvalidKeyTag);
        assert!(w.finish().is_err());
        p.empty();
    }
}
#[test]
fn malformed_input_domain_bounds_and_allocation_are_terminal() {
    let _reset = Reset::new();
    for mode in 0..6 {
        let p = Parent::new();
        let (mut w, a, b, id) = setup(&p, WriteCompression::None);
        if mode == 5 {
            crate::budget::inject_buffer_failure();
            assert_eq!(
                w.begin_unsorted_symbol(id, None).err().unwrap().kind,
                ErrorKind::AllocationFailed
            );
            p.empty();
            continue;
        }
        let mut rows = w
            .begin_unsorted_symbol(
                id,
                Some(
                    &[KeyBounds {
                        minimum: 1,
                        maximum: 1,
                    }; 2],
                ),
            )
            .unwrap();
        let result = match mode {
            0 => rows.write(&[a], &[Value::ZERO]),
            1 => rows.write(&[b, a], &[Value::ZERO]),
            2 => rows.write(&[a, a], &[]),
            3 => {
                rows.input = i32::MAX as u32;
                rows.write(&[a, a], &[Value::ZERO])
            }
            _ => {
                let other = Parent::new();
                let (_, key, _, _) = setup(&other, WriteCompression::None);
                rows.write(&[key, a], &[Value::ZERO])
            }
        };
        assert!(result.is_err());
        drop(rows);
        assert!(w.finish().is_err());
        p.empty();
    }
    let p = Parent::new();
    let (mut w, a, b, id) = setup(&p, WriteCompression::None);
    let set = w
        .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
        .unwrap();
    w.set_domains(
        id,
        &[
            WriteDomain::Strict(SymbolReference::Registered(set)),
            WriteDomain::Strict(SymbolReference::Universe),
        ],
    )
    .unwrap();
    let mut out = w.begin_unsorted_symbol(set, None).unwrap();
    out.write(&[a], &[Value::ZERO]).unwrap();
    out.finish().unwrap();
    let mut out = w.begin_unsorted_symbol(id, None).unwrap();
    assert!(out.write(&[b, a], &[Value::ZERO]).is_err());
    drop(out);
    p.empty();
}
#[test]
fn abandoned_replay_limit_and_corrupt_runs_never_publish() {
    let _reset = Reset::new();
    for mode in 0..3 {
        let p = Parent::new();
        let (mut w, a, b, id) = setup(&p, WriteCompression::None);
        let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
        rows.write(&[b, b], &[Value::ZERO]).unwrap();
        rows.write(&[a, a], &[Value::ZERO]).unwrap();
        if mode == 0 {
            drop(rows);
        } else if mode == 1 {
            let dir = rows.sorter.as_ref().unwrap().directory();
            let entry = fs::read_dir(dir).unwrap().next().unwrap().unwrap();
            fs::write(entry.path(), b"bad").unwrap();
            assert!(rows.finish().is_err());
        } else {
            rows.inner.as_mut().unwrap().writer.limit = 126;
            assert!(rows.finish().is_err());
        }
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn merge_io_and_cleanup_failures_keep_retryable_parent_ownership() {
    let _reset = Reset::new();
    for read in [false, true] {
        let p = Parent::new();
        let (mut w, a, b, id) = setup(&p, WriteCompression::None);
        let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
        for key in [[b, b], [b, a], [a, b], [a, a]] {
            rows.write(&key, &[Value::ZERO]).unwrap();
        }
        let dir = rows.sorter.as_ref().unwrap().directory().to_path_buf();
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
        assert!(dir.exists());
        assert!(!w.statistics().published);
        assert!(w.abort().is_err());
        drop(io);
        drop(cleanup);
        Reset::new();
        w.abort().unwrap();
        assert!(!dir.exists());
        p.empty();
    }
}
#[test]
fn malformed_binary_payloads_are_rejected_without_allocation() {
    let p = Parent::new();
    let (mut w, a, _, id) = setup(&p, WriteCompression::None);
    let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
    let inner = rows.inner.as_ref().unwrap();
    for bytes in [vec![], vec![0; 8], vec![255; 9], vec![0; 146], {
        let mut p = Packet {
            bytes: [0; 145],
            length: 0,
        };
        p.bytes(&a.get().to_be_bytes()).unwrap();
        p.bytes(&a.get().to_be_bytes()).unwrap();
        p.bytes(&[7]).unwrap();
        p.bytes[..p.length].to_vec()
    }] {
        assert!(decode(&bytes, &mut rows.keys, inner).is_err());
    }
    drop(rows);
    p.empty();
}

#[test]
fn scratch_and_root_quotas_are_not_multiplied_or_raised() {
    let _reset = Reset::new();
    let p = Parent::new();
    let (mut w, _, _, id) = setup(&p, WriteCompression::None);
    let available = w.budget_statistics();
    let reservation = w
        .budget
        .reserve(available.limit_bytes - available.current_bytes)
        .unwrap();
    assert_eq!(
        w.begin_unsorted_symbol(id, None).err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    drop(reservation);
    w.abort().unwrap();
    p.empty();
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut limits = MetadataOptions::new(p.destination().parent().unwrap());
    limits.run_bytes = 512;
    limits.maximum_temporary_bytes = 4096;
    limits.maximum_open_files = 6;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits).unwrap();
    let key = w.register_uel(b"a").unwrap();
    let id = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
    let mut failure = None;
    for _ in 0..100 {
        if let Err(e) = rows.write(&[key], &[Value::ZERO]) {
            failure = Some(e);
            break;
        }
    }
    assert_eq!(failure.unwrap().kind, ErrorKind::TemporarySpaceExceeded);
    drop(rows);
    assert!(w.finish().is_err());
    assert!(w.metadata_statistics().peak_temporary_bytes <= 4096);
    p.empty();
}
#[test]
fn scalar_empty_defaults_and_ordinary_nan_bits_survive_sorted_replay() {
    let p = Parent::new();
    let (mut w, a, b, _) = setup(&p, WriteCompression::None);
    let scalar = w
        .register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Variable, 0))
        .unwrap();
    let s = w
        .register_symbol(&WriteSymbol::new(b"Bits", SymbolType::Parameter, 1))
        .unwrap();
    assert_eq!(
        w.begin_unsorted_symbol(scalar, None)
            .unwrap()
            .finish()
            .unwrap()
            .records,
        0
    );
    let values = [
        Value::NumberBits((-0.0f64).to_bits()),
        Value::NumberBits(0x7ff8000000001234),
    ];
    let mut rows = w.begin_unsorted_symbol(s, None).unwrap();
    rows.write(&[b], &values[1..]).unwrap();
    rows.write(&[a], &values[..1]).unwrap();
    rows.finish().unwrap();
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let h = r.symbol(SymbolId::new(s.get()).unwrap()).unwrap();
    let mut c = r.scan(&h, r.scan_options()).unwrap();
    let batch = c.next_batch().unwrap().unwrap();
    assert_eq!(batch.value_column(ValueField::Level).unwrap(), &values);
    let h = r.symbol(SymbolId::new(scalar.get()).unwrap()).unwrap();
    let mut c = r.scan(&h, r.scan_options()).unwrap();
    assert_eq!(c.next_batch().unwrap().unwrap().len(), 1);
}
