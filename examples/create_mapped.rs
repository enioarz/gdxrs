//! Fixed mapped-author recipe or generated sparse dictionary. No retained tokens.
use gdxrs::*;
fn id(n: i64) -> Result<MappedUelId> {
    MappedUelId::new(n)
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args.next().ok_or("missing output")?;
    let scratch = args.next().ok_or("missing scratch")?;
    let mut compression = WriteCompression::None;
    let mut large = false;
    for a in args {
        match a.to_str() {
            Some("zlib") => compression = WriteCompression::Zlib,
            Some("large") => large = true,
            _ => return Err("invalid mode".into()),
        }
    }
    let mut w = Writer::create(
        output,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression,
            ..WriteOptions::default()
        },
    )?;
    let mut options = MetadataOptions::new(scratch);
    options.maximum_open_files = 7;
    options.merge_fan_in = 2;
    options.run_bytes = if large { 16384 } else { 512 };
    w.begin_metadata(&options)?;
    let i = w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?;
    if large {
        w.register_mapped_uels(
            (1..=2048).map(|n| Ok((id(((n * 17) % 2048) * 7)?, format!("u{n}")))),
        )?;
        let mut rows = w.begin_mapped_symbol(i)?;
        for n in (1..=2048).rev() {
            rows.write(&[id(((n * 17) % 2048) * 7)?], &[Value::ZERO])?;
        }
        rows.finish()?;
    } else {
        let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
        alias.alias_target = Some(SymbolReference::Registered(i));
        alias.description = b"Aliased with I";
        let j = w.register_symbol(&alias)?;
        let p = w.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?;
        w.set_domains(
            p,
            &[
                WriteDomain::Strict(SymbolReference::Registered(j)),
                WriteDomain::Strict(SymbolReference::Universe),
            ],
        )?;
        let mut variable = WriteSymbol::new(b"V", SymbolType::Variable, 20);
        variable.user_information = 5;
        let v = w.register_symbol(&variable)?;
        let mut equation = WriteSymbol::new(b"E", SymbolType::Equation, 0);
        equation.user_information = 53;
        let e = w.register_symbol(&equation)?;
        w.register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Parameter, 0))?;
        w.add_acronym(AcronymId::new(7)?, b"Acro", b"typed")?;
        w.register_mapped_uels(
            [(9, b"z".as_slice()), (0, b"a"), (3, b"C"), (9, b"z")]
                .into_iter()
                .map(|(m, b)| Ok((id(m)?, b))),
        )?;
        let mut rows = w.begin_mapped_symbol(i)?;
        for n in [3, 9, 0] {
            rows.write(&[id(n)?], &[Value::ZERO])?;
        }
        rows.finish()?;
        let mut rows = w.begin_mapped_symbol(p)?;
        for (keys, value) in [
            ([9, 0], Value::NumberBits(1.25f64.to_bits())),
            ([3, 9], Value::Special(SpecialValue::Undefined)),
            ([0, 3], Value::Special(SpecialValue::Epsilon)),
        ] {
            rows.write(&keys.map(|n| id(n).unwrap()), &[value])?;
        } // fixed known-valid IDs
        rows.finish()?;
        let mut rows = w.begin_mapped_symbol(v)?;
        rows.write(
            &[id(9)?; 20],
            &[
                Value::Acronym {
                    id: AcronymId::new(7)?,
                    encoded_bits: (7.0 * 10.0e300f64).to_bits(),
                },
                Value::NumberBits((-1.0f64).to_bits()),
                Value::Special(SpecialValue::Undefined),
                Value::Special(SpecialValue::PositiveInfinity),
                Value::NumberBits(2.5f64.to_bits()),
            ],
        )?;
        rows.finish()?;
        let mut rows = w.begin_mapped_symbol(e)?;
        rows.write(
            &[],
            &[
                Value::Special(SpecialValue::NotAvailable),
                Value::ZERO,
                Value::ZERO,
                Value::ZERO,
                Value::NumberBits(1.0f64.to_bits()),
            ],
        )?;
        rows.finish()?;
    }
    let metadata = w.metadata_statistics();
    w.finish()?;
    println!(
        "{{\"peak_root\":{},\"peak_scratch\":{},\"peak_handles\":{}}}",
        w.budget_statistics().peak_bytes,
        metadata.peak_temporary_bytes,
        metadata.peak_open_files
    );
    Ok(())
}
