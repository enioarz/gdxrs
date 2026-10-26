//! Task 022's fixed-size full metadata/interoperability authoring recipe.
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
    w.begin_metadata(&MetadataOptions::new(scratch))?;
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
    let mut s = w.begin_symbol(set, None)?;
    s.write(&[ids[0]], &[one])?;
    s.write(&[ids[2]], &[Value::ZERO])?;
    s.finish()?;
    // Deliberately author data sections in a different order from symbol IDs.
    let mut s = w.begin_symbol(wide, None)?;
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
    let mut s = w.begin_symbol(eq, None)?;
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
    let mut s = w.begin_symbol(p, None)?;
    s.write(
        &[ids[0], ids[3]],
        &[Value::Special(SpecialValue::Undefined)],
    )?;
    s.write(
        &[ids[2], ids[1]],
        &[Value::Acronym {
            id: AcronymId::new(7)?,
            encoded_bits: (7.0 * 10.0e300f64).to_bits(),
        }],
    )?;
    s.finish()?;
    let mut s = w.begin_symbol(relaxed, None)?;
    s.write(&[ids[1], ids[0]], &[Value::Special(SpecialValue::Epsilon)])?;
    s.finish()?;
    let stats = w.finish()?;
    assert!(stats.published);
    assert_eq!(w.finish()?, stats);
    Ok(())
}
