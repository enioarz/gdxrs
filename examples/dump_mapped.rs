//! Bounded mapped stream with explicit original raw keys, for native qualification.
use gdxrs::*;
use std::io::{self, Write};
fn value(v: Value) -> String {
    match v {
        Value::NumberBits(n) => format!("f:{n:016x}"),
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
    let input = args.next().ok_or("missing input")?;
    let scratch = args.next().ok_or("missing scratch")?;
    let symbol = args
        .next()
        .ok_or("missing symbol")?
        .to_str()
        .ok_or("invalid symbol")?
        .parse::<u32>()?;
    let mut large = false;
    let mut skip = false;
    for a in args {
        match a.to_str() {
            Some("large") => large = true,
            Some("skip") => skip = true,
            _ => return Err("invalid mode".into()),
        }
    }
    let r = Reader::open(
        input,
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )?;
    let mut options = MetadataOptions::new(scratch);
    options.maximum_open_files = 7;
    options.merge_fan_in = 2;
    options.run_bytes = if large { 16384 } else { 512 };
    let mut map = r.mapped_uels(&options)?;
    map.register(
        (1..=if large { 2048 } else { 3 })
            .filter(|n| !skip || *n != 2)
            .map(|n| {
                Ok((
                    UelId::new(n)?,
                    MappedUelId::new(if large {
                        i64::from(((n * 17) % 2048) * 7)
                    } else {
                        match n {
                            1 => 9,
                            2 => 0,
                            _ => 3,
                        }
                    })?,
                ))
            }),
    )?;
    let h = r.symbol(SymbolId::new(symbol)?)?;
    let mut c = r.scan_mapped(
        &h,
        &mut map,
        MappedReadOptions {
            missing: if skip {
                MissingMapping::SkipRecord
            } else {
                MissingMapping::Error
            },
            batch_bytes: 1024,
            ..MappedReadOptions::default()
        },
    )?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "GDXRS-MAPPED\t1")?;
    while let Some(row) = c.next()? {
        write!(out, "R\t")?;
        for (n, key) in row.mapped_keys.iter().enumerate() {
            write!(out, "{}{}", if n > 0 { "," } else { "" }, key.get())?;
        }
        write!(out, "\t")?;
        for (n, v) in row.values.iter().enumerate() {
            write!(out, "{}{}", if n > 0 { "," } else { "" }, value(*v))?;
        }
        write!(out, "\t")?;
        for (n, key) in row.raw_keys.iter().enumerate() {
            write!(out, "{}{}", if n > 0 { "," } else { "" }, key.get())?;
        }
        writeln!(out)?;
    }
    let stats = c.close()?;
    writeln!(out, "E\t{}\t{}", stats.returned, stats.skipped)?;
    out.flush()?;
    drop(c);
    map.close()?;
    eprintln!(
        "mapped_peak_root={} mapped_peak_handles={}",
        r.budget_statistics().peak_bytes,
        map.statistics().peak_open_files
    );
    Ok(())
}
