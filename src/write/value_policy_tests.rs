use crate::write::Event;
use crate::write::tests::{Parent, Reset, fault, options};
use crate::*;
use std::{fs, io, sync::Arc};
const CUSTOM: [f64; 5] = [-101.0, -102.0, -103.0, -104.0, -105.0];
fn bits() -> [u64; 5] {
    CUSTOM.map(f64::to_bits)
}
fn limits(p: &Parent) -> MetadataOptions {
    let mut o = MetadataOptions::new(p.destination().parent().unwrap());
    o.maximum_open_files = 8;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    o
}
fn writer(p: &Parent) -> Writer {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    w.begin_metadata(&limits(p)).unwrap();
    w
}
fn source(p: &Parent) -> Reader {
    let mut w = writer(p);
    w.add_acronym(AcronymId::new(7).unwrap(), b"Old", b"before")
        .unwrap();
    w.update_acronym(AcronymId::new(7).unwrap(), b"Alpha", b"after and longer")
        .unwrap();
    w.add_acronym(AcronymId::new(9).unwrap(), b"Beta", b"second")
        .unwrap();
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
fn bitwise_sentinels_ieee_preservation_explicit_native_normalization_and_finite_acronyms() {
    let nan = 0x7ff8000000001234;
    let map = SpecialValueMap::new([
        nan,
        (-0.0f64).to_bits(),
        0,
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
    ])
    .unwrap();
    let p = WriteValuePolicy::new(map, IeeeWritePolicy::Preserve);
    for (i, s) in [
        SpecialValue::Undefined,
        SpecialValue::NotAvailable,
        SpecialValue::PositiveInfinity,
        SpecialValue::NegativeInfinity,
        SpecialValue::Epsilon,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(p.decode_bits(map.bits()[i]).unwrap(), Value::Special(s));
    }
    assert!(SpecialValueMap::new([0; 5]).is_err());
    for b in [
        nan,
        (-0.0f64).to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
    ] {
        assert_eq!(
            WriteValuePolicy::default().decode_bits(b).unwrap(),
            Value::NumberBits(b)
        );
    }
    let native =
        WriteValuePolicy::new(SpecialValueMap::default(), IeeeWritePolicy::NormalizeNative);
    assert_eq!(
        native.decode_bits(nan).unwrap(),
        Value::Special(SpecialValue::NotAvailable)
    );
    assert_eq!(
        native.decode_bits((-0.0f64).to_bits()).unwrap(),
        Value::ZERO
    );
    assert_eq!(
        native.decode_bits(f64::INFINITY.to_bits()).unwrap(),
        Value::Special(SpecialValue::PositiveInfinity)
    );
    for id in [1, 7, 9, 17_976_931] {
        let id = AcronymId::new(id).unwrap();
        assert!(matches!(id.value().unwrap(),Value::Acronym{id:x,..} if x==id));
    }
    assert_eq!(
        AcronymId::new(17_976_932)
            .unwrap()
            .value()
            .unwrap_err()
            .kind,
        ErrorKind::InvalidAcronymId
    );
    assert!(AcronymId::new(i32::MAX as u32).unwrap().value().is_err());
}
#[test]
fn projection_keeps_identity_and_sessions_independent_and_native_na_map_precedence() {
    let p = Parent::new();
    let r = source(&p);
    let root = Arc::clone(&r.source.budget);
    let baseline = r.budget_statistics().current_bytes;
    let mut a = r.value_session(&limits(&p)).unwrap();
    let mut b = r.value_session(&limits(&p)).unwrap();
    a.set_special_values(bits()).unwrap();
    a.set_read_special_values([(-999.0f64).to_bits(); 5], ReadCollisions::AllowLoss)
        .unwrap();
    for special in [
        SpecialValue::Undefined,
        SpecialValue::NotAvailable,
        SpecialValue::PositiveInfinity,
        SpecialValue::NegativeInfinity,
        SpecialValue::Epsilon,
    ] {
        let original = Value::Special(special);
        let mapped = a.project(original).unwrap();
        assert_eq!(mapped.original, original);
        assert!(mapped.to_f64(FloatConversion::Exact).is_err());
        assert_eq!(mapped.to_f64(FloatConversion::AllowLoss).unwrap(), -999.0);
    }
    for bits in [
        (-0.0f64).to_bits(),
        0x7ff8000000001234,
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        SpecialValueMap::default().bits()[0],
    ] {
        let v = Value::NumberBits(bits);
        assert_eq!(
            a.project(v)
                .unwrap()
                .to_f64(FloatConversion::Exact)
                .unwrap()
                .to_bits(),
            bits
        );
    }
    a.set_acronym_policy(AcronymReadPolicy::ToNotAvailable)
        .unwrap();
    let v = AcronymId::new(7).unwrap().value().unwrap();
    let mapped = a.project(v).unwrap();
    assert_eq!(mapped.original, v);
    assert_eq!(
        mapped.to_f64(FloatConversion::AllowLoss).unwrap(),
        CUSTOM[1]
    );
    assert_eq!(a.next_acronym_id(), None);
    assert_eq!(
        b.project(Value::Special(SpecialValue::NotAvailable))
            .unwrap()
            .to_f64(FloatConversion::AllowLoss)
            .unwrap()
            .to_bits(),
        SpecialValueMap::default().bits()[1]
    );
    a.reset_special_values().unwrap();
    assert_eq!(a.special_values(), SpecialValueMap::default());
    a.close().unwrap();
    b.close().unwrap();
    drop(a);
    drop(b);
    assert_eq!(root.statistics().current_bytes, baseline);
}
#[test]
fn explicit_auto_identity_mapping_next_ids_resets_information_and_collisions() {
    let p = Parent::new();
    let r = source(&p);
    let mut s = r.value_session(&limits(&p)).unwrap();
    let (id7, id9) = (AcronymId::new(7).unwrap(), AcronymId::new(9).unwrap());
    let info = s.acronym_information(id7).unwrap().unwrap();
    assert_eq!(
        (info.name(), info.text()),
        (b"Alpha".as_slice(), b"after and longer".as_slice())
    );
    assert!(
        s.acronym_information(AcronymId::new(31).unwrap())
            .unwrap()
            .is_none()
    );
    s.set_acronym_mapping(id7, AcronymId::new(70).unwrap())
        .unwrap();
    s.set_next_acronym_id(Some(AcronymId::new(100).unwrap()))
        .unwrap();
    assert_eq!(
        s.project(id7.value().unwrap())
            .unwrap()
            .acronym_mapping
            .unwrap(),
        AcronymMapping {
            original: id7,
            mapped: AcronymId::new(70).unwrap(),
            automatic: false
        }
    );
    assert!(
        s.project(id9.value().unwrap())
            .unwrap()
            .acronym_mapping
            .unwrap()
            .automatic
    );
    assert_eq!(s.next_acronym_id().unwrap().get(), 101);
    s.project(id9.value().unwrap()).unwrap();
    assert_eq!(s.next_acronym_id().unwrap().get(), 101);
    s.reset_special_values().unwrap();
    assert_eq!(s.acronym_mapping(id9).unwrap().unwrap().mapped.get(), 100);
    let used = s.statistics().temporary_bytes;
    s.reset_acronyms().unwrap();
    assert!(s.acronym_mapping(id7).unwrap().is_none());
    assert!(s.statistics().temporary_bytes >= used);
    assert_eq!(
        s.project(id7.value().unwrap())
            .unwrap()
            .acronym_mapping
            .unwrap()
            .mapped,
        id7
    );
    assert_eq!(
        s.set_acronym_mapping(id9, id7).unwrap_err().kind,
        ErrorKind::InvalidReference
    );
    assert!(s.project(Value::ZERO).is_err());
    assert!(s.reset_acronyms().is_err());
    s.abort().unwrap();
    let mut s = r.value_session(&limits(&p)).unwrap();
    s.set_acronym_mapping(id7, id9).unwrap();
    s.set_next_acronym_id(Some(id9)).unwrap();
    assert!(s.project(id9.value().unwrap()).is_err());
    s.abort().unwrap();
}
#[test]
fn mapped_policies_all_writer_adapters_share_the_transaction_policy() {
    for mode in 0..5 {
        let p = Parent::new();
        let mut w = writer(&p);
        w.set_write_special_values(bits()).unwrap();
        w.add_acronym(AcronymId::new(7).unwrap(), b"Alpha", b"typed")
            .unwrap();
        let id = w
            .register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 1))
            .unwrap();
        w.register_mapped_uels([Ok((MappedUelId::new(0).unwrap(), b"a"))])
            .unwrap();
        let key = w.raw_id_for_mapped(MappedUelId::new(0).unwrap()).unwrap();
        let values = [CUSTOM[0], CUSTOM[1], CUSTOM[2], CUSTOM[3], CUSTOM[4]];
        match mode {
            0 => {
                let mut rows = w.begin_symbol(id, None).unwrap();
                rows.write_f64(&[key], &values).unwrap();
                rows.finish().unwrap();
            }
            1 => {
                let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
                rows.write_f64(&[key], &values).unwrap();
                rows.finish().unwrap();
            }
            2 => {
                let mut rows = w.begin_mapped_symbol(id).unwrap();
                rows.write_f64(&[MappedUelId::new(0).unwrap()], &values)
                    .unwrap();
                rows.finish().unwrap();
            }
            3 => {
                let mut rows = w.begin_string_records().unwrap();
                rows.begin_symbol(id).unwrap();
                rows.write_f64(&[b"a"], &values).unwrap();
                rows.finish().unwrap();
            }
            _ => {
                let mut rows = w.begin_deferred_records().unwrap();
                rows.begin_symbol(id).unwrap();
                rows.write_mapped_f64(&[MappedUelId::new(0).unwrap()], &values)
                    .unwrap();
                rows.finish().unwrap();
            }
        }
        w.finish().unwrap();
        let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
        let h = r.symbol(SymbolId::new(1).unwrap()).unwrap();
        let mut scan = r.scan(&h, r.scan_options()).unwrap();
        let b = scan.next_batch().unwrap().unwrap();
        for (f, s) in ValueField::ALL.into_iter().zip([
            SpecialValue::Undefined,
            SpecialValue::NotAvailable,
            SpecialValue::PositiveInfinity,
            SpecialValue::NegativeInfinity,
            SpecialValue::Epsilon,
        ]) {
            assert_eq!(b.value_column(f).unwrap(), &[Value::Special(s)]);
        }
    }
}
#[test]
fn policy_conversion_and_invalid_configuration_block_publication_and_stay_sticky() {
    for mode in 0..3 {
        let p = Parent::new();
        let mut w = writer(&p);
        let id = w
            .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 0))
            .unwrap();
        let error = if mode == 0 {
            w.set_write_special_values([0; 5]).unwrap_err()
        } else {
            let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
            let e = rows
                .write_f64(&[], if mode == 1 { &[0.0; 6] } else { &[f64::MAX] })
                .unwrap_err();
            assert_eq!(rows.write_f64(&[], &[0.0]).unwrap_err().kind, e.kind);
            drop(rows);
            e
        };
        assert!(matches!(
            error.kind,
            ErrorKind::InvalidOptions
                | ErrorKind::InvalidDimension
                | ErrorKind::InvalidAcronymId
                | ErrorKind::InvalidCount
        ));
        assert!(!p.destination().exists());
        assert_eq!(w.finish().unwrap_err().kind, error.kind);
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn journal_quota_root_corruption_cleanup_failures_and_retry_preserve_original_errors() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let mut o = limits(&p);
    o.maximum_temporary_bytes = 128;
    let mut s = r.value_session(&o).unwrap();
    s.project(AcronymId::new(1).unwrap().value().unwrap())
        .unwrap();
    s.project(AcronymId::new(2).unwrap().value().unwrap())
        .unwrap();
    s.project(AcronymId::new(3).unwrap().value().unwrap())
        .unwrap();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let e = s
        .project(AcronymId::new(4).unwrap().value().unwrap())
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::TemporarySpaceExceeded);
    assert!(e.cleanup_error().is_some());
    assert_eq!(s.project(Value::ZERO).unwrap_err().kind, e.kind);
    assert!(s.close().is_err());
    drop(reset);
    s.abort().unwrap();
    let mut s = r.value_session(&limits(&p)).unwrap();
    s.set_acronym_mapping(AcronymId::new(7).unwrap(), AcronymId::new(70).unwrap())
        .unwrap();
    s.acronym_mapping(AcronymId::new(7).unwrap()).unwrap(); // flush the admitted append buffer before physical corruption
    let dir = fs::read_dir(p.destination().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap();
    fs::write(dir.join("journal"), b"bad").unwrap();
    assert!(
        s.project(AcronymId::new(7).unwrap().value().unwrap())
            .is_err()
    );
    s.abort().unwrap();
    let baseline = r.budget_statistics();
    let reservation = r
        .source
        .budget
        .reserve(baseline.limit_bytes - baseline.current_bytes)
        .unwrap();
    assert_eq!(
        r.value_session(&limits(&p)).err().unwrap().kind,
        ErrorKind::BudgetTooSmall
    );
    drop(reservation);
}
#[test]
fn metadata_rename_faults_preserve_cleanup_ownership_and_no_clobber() {
    for event in [
        Event::MetadataCreate,
        Event::MetadataWrite,
        Event::MetadataSync,
        Event::MetadataRename,
    ] {
        let reset = Reset::new();
        let p = Parent::new();
        let mut w = writer(&p);
        w.add_acronym(AcronymId::new(7).unwrap(), b"Alpha", b"before")
            .unwrap();
        fault(event, 0, false, io::ErrorKind::StorageFull);
        let e = w
            .update_acronym(
                AcronymId::new(7).unwrap(),
                b"Changed",
                b"much longer new text",
            )
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Io);
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
        drop(reset);
    }
    let p = Parent::new();
    let mut w = writer(&p);
    w.add_acronym(AcronymId::new(7).unwrap(), b"Alpha", b"before")
        .unwrap();
    w.register_symbol(&WriteSymbol::new(b"Taken", SymbolType::Parameter, 0))
        .unwrap();
    assert!(
        w.update_acronym(AcronymId::new(7).unwrap(), b"Taken", b"collision")
            .is_err()
    );
    w.abort().unwrap();
    p.empty();
}

#[test]
fn typed_writes_bypass_maps_and_write_reset_changes_only_future_float_input() {
    let p = Parent::new();
    let mut w = writer(&p);
    let ids = [
        w.register_symbol(&WriteSymbol::new(b"Typed", SymbolType::Parameter, 0))
            .unwrap(),
        w.register_symbol(&WriteSymbol::new(b"Mapped", SymbolType::Parameter, 0))
            .unwrap(),
        w.register_symbol(&WriteSymbol::new(b"Reset", SymbolType::Parameter, 0))
            .unwrap(),
    ];
    w.set_write_special_values(bits()).unwrap();
    let mut row = w.begin_symbol(ids[0], None).unwrap();
    row.write(&[], &[Value::NumberBits(CUSTOM[0].to_bits())])
        .unwrap();
    row.finish().unwrap();
    let mut row = w.begin_symbol(ids[1], None).unwrap();
    row.write_f64(&[], &CUSTOM[..1]).unwrap();
    row.finish().unwrap();
    w.reset_write_value_policy().unwrap();
    let mut row = w.begin_symbol(ids[2], None).unwrap();
    row.write_f64(&[], &CUSTOM[..1]).unwrap();
    row.finish().unwrap();
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    for (id, expected) in ids.into_iter().zip([
        Value::NumberBits(CUSTOM[0].to_bits()),
        Value::Special(SpecialValue::Undefined),
        Value::NumberBits(CUSTOM[0].to_bits()),
    ]) {
        let h = r.symbol(SymbolId::new(id.get()).unwrap()).unwrap();
        let mut scan = r.scan(&h, r.scan_options()).unwrap();
        assert_eq!(
            scan.next_batch()
                .unwrap()
                .unwrap()
                .value_column(ValueField::Level)
                .unwrap(),
            &[expected]
        );
    }
}
#[test]
fn invalid_read_policies_and_inconsistent_acronyms_are_atomic_and_terminal() {
    let p = Parent::new();
    let r = source(&p);
    for mode in 0..4 {
        let mut s = r.value_session(&limits(&p)).unwrap();
        let old = s.read_special_values();
        let error = match mode {
            0 => s
                .set_read_special_values([0; 5], ReadCollisions::Reject)
                .unwrap_err(),
            1 => s.set_special_values([0; 5]).unwrap_err(),
            2 => s
                .project(Value::Acronym {
                    id: AcronymId::new(7).unwrap(),
                    encoded_bits: match AcronymId::new(9).unwrap().value().unwrap() {
                        Value::Acronym { encoded_bits, .. } => encoded_bits,
                        _ => unreachable!(),
                    },
                })
                .unwrap_err(),
            _ => s
                .set_next_acronym_id(Some(AcronymId::new(i32::MAX as u32).unwrap()))
                .unwrap_err(),
        };
        assert_eq!(s.read_special_values(), old);
        assert_eq!(s.project(Value::ZERO).unwrap_err().kind, error.kind);
        assert!(s.reset_special_values().is_err());
        s.abort().unwrap();
    }
}
#[test]
fn lazy_journal_allocation_failure_retains_owner_and_cleanup_diagnostic_for_retry() {
    let reset = Reset::new();
    let p = Parent::new();
    let r = source(&p);
    let mut s = r.value_session(&limits(&p)).unwrap();
    let budget = r.budget_statistics();
    let reservation = r
        .source
        .budget
        .reserve(budget.limit_bytes - budget.current_bytes)
        .unwrap();
    fault(
        Event::MetadataRemove,
        0,
        true,
        io::ErrorKind::PermissionDenied,
    );
    let error = s
        .project(AcronymId::new(7).unwrap().value().unwrap())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::BudgetTooSmall);
    assert!(error.cleanup_error().is_some());
    assert!(s.abort().is_err());
    drop(reservation);
    drop(reset);
    s.abort().unwrap();
}
#[test]
fn growing_mapping_journal_crosses_pages_without_retaining_a_table_and_epochs_stay_bounded() {
    let p = Parent::new();
    let r = source(&p);
    let mut s = r.value_session(&limits(&p)).unwrap();
    for _ in 0..8 {
        s.reset_acronyms().unwrap();
        s.set_next_acronym_id(Some(AcronymId::new(10000).unwrap()))
            .unwrap();
        for original in 1..=256 {
            let id = AcronymId::new(original).unwrap();
            let projected = s.project(id.value().unwrap()).unwrap();
            assert_eq!(
                projected.acronym_mapping.unwrap().mapped.get(),
                9999 + original
            );
        }
        assert_eq!(s.next_acronym_id().unwrap().get(), 10256);
    }
    assert!(s.statistics().temporary_bytes > 32768);
    assert!(r.budget_statistics().peak_bytes <= 1024 * 1024);
    assert_eq!(s.statistics().peak_open_files, 1);
    s.close().unwrap();
}
#[test]
fn metadata_rewrite_reopen_fault_and_live_copy_quota_never_publish() {
    let reset = Reset::new();
    let p = Parent::new();
    let mut w = writer(&p);
    w.add_acronym(AcronymId::new(7).unwrap(), b"A", b"old")
        .unwrap();
    fault(Event::MetadataCreate, 1, false, io::ErrorKind::StorageFull);
    assert_eq!(
        w.update_acronym(AcronymId::new(7).unwrap(), b"Changed", b"new")
            .unwrap_err()
            .kind,
        ErrorKind::Io
    );
    w.abort().unwrap();
    p.empty();
    drop(reset);
    let p = Parent::new();
    let mut w = Writer::create(p.destination(), options()).unwrap();
    let mut o = limits(&p);
    o.maximum_temporary_bytes = 128;
    w.begin_metadata(&o).unwrap();
    w.add_acronym(AcronymId::new(7).unwrap(), b"A", b"old")
        .unwrap();
    assert_eq!(
        w.update_acronym(
            AcronymId::new(7).unwrap(),
            b"Changed",
            b"a much longer replacement than fits the live copy quota"
        )
        .unwrap_err()
        .kind,
        ErrorKind::TemporarySpaceExceeded
    );
    assert!(!p.destination().exists());
    w.abort().unwrap();
    p.empty();
}

#[test]
fn float_preserve_policy_round_trips_all_ieee_bits_through_the_typed_reader() {
    let p = Parent::new();
    let mut w = writer(&p);
    let id = w
        .register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 0))
        .unwrap();
    let bits = [
        0x7ff8000000001234,
        (-0.0f64).to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        1.25f64.to_bits(),
    ];
    let mut rows = w.begin_symbol(id, None).unwrap();
    rows.write_f64(&[], &bits.map(f64::from_bits)).unwrap();
    rows.finish().unwrap();
    w.finish().unwrap();
    let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
    let h = r.symbol(SymbolId::new(1).unwrap()).unwrap();
    let mut scan = r.scan(&h, r.scan_options()).unwrap();
    let batch = scan.next_batch().unwrap().unwrap();
    for (field, bits) in ValueField::ALL.into_iter().zip(bits) {
        assert_eq!(
            batch.value_column(field).unwrap(),
            &[Value::NumberBits(bits)]
        );
    }
}

#[test]
fn float_conversion_failure_closes_external_sort_before_parent_cleanup() {
    let p = Parent::new();
    let mut w = writer(&p);
    let key = w.register_uel(b"a").unwrap();
    let id = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    let mut rows = w.begin_unsorted_symbol(id, None).unwrap();
    for _ in 0..40 {
        rows.write_f64(&[key], &[0.0]).unwrap();
    }
    let cleanup = crate::spill::test_cleanup_failure();
    let error = rows.write_f64(&[key], &[0.0; 6]).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidDimension);
    assert!(error.cleanup_error().is_some());
    drop(rows);
    assert!(!p.destination().exists());
    p.empty();
    drop(cleanup);
    w.abort().unwrap();
}
