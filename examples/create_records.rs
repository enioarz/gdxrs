//! Fixed, small interoperability recipe (700 IDs, 45 symbols).
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("missing output")?;
    let scratch = args.next().ok_or("missing scratch")?;
    let compression = if args.next().is_some() {
        WriteCompression::Zlib
    } else {
        WriteCompression::None
    };
    let mut w = Writer::create(
        path,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression,
            ..WriteOptions::default()
        },
    )?;
    let mut limits = MetadataOptions::new(scratch);
    limits.run_bytes = 1024;
    limits.maximum_open_files = 6;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits)?;
    let mut ids = Vec::with_capacity(700); // fixed recipe tooling only
    w.register_uels_fresh((1..=700).map(|n| Ok(format!("u{n}"))), |_, id| {
        ids.push(id);
        Ok(())
    })?;
    w.add_set_text(b"member")?;
    let acro = AcronymId::new(7)?;
    w.add_acronym(acro, b"Acro", b"typed")?;
    let set = w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?;
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Registered(set));
    let alias = w.register_symbol(&alias)?;
    let p = w.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?;
    w.set_domains(
        p,
        &[
            WriteDomain::Strict(SymbolReference::Registered(alias)),
            WriteDomain::Strict(SymbolReference::Universe),
        ],
    )?;
    let wide = w.register_symbol(&WriteSymbol::new(b"Wide", SymbolType::Parameter, 20))?;
    let scalar = w.register_symbol(&WriteSymbol::new(b"Scalar", SymbolType::Parameter, 0))?;
    w.register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Parameter, 0))?;
    let mut subtypes = Vec::with_capacity(17);
    for (kind, first, last, prefix) in [
        (SymbolType::Variable, 0, 9, "V"),
        (SymbolType::Equation, 53, 59, "E"),
    ] {
        for user in first..=last {
            let name = format!("{prefix}{user}");
            let mut spec = WriteSymbol::new(name.as_bytes(), kind, 0);
            spec.user_information = user;
            subtypes.push(w.register_symbol(&spec)?);
        }
    }
    for (kind, first, last, prefix) in [
        (SymbolType::Variable, 0, 9, "DefaultV"),
        (SymbolType::Equation, 53, 59, "DefaultE"),
    ] {
        for user in first..=last {
            let name = format!("{prefix}{user}");
            let mut spec = WriteSymbol::new(name.as_bytes(), kind, 0);
            spec.user_information = user;
            w.register_symbol(&spec)?;
        }
    }
    let mut widths = Vec::with_capacity(4);
    for (n, high) in [254, 255, 65534, 65535].into_iter().enumerate() {
        let name = format!("W{n}");
        widths.push((
            w.register_symbol(&WriteSymbol::new(name.as_bytes(), SymbolType::Parameter, 1))?,
            high,
        ));
    }
    let bulk = w.register_symbol(&WriteSymbol::new(b"Bulk", SymbolType::Parameter, 2))?;
    let mut out = w.begin_symbol(set, None)?;
    for index in [0, 1, 253, 507, 699] {
        out.write(&[ids[index]], &[Value::NumberBits(1f64.to_bits())])?;
    }
    out.finish()?;
    let mut out = w.begin_symbol(
        p,
        Some(
            &[KeyBounds {
                minimum: 1,
                maximum: 700,
            }; 2],
        ),
    )?;
    let values = [
        Value::Special(SpecialValue::Undefined),
        Value::Special(SpecialValue::NotAvailable),
        Value::Special(SpecialValue::PositiveInfinity),
        Value::Special(SpecialValue::NegativeInfinity),
        Value::Special(SpecialValue::Epsilon),
        Value::Acronym {
            id: acro,
            encoded_bits: (7.0 * 10.0e300f64).to_bits(),
        },
        Value::NumberBits((-0.0f64).to_bits()),
        Value::NumberBits((-3.25f64).to_bits()),
    ];
    for (n, (a, b)) in [
        (0, 0),
        (0, 1),
        (0, 253),
        (0, 507),
        (1, 0),
        (253, 1),
        (507, 253),
        (699, 699),
    ]
    .into_iter()
    .enumerate()
    {
        out.write(&[ids[a], ids[b]], &[values[n]])?;
    }
    out.finish()?;
    let mut out = w.begin_symbol(wide, None)?;
    let mut keys = [ids[0]; 20];
    out.write(&keys, &[Value::NumberBits(0.5f64.to_bits())])?;
    keys[19] = ids[1];
    out.write(&keys, &[Value::NumberBits(2f64.to_bits())])?;
    out.finish()?;
    let mut out = w.begin_symbol(scalar, None)?;
    out.write(&[], &[Value::NumberBits(1f64.to_bits())])?;
    out.finish()?;
    for id in subtypes {
        let mut out = w.begin_symbol(id, None)?;
        out.write(
            &[],
            &[
                Value::ZERO,
                Value::NumberBits(1f64.to_bits()),
                Value::NumberBits((-1f64).to_bits()),
                Value::NumberBits(0.5f64.to_bits()),
                Value::NumberBits(2f64.to_bits()),
            ],
        )?;
        out.finish()?;
    }
    for (id, high) in widths {
        let mut out = w.begin_symbol(
            id,
            Some(&[KeyBounds {
                minimum: 0,
                maximum: high,
            }]),
        )?;
        for index in [0, 1, 253] {
            out.write(&[ids[index]], &[Value::ZERO])?;
        }
        out.finish()?;
    }
    let mut out = w.begin_symbol(bulk, None)?;
    for a in &ids[..10] {
        for b in &ids {
            out.write(&[*a, *b], &[Value::NumberBits((-3.25f64).to_bits())])?;
        }
    }
    out.finish()?;
    w.finish()?;
    Ok(())
}
