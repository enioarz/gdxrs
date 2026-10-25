//! Stream a normalized semantic TSV dump without collecting metadata or records.
use gdxrs::*;
use std::{fmt::Write as _, io::Write as _};
fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}
fn dump(
    out: &mut impl std::io::Write,
    reader: &Reader,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let records = true;
    let info = reader.information()?;
    writeln!(
        out,
        "GDXRS-ORACLE\t1\nF\t7\t{}\t{}\t{}\t{}\t<creation-system>",
        info.compression_level,
        info.symbol_count,
        info.uel_count,
        hex(info.producer)
    )?;
    let mut uels = reader.uels()?;
    while let Some(v) = uels.next()? {
        writeln!(out, "U\t{}\t{}", v.id.get(), hex(v.label))?;
    }
    drop(uels);
    let mut texts = reader.set_texts()?;
    while let Some(v) = texts.next()? {
        writeln!(out, "T\t{}\t0\t{}", v.id, hex(v.text))?;
    }
    drop(texts);
    let mut acronyms = reader.acronyms()?;
    while let Some(v) = acronyms.next()? {
        writeln!(
            out,
            "A\t{}\t{}\t{}\t{}",
            v.ordinal,
            v.id.get(),
            hex(v.name),
            hex(v.text)
        )?;
    }
    drop(acronyms);
    for n in 0..=info.symbol_count {
        let handle = reader.symbol(SymbolId::new(n)?)?;
        let v = handle.metadata()?;
        let count = if v.kind == SymbolType::Alias {
            v.physical_record_count
        } else {
            v.raw_record_count
        };
        writeln!(
            out,
            "S\t{n}\t{}\t{}\t{}\t{count}\t{}\t{}",
            hex(v.name),
            v.dimension,
            v.kind.code(),
            v.user_information,
            hex(v.description)
        )?;
        if n != 0 {
            let mut domains = reader.domains(&handle)?;
            write!(out, "D\t{n}\t{}", domains.mode().code())?;
            while let Some(d) = domains.next()? {
                write!(
                    out,
                    "\t{}:{}",
                    d.strict_symbol.map(|id| id.get()).unwrap_or(0),
                    hex(d.name)
                )?;
            }
            out.write_all(b"\n")?;
            drop(domains);
            let mut comments = reader.comments(&handle)?;
            while let Some(c) = comments.next()? {
                writeln!(out, "C\t{n}\t{}\t{}", c.ordinal, hex(c.text))?;
            }
        }
        if records {
            let mut cursor = reader.scan(&handle, reader.scan_options())?;
            let mut count = 0;
            while let Some(batch) = cursor.next_batch()? {
                for i in 0..batch.len() {
                    write!(out, "R\t{n}\t")?;
                    for (index, dimension) in batch.key_dimensions().iter().enumerate() {
                        write!(
                            out,
                            "{}{}",
                            if index == 0 { "" } else { "," },
                            batch.key_column(*dimension).unwrap()[i].get()
                        )?;
                    }
                    out.write_all(b"\t")?;
                    for (index, field) in batch.value_fields().iter().enumerate() {
                        write!(
                            out,
                            "{}{}",
                            if index == 0 { "" } else { "," },
                            value(batch.value_column(*field).unwrap()[i])
                        )?;
                    }
                    out.write_all(b"\n")?;
                    count += 1;
                }
            }
            writeln!(out, "E\t{n}\t{count}")?;
        }
    }
    Ok(())
}
fn value(v: Value) -> String {
    match v {
        Value::NumberBits(bits) => format!("f:{bits:016x}"),
        Value::Acronym { id, .. } => format!("a:{}", id.get()),
        Value::Special(v) => format!(
            "s:{}",
            match v {
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
    let path = args.next().ok_or("usage: dump FILE.gdx")?;
    if args.next().is_some() {
        return Err("usage: dump FILE.gdx".into());
    }
    let reader = Reader::open(path, OpenOptions::default())?;
    let mut out = std::io::BufWriter::with_capacity(65536, std::io::stdout().lock());
    dump(&mut out, &reader)?;
    out.flush()?;
    Ok(())
}
