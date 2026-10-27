//! Fixed independently checked task-029 author recipe.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("output")?;
    let scratch = args.next().ok_or("scratch")?;
    let zlib = args.next().is_some();
    let mut w = Writer::create(
        path,
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
    o.maximum_open_files = 8;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    w.begin_metadata(&o)?;
    w.register_mapped_uels(
        [(0, b"a".as_slice()), (9, b"b"), (3, b"c")]
            .map(|(id, label)| Ok((MappedUelId::new(id)?, label))),
    )?;
    w.register_uel(&[b'Z'; 63])?;
    let keys = [
        w.raw_id_for_mapped(MappedUelId::new(0)?)?,
        w.raw_id_for_mapped(MappedUelId::new(9)?)?,
        w.raw_id_for_mapped(MappedUelId::new(3)?)?,
    ];
    let text = w.add_set_text(b"element text")?;
    w.add_set_text(b"spare")?;
    w.set_text_node(0, i32::MAX)?;
    w.set_text_node(text, i32::MIN)?;
    if w.set_text_node(text, 7)? {
        return Err("node overwritten".into());
    }
    let i = w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?;
    let mut alias = WriteSymbol::new(b"J", SymbolType::Alias, 1);
    alias.alias_target = Some(SymbolReference::Registered(i));
    alias.description = b"Aliased with I";
    let j = w.register_symbol(&alias)?;
    let k = w.register_symbol(&WriteSymbol::new(b"K", SymbolType::Set, 1))?;
    let p = w.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 2))?;
    let r = w.register_symbol(&WriteSymbol::new(b"R", SymbolType::Parameter, 0))?;
    w.set_domains(
        p,
        &[
            WriteDomain::Strict(SymbolReference::Registered(j)),
            WriteDomain::Strict(SymbolReference::Registered(i)),
        ],
    )?;
    let mut rows = w.begin_symbol(i, None)?;
    rows.write(&keys[..1], &[Value::NumberBits(f64::from(text).to_bits())])?;
    rows.write(&keys[1..2], &[Value::ZERO])?;
    rows.finish()?;
    w.add_comment(i, b"after I")?;
    w.rename_uel(keys[0], b"alpha-long-label")?;
    let mut rows = w.begin_symbol(k, None)?;
    rows.write(&keys[2..], &[Value::ZERO])?;
    rows.finish()?;
    w.rename_uel(keys[2], b"gamma")?;
    let mut rows = w.begin_symbol(p, None)?;
    rows.write(&keys[..2], &[Value::Special(SpecialValue::Epsilon)])?;
    rows.write(&[keys[1], keys[0]], &[Value::NumberBits(1.25f64.to_bits())])?;
    rows.finish()?;
    w.set_relaxed_domains(p, &[b"Shown", b"*"])?;
    w.set_relaxed_domains(p, &[b"ShownAgain", b"*"])?;
    w.add_comment(p, b"late P")?;
    let mut rows = w.begin_symbol(r, None)?;
    rows.write(&[], &[Value::NumberBits(42.0f64.to_bits())])?;
    rows.finish()?;
    w.rename_uel(keys[1], b"beta-final")?;
    if w.set_text_information(text)?.node != i32::MIN {
        return Err("writer node lost".into());
    }
    w.finish()?;
    println!(
        "{{\"peak_root\":{},\"peak_handles\":{},\"peak_temporary\":{}}}",
        w.budget_statistics().peak_bytes,
        w.metadata_statistics().peak_open_files,
        w.metadata_statistics().peak_temporary_bytes
    );
    Ok(())
}
