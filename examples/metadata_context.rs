//! Fixed task-029 metadata session recipe, streamed without a retained dictionary.
use gdxrs::*;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("input")?;
    let scratch = args.next().ok_or("scratch")?;
    let view = args.next().ok_or("base/view")? == "view";
    let r = Reader::open(
        path,
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )?;
    let mut o = MetadataOptions::new(scratch);
    o.maximum_open_files = 8;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    let mut map = r.mapped_uels(&o)?;
    map.register(
        [(1, 9), (2, 0), (3, 3)]
            .map(|(raw, mapped)| Ok((UelId::new(raw)?, MappedUelId::new(mapped)?))),
    )?;
    let mut session = r.metadata_session(&o)?;
    if view {
        session.rename_uel(UelId::new(1)?, b"session-label")?;
        session.rename_uel(UelId::new(3)?, b"session-gamma")?;
        session.set_text_node(0, i32::MAX)?;
        session.set_text_node(1, i32::MIN)?;
        if session.set_text_node(1, 7)? {
            return Err("node overwritten".into());
        }
    }
    println!("GDXRS-METADATA-CONTEXT\t1");
    let info = map.information()?;
    println!(
        "M\t{}\t{}",
        info.uel_count,
        info.highest_mapped.map_or(-1, |id| i64::from(id.get()))
    );
    session.visit_uels(|mut info| {
        info.mapped = map.mapped(info.id)?;
        println!(
            "U\t{}\t{}\t{}",
            info.id.get(),
            info.mapped.map_or(-1, |id| i64::from(id.get())),
            hex(info.label())
        );
        Ok(())
    })?;
    let raw = map.raw(MappedUelId::new(9)?)?.ok_or("inverse")?;
    println!(
        "G\t9\t{}",
        hex(map.uel_in_session(&mut session, raw)?.label())
    );
    let raw = session
        .find_uel(if view {
            b"session-label"
        } else {
            b"alpha-long-label"
        })?
        .ok_or("find")?;
    println!(
        "F\t{}\t{}",
        raw.get(),
        map.mapped(raw)?.ok_or("mapping")?.get()
    );
    for id in 0..3 {
        let info = session.set_text_information(id)?;
        println!("T\t{id}\t{}\t{}", info.node, hex(info.text()));
    }
    for id in 1..=5 {
        println!(
            "H\t{id}\t{}",
            u8::from(r.symbol(SymbolId::new(id)?)?.metadata()?.has_set_text)
        );
    }
    let h = r.symbol(SymbolId::new(4)?)?;
    let lengths = session.symbol_index_max_lengths(&h)?;
    println!(
        "L\t{}\t{}\t{}\t{},{}",
        session.uel_max_length()?,
        r.symbol_max_length()?,
        lengths.iter().max().ok_or("lengths")?,
        lengths[0],
        lengths[1]
    );
    let stats = map.visit_domain(&h, 0, |raw, mapped| {
        println!("D\t{}\t{}", raw.get(), mapped.get());
        Ok(())
    })?;
    println!("E\t{}", stats.visited);
    session.close()?;
    map.close()?;
    eprintln!(
        "{{\"peak_root\":{},\"peak_handles\":{}}}",
        r.budget_statistics().peak_bytes,
        map.statistics()
            .peak_open_files
            .max(session.statistics().peak_open_files)
    );
    Ok(())
}
