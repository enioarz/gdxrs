//! Task 022's fixed-size full metadata/interoperability authoring recipe.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("missing output")?;
    let scratch = args.next().ok_or("missing scratch")?;
    let mut compression = WriteCompression::None;
    let mut unsorted = false;
    for argument in args {
        match argument.to_str() {
            Some("zlib") => compression = WriteCompression::Zlib,
            Some("unsorted") => unsorted = true,
            _ => return Err("invalid authoring mode".into()),
        }
    }
    let mut w = Writer::create(
        path,
        WriteOptions {
            memory_budget_bytes: 1024 * 1024,
            compression,
            ..WriteOptions::default()
        },
    )?;
    let mut limits = MetadataOptions::new(scratch);
    limits.run_bytes = 512;
    limits.maximum_open_files = 6;
    limits.merge_fan_in = 2;
    w.begin_metadata(&limits)?;
    let ids = [
        w.register_uel(b"z")?,
        w.register_uel(b"a")?,
        w.register_uel(b"C")?,
        w.register_uel(b"\x80")?,
    ];
    assert_eq!(w.register_uel(b"z")?, ids[0]);
    assert_eq!(w.add_set_text(b"member")?, 1);
    assert_eq!(w.add_set_text(b"member")?, 1);
    w.add_acronym(AcronymId::new(7)?, b"Acro", b"typed")?;
    let mut spec = WriteSymbol::new(b"I", SymbolType::Set, 1);
    spec.description = &[b'd'; 255];
    let set = w.register_symbol(&spec)?;
    w.add_comment(set, &[b'c'; 255])?;
    w.add_comment(set, b"second")?;
    let mut spec = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    spec.alias_target = Some(SymbolReference::Registered(set));
    spec.description = b"Aliased with I";
    let alias = w.register_symbol(&spec)?;
    spec.name = b"K";
    spec.description = b"Aliased with J";
    spec.alias_target = Some(SymbolReference::Registered(alias));
    w.register_symbol(&spec)?;
    let mut spec = WriteSymbol::new(b"P", SymbolType::Parameter, 2);
    spec.description = b"strict";
    let p = w.register_symbol(&spec)?;
    w.add_comment(p, b"param")?;
    w.set_domains(
        p,
        &[
            WriteDomain::Strict(SymbolReference::Registered(alias)),
            WriteDomain::Strict(SymbolReference::Universe),
        ],
    )?;
    let relaxed = w.register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 2))?;
    w.set_domains(
        relaxed,
        &[WriteDomain::Relaxed(b"future"), WriteDomain::Relaxed(b"*")],
    )?;
    let wide = w.register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 20))?;
    let eq = w.register_symbol(&WriteSymbol::new(b"E", SymbolType::Equation, 0))?;
    w.register_symbol(&WriteSymbol::new(b"Empty", SymbolType::Parameter, 0))?;
    let mut spec = WriteSymbol::new(b"All", SymbolType::Alias, 1);
    spec.alias_target = Some(SymbolReference::Universe);
    spec.description = b"Aliased with *";
    w.register_symbol(&spec)?;
    let one = Value::NumberBits(1f64.to_bits());
    let mut s = begin(&mut w, set, unsorted)?;
    if unsorted {
        s.write(&[ids[2]], &[Value::ZERO])?;
        s.write(&[ids[0]], &[one])?;
    } else {
        s.write(&[ids[0]], &[one])?;
        s.write(&[ids[2]], &[Value::ZERO])?;
    }
    s.finish()?;
    // Deliberately author data sections in a different order from symbol IDs.
    let mut s = begin(&mut w, wide, unsorted)?;
    s.write(
        &[ids[0]; 20],
        &[
            one,
            Value::NumberBits((-1f64).to_bits()),
            Value::Special(SpecialValue::NegativeInfinity),
            Value::Special(SpecialValue::PositiveInfinity),
            Value::NumberBits(2.5f64.to_bits()),
        ],
    )?;
    s.finish()?;
    let mut s = begin(&mut w, eq, unsorted)?;
    s.write(
        &[],
        &[
            Value::Special(SpecialValue::NotAvailable),
            Value::ZERO,
            Value::ZERO,
            Value::ZERO,
            one,
        ],
    )?;
    s.finish()?;
    let mut s = begin(&mut w, p, unsorted)?;
    let first = ([ids[0], ids[3]], Value::Special(SpecialValue::Undefined));
    let second = (
        [ids[2], ids[1]],
        Value::Acronym {
            id: AcronymId::new(7)?,
            encoded_bits: (7.0 * 10.0e300f64).to_bits(),
        },
    );
    for (keys, value) in if unsorted {
        [second, first]
    } else {
        [first, second]
    } {
        s.write(&keys, &[value])?;
    }
    s.finish()?;
    let mut s = begin(&mut w, relaxed, unsorted)?;
    s.write(&[ids[1], ids[0]], &[Value::Special(SpecialValue::Epsilon)])?;
    s.finish()?;
    let stats = w.finish()?;
    assert!(stats.published);
    assert_eq!(w.finish()?, stats);
    Ok(())
}

// Fixed-size caller-side boxes for authoring controls only (under 2 KiB each).
// Their library control allowances are already charged; no records are collected.
enum Rows<'a> {
    Sorted(Box<SymbolWriter<'a>>),
    Unsorted(Box<UnsortedSymbolWriter<'a>>),
}
fn begin(w: &mut Writer, id: WriterSymbolId, unsorted: bool) -> Result<Rows<'_>> {
    if unsorted {
        Ok(Rows::Unsorted(Box::new(w.begin_unsorted_symbol(id, None)?)))
    } else {
        Ok(Rows::Sorted(Box::new(w.begin_symbol(id, None)?)))
    }
}
impl Rows<'_> {
    fn write(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<()> {
        match self {
            Self::Sorted(s) => s.write(keys, values),
            Self::Unsorted(s) => s.write(keys, values),
        }
    }
    fn finish(self) -> Result<u32> {
        match self {
            Self::Sorted(s) => (*s).finish(),
            Self::Unsorted(s) => {
                let stats = (*s).finish()?;
                eprintln!(
                    "unsorted_records={} merge_passes={}",
                    stats.records, stats.spill.merge_passes
                );
                Ok(stats.records)
            }
        }
    }
}
