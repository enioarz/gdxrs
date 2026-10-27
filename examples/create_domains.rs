//! Fixed, independently checked task-027 development recipe (seven input rows).
use gdxrs::*;
use std::io::Write;
fn value(v: Value) -> String {
    match v {
        Value::NumberBits(b) => format!("f:{b:016x}"),
        Value::Acronym { id, .. } => format!("a:{}", id.get()),
        Value::Special(s) => format!(
            "s:{}",
            match s {
                SpecialValue::Undefined => "undef",
                SpecialValue::NotAvailable => "na",
                SpecialValue::PositiveInfinity => "pinf",
                SpecialValue::NegativeInfinity => "minf",
                SpecialValue::Epsilon => "eps",
            }
        ),
    }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args.next().ok_or("output")?;
    let scratch = args.next().ok_or("scratch")?;
    let (mut zlib, mut forward) = (false, false);
    for mode in args {
        match mode.to_str() {
            Some("zlib") => zlib = true,
            Some("forward") => forward = true,
            _ => return Err("invalid mode".into()),
        }
    }

    let mut w = Writer::create(
        output,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression: if zlib {
                WriteCompression::Zlib
            } else {
                WriteCompression::None
            },
            ..WriteOptions::default()
        },
    )?;
    let mut o = MetadataOptions::new(scratch);
    o.maximum_open_files = 7;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    w.begin_metadata(&o)?;
    let early = if forward {
        Some(w.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?)
    } else {
        None
    };
    if let Some(p) = early {
        w.set_domains(
            p,
            &[
                WriteDomain::Strict(SymbolReference::Name(b"J")),
                WriteDomain::Strict(SymbolReference::Name(b"I")),
            ],
        )?;
    }
    let i = w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?;
    w.set_domains(i, &[WriteDomain::Strict(SymbolReference::Registered(i))])?;
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Registered(i));
    alias.description = b"Aliased with I";
    let j = w.register_symbol(&alias)?;
    let p = match early {
        Some(p) => p,
        None => w.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?,
    };
    if early.is_none() {
        w.set_domains(
            p,
            &[
                WriteDomain::Strict(SymbolReference::Registered(j)),
                WriteDomain::Strict(SymbolReference::Registered(i)),
            ],
        )?;
    }
    let r = w.register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 1))?;
    w.set_domains(r, &[WriteDomain::Relaxed(b"Unregistered")])?;
    let u = w.register_symbol(&WriteSymbol::new(b"U", SymbolType::Parameter, 1))?;
    w.set_domains(u, &[WriteDomain::Strict(SymbolReference::Universe)])?;
    let keys = [
        w.register_uel(b"a")?,
        w.register_uel(b"b")?,
        w.register_uel(b"c")?,
    ];
    let mut rows = w.begin_deferred_records()?;
    rows.begin_symbol(p)?;
    let tuples = [[1, 1], [1, 1], [2, 1], [2, 1], [2, 2], [3, 1], [3, 3]];
    let values = [
        Value::NumberBits(1.25f64.to_bits()),
        Value::Special(SpecialValue::Epsilon),
        Value::Special(SpecialValue::NotAvailable),
        Value::Special(SpecialValue::Undefined),
        Value::Special(SpecialValue::NegativeInfinity),
        Value::Special(SpecialValue::PositiveInfinity),
        Value::NumberBits(2.5f64.to_bits()),
    ];
    for (tuple, value) in tuples.into_iter().zip(values) {
        rows.write(&[keys[tuple[0] - 1], keys[tuple[1] - 1]], &[value])?;
    }
    rows.begin_symbol(i)?;
    rows.write(&[keys[0]], &[Value::ZERO])?;
    rows.begin_symbol(r)?;
    rows.write(&[keys[2]], &[Value::Special(SpecialValue::Epsilon)])?;
    rows.begin_symbol(u)?;
    rows.write(&[keys[2]], &[Value::Special(SpecialValue::NotAvailable)])?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    writeln!(out, "GDXRS-RUST-ERRORS\t1").map_err(Error::from)?;
    let stats = rows.finish_reporting(ErrorLimits::default(), |e| {
        let mask = match e.kind {
            DataErrorKind::DomainViolation { dimensions } => dimensions,
            DataErrorKind::Duplicate => 0,
            _ => {
                return Err(std::io::Error::other("unexpected diagnostic").into());
            }
        };
        writeln!(
            out,
            "X\t{}\t{},{}\t{}\t{}",
            e.input_ordinal,
            e.raw_keys[0].get(),
            e.raw_keys[1].get(),
            mask,
            value(e.values[0])
        )
        .map_err(Error::from)
    })?;
    w.finish()?;
    out.flush()?;
    eprintln!(
        "{{\"input\":{},\"accepted\":{},\"reported\":{},\"peak_root\":{},\"peak_handles\":{}}}",
        stats.input_records,
        stats.accepted_records,
        stats.errors.reported,
        w.budget_statistics().peak_bytes,
        w.metadata_statistics().peak_open_files
    );
    Ok(())
}
