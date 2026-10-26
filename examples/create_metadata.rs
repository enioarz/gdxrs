//! Bounded metadata registration; symbols contain no physical records.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .ok_or("usage: create_metadata NEW.gdx SCRATCH [zlib]")?;
    let scratch = args.next().ok_or("missing scratch parent")?;
    let compression = match args.next().as_deref() {
        None => WriteCompression::None,
        Some(v) if v == "zlib" => WriteCompression::Zlib,
        _ => return Err("invalid compression".into()),
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut writer = Writer::create(
        output,
        WriteOptions {
            compression,
            ..WriteOptions::default()
        },
    )?;
    let mut limits = MetadataOptions::new(scratch);
    limits.run_bytes = 1024;
    limits.merge_fan_in = 2;
    limits.maximum_open_files = 6;
    writer.begin_metadata(&limits)?;
    let labels: [&[u8]; 8] = [
        b"z",
        b"a",
        b"z",
        b"A",
        b"\x80label",
        b" space ",
        b"",
        b"single'",
    ];
    let ids = [1, 2, 1, 3, 4, 5, 6, 7];
    writer.register_uels_fresh(labels.into_iter().map(Ok), |ordinal, id| {
        if id.get() != ids[ordinal as usize - 1] {
            return Err(std::io::Error::other("wrong registration ID").into());
        }
        Ok(())
    })?;
    let mut alias = WriteSymbol::new(b"Alias", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Name(b"I"));
    writer.register_symbol(&alias)?;
    let mut set = WriteSymbol::new(b"I", SymbolType::Set, 1);
    set.description = b"domain set";
    let set = writer.register_symbol(&set)?;
    writer.add_comment(set, b"first")?;
    writer.add_comment(set, b"second")?;
    let mut scalar = WriteSymbol::new(b"Scalar", SymbolType::Parameter, 0);
    scalar.description = b"scalar";
    writer.register_symbol(&scalar)?;
    let parameter = writer.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?;
    writer.set_domains(
        parameter,
        &[
            WriteDomain::Strict(SymbolReference::Name(b"i")),
            WriteDomain::Strict(SymbolReference::Universe),
        ],
    )?;
    let variable = writer.register_symbol(&WriteSymbol::new(b"V", SymbolType::Variable, 1))?;
    writer.set_domains(variable, &[WriteDomain::Relaxed(b"future")])?;
    writer.register_symbol(&WriteSymbol::new(b"E", SymbolType::Equation, 0))?;
    let mut alias = WriteSymbol::new(b"All", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Universe);
    writer.register_symbol(&alias)?;
    writer.add_set_text(b"note")?;
    writer.add_acronym(AcronymId::new(7)?, b"Acro", b"acronym")?;
    writer.freeze_metadata()?;
    let stats = writer.finish()?;
    println!(
        "bytes={} uels={} symbols={}",
        stats.output_bytes,
        writer.metadata_statistics().uels,
        writer.metadata_statistics().symbols
    );
    Ok(())
}
