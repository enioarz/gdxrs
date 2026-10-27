//! Explicit per-session projection; core scan remains typed and streaming.
use gdxrs::*;
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("input")?;
    let scratch = args.next().ok_or("scratch")?;
    let mode = args.next().ok_or("mode")?;
    let mode = mode.to_str().ok_or("UTF-8 mode")?;
    let r = Reader::open(
        path,
        OpenOptions {
            memory_budget_bytes: 1024 * 1024,
            ..OpenOptions::default()
        },
    )?;
    let mut o = MetadataOptions::new(scratch);
    o.maximum_open_files = 6;
    o.merge_fan_in = 2;
    o.run_bytes = 512;
    let mut session = r.value_session(&o)?;
    if mode != "identity" {
        session
            .set_special_values([-101.0f64, -102.0, -103.0, -104.0, -105.0].map(f64::to_bits))?;
        session.set_read_special_values(
            if mode == "loss" {
                [0x7ff8000000001234; 5]
            } else {
                [-201.0f64, -202.0, -203.0, -204.0, -205.0].map(f64::to_bits)
            },
            if mode == "loss" {
                ReadCollisions::AllowLoss
            } else {
                ReadCollisions::Reject
            },
        )?;
        session.set_acronym_mapping(AcronymId::new(7)?, AcronymId::new(70)?)?;
        session.set_next_acronym_id(Some(AcronymId::new(if mode == "collision" {
            70
        } else {
            100
        })?))?;
    }
    if mode == "reset" {
        session.reset_special_values()?;
    }
    if mode == "loss" {
        session.set_acronym_policy(AcronymReadPolicy::ToNotAvailable)?;
    }
    let h = r.symbol(SymbolId::new(1)?)?;
    let mut scan = r.scan(&h, r.scan_options())?;
    println!("GDXRS-VALUE-PROJECTION\t1");
    while let Some(batch) = scan.next_batch()? {
        for row in 0..batch.len() {
            print!("R\t{}\t", batch.key_column(0).ok_or("keys")?[row].get());
            for (f, field) in ValueField::ALL.into_iter().enumerate() {
                let original = batch.value_column(field).ok_or("values")?[row];
                let projected = session.project(original)?;
                if projected.original != original {
                    return Err("projection changed original".into());
                }
                print!(
                    "{}{:016x}",
                    if f == 0 { "" } else { "," },
                    projected.to_f64(FloatConversion::AllowLoss)?.to_bits()
                );
            }
            println!();
        }
    }
    println!("N\t{}", session.next_acronym_id().map_or(0, AcronymId::get));
    session.close()?;
    eprintln!(
        "{{\"peak_root\":{},\"peak_handles\":{}}}",
        r.budget_statistics().peak_bytes,
        session.statistics().peak_open_files
    );
    Ok(())
}
