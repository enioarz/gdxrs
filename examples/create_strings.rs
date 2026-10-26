//! Bounded task-025 author: fixed recipe or generated 8192-label dictionary.
use gdxrs::*;
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
    let mut writer = Writer::create(
        output,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression,
            ..WriteOptions::default()
        },
    )?;
    let mut limits = MetadataOptions::new(scratch);
    limits.maximum_open_files = 7;
    limits.merge_fan_in = 2;
    limits.run_bytes = if large { 16384 } else { 512 };
    writer.begin_metadata(&limits)?;
    let i = writer.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?;
    let started = std::time::Instant::now();
    let stats = if large {
        let mut rows = writer.begin_string_records()?;
        rows.begin_symbol(i)?;
        for n in (0..8192).rev() {
            let label = format!("L{n:08}{}", "x".repeat(54)); // fixed 63-byte caller label
            rows.write(&[label.as_bytes()], &[Value::ZERO])?;
        }
        rows.finish()?
    } else {
        writer.register_uel(b"z")?;
        writer.register_uel(b"pre")?;
        writer.add_acronym(AcronymId::new(7)?, b"Acro", b"typed")?;
        let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
        alias.alias_target = Some(SymbolReference::Registered(i));
        alias.description = b"Aliased with I";
        let j = writer.register_symbol(&alias)?;
        let p = writer.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?;
        writer.set_domains(
            p,
            &[
                WriteDomain::Strict(SymbolReference::Registered(j)),
                WriteDomain::Strict(SymbolReference::Universe),
            ],
        )?;
        let mut v = WriteSymbol::new(b"V", SymbolType::Variable, 20);
        v.user_information = 5;
        let v = writer.register_symbol(&v)?;
        let mut e = WriteSymbol::new(b"E", SymbolType::Equation, 0);
        e.user_information = 53;
        let e = writer.register_symbol(&e)?;
        let empty =
            writer.register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Parameter, 0))?;
        let mut rows = writer.begin_string_records()?;
        rows.begin_symbol(i)?;
        for label in [b"a".as_slice(), b"A", b"\x80", b"z"] {
            rows.write(&[label], &[Value::ZERO])?;
        }
        rows.begin_symbol(p)?;
        rows.write(&[b"\x80", b"a"], &[Value::Special(SpecialValue::Epsilon)])?;
        rows.write(
            &[b"a", b"new"],
            &[Value::Special(SpecialValue::NotAvailable)],
        )?;
        rows.begin_symbol(v)?;
        rows.write(
            &[b"new".as_slice(); 20],
            &[
                Value::Acronym {
                    id: AcronymId::new(7)?,
                    encoded_bits: (7.0 * 10.0e300f64).to_bits(),
                },
                Value::NumberBits((-0.0f64).to_bits()),
                Value::Special(SpecialValue::Undefined),
                Value::Special(SpecialValue::PositiveInfinity),
                Value::NumberBits(2.5f64.to_bits()),
            ],
        )?;
        rows.begin_symbol(e)?;
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
        rows.begin_symbol(empty)?;
        rows.finish()?
    };
    let elapsed = started.elapsed().as_secs_f64();
    let metadata = writer.metadata_statistics();
    writer.finish()?;
    let budget = writer.budget_statistics();
    println!(
        "{{\"records\":{},\"uels\":{},\"requests\":{},\"merge_passes\":{},\"peak_root\":{},\"peak_scratch\":{},\"peak_handles\":{},\"seconds\":{}}}",
        stats.records,
        stats.uels,
        stats.label_requests,
        stats.merge_passes,
        budget.peak_bytes,
        metadata.peak_temporary_bytes,
        metadata.peak_open_files,
        elapsed
    );
    Ok(())
}
