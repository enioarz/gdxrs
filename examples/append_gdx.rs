//! Fixed small task-030 interoperability recipe, or streaming catalog-preserving rewrite.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source = args.next().ok_or("source")?;
    let destination = args.next().ok_or("destination")?;
    let scratch = args.next().ok_or("scratch")?;
    let compression = match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("raw") => WriteCompression::None,
        Some("zlib") => WriteCompression::Zlib,
        _ => return Err("raw/zlib".into()),
    };
    let policy = match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("new") => AppendPolicy::CreateNew,
        Some("replace") => AppendPolicy::ReplaceSource,
        _ => return Err("new/replace".into()),
    };
    let add = match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("add") => true,
        Some("only") => false,
        _ => return Err("add/only".into()),
    };
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let root = 4 * 1024 * 1024;
    let mut admission = AppendOptions::new(policy);
    admission.maximum_transaction_bytes = 12 * 1024 * 1024;
    admission.maximum_open_files = 10;
    let mut w = Writer::open_append(
        source,
        destination,
        OpenOptions {
            memory_budget_bytes: root,
            ..OpenOptions::default()
        },
        WriteOptions {
            memory_budget_bytes: root,
            maximum_output_bytes: 4 * 1024 * 1024,
            compression,
        },
        admission,
    )?;
    let mut limits = MetadataOptions::new(scratch);
    limits.maximum_temporary_bytes = 8 * 1024 * 1024;
    limits.maximum_open_files = 8;
    limits.run_bytes = 512;
    limits.merge_fan_in = 2;
    w.begin_append(&limits)?;
    if add {
        let old = w.append_symbol_id(SymbolId::new(2)?)?;
        let first = w.append_uel_id(UelId::new(1)?)?;
        let omega = w.register_uel(b"omega")?;
        let text = w.add_set_text(b"new text")?;
        let acro = AcronymId::new(9)?;
        w.add_acronym(acro, b"AddedAcro", b"appended")?;
        let set = w.register_symbol(&WriteSymbol::new(b"NewSet", SymbolType::Set, 1))?;
        let param = w.register_symbol(&WriteSymbol::new(b"NewParam", SymbolType::Parameter, 2))?;
        w.set_domains(
            param,
            &[
                WriteDomain::Strict(SymbolReference::Registered(old)),
                WriteDomain::Strict(SymbolReference::Registered(set)),
            ],
        )?;
        w.add_comment(param, b"appended comment")?;
        let mut alias = WriteSymbol::new(b"NewAlias", SymbolType::Alias, 1);
        alias.alias_target = Some(SymbolReference::Registered(set));
        alias.description = b"Aliased with NewSet";
        w.register_symbol(&alias)?;
        let mut rows = w.begin_symbol(set, None)?;
        rows.write(&[omega], &[Value::NumberBits(f64::from(text).to_bits())])?;
        rows.finish()?;
        let mut rows = w.begin_symbol(param, None)?;
        rows.write(&[first, omega], &[acro.value()?])?;
        rows.finish()?;
    }
    let done = w.finish()?;
    let a = w.append_statistics().ok_or("append stats")?;
    let m = w.metadata_statistics();
    println!(
        "{{\"published\":{},\"source_bytes\":{},\"rewritten_records\":{},\"output_bytes\":{},\"peak_root\":{},\"peak_scratch\":{},\"admitted_transaction_bytes\":{},\"admitted_open_files\":{},\"peak_scratch_handles\":{}}}",
        done.published,
        a.source_bytes,
        a.rewritten_records,
        done.output_bytes,
        w.budget_statistics().peak_bytes,
        m.peak_temporary_bytes,
        a.admitted_transaction_bytes,
        a.admitted_open_files,
        m.peak_open_files
    );
    Ok(())
}
