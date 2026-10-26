use super::*;
use crate::write::tests::{Parent, Reset, fault, options};
fn setup(p: &Parent) -> (Writer, WriterUelId, WriterUelId, WriterSymbolId) {
    let mut w = Writer::create(p.destination(), options()).unwrap();
    w.begin_metadata(&MetadataOptions::new(p.destination().parent().unwrap()))
        .unwrap();
    let a = w.register_uel(b"a").unwrap();
    let b = w.register_uel(b"b").unwrap();
    let s = w
        .register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))
        .unwrap();
    (w, a, b, s)
}
#[test]
fn records_dimensions_values_widths_and_short_writes() {
    let _reset = Reset::new();
    for compression in [WriteCompression::None, WriteCompression::Zlib] {
        if compression == WriteCompression::Zlib && !cfg!(feature = "compression") {
            continue;
        }
        let p = Parent::new();
        let mut o = options();
        o.compression = compression;
        let mut w = Writer::create(p.destination(), o).unwrap();
        w.begin_metadata(&MetadataOptions::new(p.destination().parent().unwrap()))
            .unwrap();
        let a = w.register_uel(b"a").unwrap();
        let b = w.register_uel(b"b").unwrap();
        let mut syms = Vec::new(); // bounded test recipe: 24 symbols
        for dim in [0, 1, 2, 20] {
            for (low, high) in [
                (1, 255),
                (0, 255),
                (0, 65534),
                (0, 65535),
                (0, i32::MAX),
                (1, 1),
            ] {
                let name = format!("P{}", syms.len());
                syms.push((
                    w.register_symbol(&WriteSymbol::new(
                        name.as_bytes(),
                        SymbolType::Parameter,
                        dim,
                    ))
                    .unwrap(),
                    dim,
                    low,
                    high,
                ));
            }
        }
        crate::write::tests::short_chunks(3);
        let vals = [
            Value::NumberBits((-0.0f64).to_bits()),
            Value::NumberBits(f64::NAN.to_bits()),
        ];
        for (s, dim, low, high) in &syms {
            let bounds = [KeyBounds {
                minimum: *low,
                maximum: *high,
            }; 20];
            let keys = [a; 20];
            let mut out = w.begin_symbol(*s, Some(&bounds[..*dim as usize])).unwrap();
            out.write(&keys[..*dim as usize], &vals[..1]).unwrap();
            if *dim > 0 && *high > 1 {
                let mut keys = keys;
                keys[*dim as usize - 1] = b;
                out.write(&keys[..*dim as usize], &vals[1..]).unwrap();
            }
            out.finish().unwrap();
        }
        w.finish().unwrap();
        let r = Reader::open(p.destination(), OpenOptions::default()).unwrap();
        for (s, dim, _, high) in syms {
            let h = r.symbol(SymbolId::new(s.get()).unwrap()).unwrap();
            let mut scan = r.scan(&h, ScanOptions::default()).unwrap();
            let batch = scan.next_batch().unwrap().unwrap();
            assert_eq!(batch.len(), if dim > 0 && high > 1 { 2 } else { 1 });
            assert_eq!(batch.value_column(ValueField::Level).unwrap()[0], vals[0]);
        }
    }
}
#[test]
fn bad_rows_are_terminal_and_never_publish() {
    let _reset = Reset::new();
    for case in 0..7 {
        let p = Parent::new();
        let (mut w, a, b, s) = setup(&p);
        let mut out = w.begin_symbol(s, None).unwrap();
        out.write(&[b], &[Value::ZERO]).unwrap();
        let error = match case {
            0 => out.write(&[b], &[Value::ZERO]),
            1 => out.write(&[a], &[Value::ZERO]),
            2 => out.write(&[], &[Value::ZERO]),
            3 => out.write(&[b], &[]),
            4 => {
                out.count = i32::MAX as u32;
                out.write(&[b], &[Value::ZERO])
            }
            5 => out.write(
                &[{
                    let other = Parent::new();
                    let (_, foreign, _, _) = setup(&other);
                    foreign
                }],
                &[Value::ZERO],
            ),
            _ => out.write(
                &[b],
                &[Value::Acronym {
                    id: AcronymId::new(7).unwrap(),
                    encoded_bits: 1f64.to_bits(),
                }],
            ),
        };
        assert!(error.is_err());
        drop(out);
        assert!(w.finish().is_err());
        assert!(!p.destination().exists());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn abandoned_and_forgotten_symbols_block_publication() {
    for forget in [false, true] {
        let p = Parent::new();
        let (mut w, _, _, s) = setup(&p);
        let out = w.begin_symbol(s, None).unwrap();
        if forget {
            std::mem::forget(out);
        } else {
            drop(out);
        }
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn strict_alias_domains_use_sorted_disk_members_and_reject_missing_keys() {
    let _reset = Reset::new();
    for valid in [false, true] {
        let p = Parent::new();
        let (mut w, a, b, param) = setup(&p);
        let set = w
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
        alias.alias_target = Some(SymbolReference::Registered(set));
        let alias = w.register_symbol(&alias).unwrap();
        w.set_domains(
            param,
            &[WriteDomain::Strict(SymbolReference::Registered(alias))],
        )
        .unwrap();
        let mut out = w.begin_symbol(set, None).unwrap();
        out.write(&[a], &[Value::ZERO]).unwrap();
        out.finish().unwrap();
        let mut out = w.begin_symbol(param, None).unwrap();
        let result = out.write(&[if valid { a } else { b }], &[Value::ZERO]);
        if valid {
            result.unwrap();
            out.finish().unwrap();
            w.finish().unwrap();
        } else {
            assert!(result.is_err());
            drop(out);
            assert!(w.finish().is_err());
            p.empty();
        }
    }
}
#[test]
fn invalid_starts_and_forward_domain_data_are_terminal() {
    for case in 0..6 {
        let p = Parent::new();
        let (mut w, _, _, s) = setup(&p);
        let result = match case {
            0 => w.begin_symbol(s, Some(&[])).map(|_| ()),
            1 => w
                .begin_symbol(
                    s,
                    Some(&[KeyBounds {
                        minimum: -1,
                        maximum: 4,
                    }]),
                )
                .map(|_| ()),
            2 => w
                .begin_symbol(
                    s,
                    Some(&[KeyBounds {
                        minimum: 4,
                        maximum: 1,
                    }]),
                )
                .map(|_| ()),
            3 => {
                let domain = w
                    .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
                    .unwrap();
                w.set_domains(
                    s,
                    &[WriteDomain::Strict(SymbolReference::Registered(domain))],
                )
                .unwrap();
                w.begin_symbol(s, None).map(|_| ())
            }
            4 => {
                w.begin_symbol(s, None).unwrap().finish().unwrap();
                w.begin_symbol(s, None).map(|_| ())
            }
            _ => {
                let mut spec = WriteSymbol::new(b"V", SymbolType::Alias, 1);
                spec.alias_target = Some(SymbolReference::Universe);
                let id = w.register_symbol(&spec).unwrap();
                w.begin_symbol(id, None).map(|_| ())
            }
        };
        assert!(result.is_err());
        assert!(w.finish().is_err());
        w.abort().unwrap();
        p.empty();
    }
}
#[test]
fn output_and_scratch_faults_and_root_admission_are_terminal() {
    let _reset = Reset::new();
    for event in [Event::Write, Event::MetadataWrite] {
        let p = Parent::new();
        let (mut w, _, _, s) = setup(&p);
        let out = w.begin_symbol(s, None).unwrap();
        fault(event, 0, true, io::ErrorKind::StorageFull);
        assert!(out.finish().and_then(|_| w.finish().map(|_| 0)).is_err());
        assert!(!p.destination().exists());
        Reset::new();
        w.abort().unwrap();
        p.empty();
    }
    let p = Parent::new();
    let (mut w, _, _, s) = setup(&p);
    let reservation = w
        .budget
        .reserve(w.budget.statistics().limit_bytes - w.budget.statistics().current_bytes)
        .unwrap();
    assert!(w.begin_symbol(s, None).is_err());
    drop(reservation);
    w.abort().unwrap();
    p.empty();
}

#[test]
fn typed_value_references_set_text_and_scalar_multiplicity() {
    for case in 0..7 {
        let p = Parent::new();
        let (mut w, a, _, _) = setup(&p);
        let kind = if case < 2 {
            SymbolType::Set
        } else {
            SymbolType::Parameter
        };
        let s = w.register_symbol(&WriteSymbol::new(b"Q", kind, 0)).unwrap();
        let mut out = w.begin_symbol(s, None).unwrap();
        let value = match case {
            0 => Value::NumberBits(1f64.to_bits()), // unregistered text
            1 => Value::Special(SpecialValue::Epsilon),
            2 => Value::Acronym {
                id: AcronymId::new(7).unwrap(),
                encoded_bits: (7.0 * 10.0e300f64).to_bits(),
            },
            3 => Value::NumberBits((7.0 * 10.0e300f64).to_bits()),
            4 => Value::Acronym {
                id: AcronymId::new(7).unwrap(),
                encoded_bits: 1f64.to_bits(),
            },
            _ => Value::ZERO,
        };
        let result = if case == 5 {
            out.write(&[], &[value]).unwrap();
            out.write(&[], &[value])
        } else if case == 6 {
            out.write(&[a], &[value])
        } else {
            out.write(&[], &[value])
        };
        assert!(result.is_err());
        drop(out);
        assert!(w.finish().is_err());
        p.empty();
    }
}
#[test]
fn bounds_foreign_keys_and_mid_record_output_limit() {
    for case in 0..3 {
        let p = Parent::new();
        let (mut w, a, b, s) = setup(&p);
        let mut out = w
            .begin_symbol(
                s,
                Some(&[KeyBounds {
                    minimum: 1,
                    maximum: 1,
                }]),
            )
            .unwrap();
        let result = match case {
            0 => out.write(&[b], &[Value::ZERO]),
            1 => {
                let other = Parent::new();
                let (_, foreign, _, _) = setup(&other);
                out.write(&[foreign], &[Value::ZERO])
            }
            _ => {
                out.writer.limit = out.writer.position;
                out.write(&[a], &[Value::ZERO])
            }
        };
        assert!(result.is_err());
        drop(out);
        assert!(w.finish().is_err());
        p.empty();
    }
}
#[test]
fn empty_domain_and_corrupt_membership_never_accept_rows() {
    for corrupt in [false, true] {
        let p = Parent::new();
        let (mut w, a, _, param) = setup(&p);
        let set = w
            .register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))
            .unwrap();
        w.set_domains(
            param,
            &[WriteDomain::Strict(SymbolReference::Registered(set))],
        )
        .unwrap();
        let mut out = w.begin_symbol(set, None).unwrap();
        if corrupt {
            out.write(&[a], &[Value::ZERO]).unwrap();
        }
        out.finish().unwrap();
        let mut out = w.begin_symbol(param, None).unwrap();
        if corrupt {
            // The set-member extent is checked by the journal's checksum reader.
            out.domains[0].as_mut().unwrap().start += 1;
        }
        assert!(out.write(&[a], &[Value::ZERO]).is_err());
        drop(out);
        p.empty();
    }
}
