//! Development-only full-scan phase timer. Never collects records.
use gdxrs::*;
use std::{error::Error, time::Instant};
type R<T> = std::result::Result<T, Box<dyn Error>>;
const ROOT: usize = 256 * 1024 * 1024;
const CAP: u64 = 512 * 1024 * 1024;
fn require(ok: bool, message: &str) -> R<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn run() -> R<()> {
    let args: Vec<_> = std::env::args_os().collect(); // Exactly two bounded arguments.
    require(args.len() == 3, "usage: m6_profile_worker SOURCE COUNT")?;
    let n: u64 = args[2].to_str().ok_or("count")?.parse()?;
    require((1024..=4_000_000).contains(&n), "bounded profile recipe")?;
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let fields: Vec<_> = limits
        .lines()
        .find(|s| s.starts_with("Max address space"))
        .ok_or("address-space limit")?
        .split_whitespace()
        .collect();
    require(
        fields.get(3).ok_or("soft")?.parse::<u64>()? == CAP
            && fields.get(4).ok_or("hard")?.parse::<u64>()? == CAP,
        "OS cap not enforced",
    )?;
    let total = Instant::now();
    let reader = Reader::open(
        &args[1],
        OpenOptions {
            memory_budget_bytes: ROOT,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 1024 * 1024,
        },
    )?;
    let symbol = reader.symbol(SymbolId::new(1)?)?;
    let open_seconds = total.elapsed().as_secs_f64();
    let setup = Instant::now();
    let mut cursor = reader.scan(&symbol, reader.scan_options())?;
    let setup_seconds = setup.elapsed().as_secs_f64();
    let mut batch_seconds = 0.;
    let mut sink_seconds = 0.;
    let mut count = 0u64;
    let mut digest = 0xcbf29ce484222325u64;
    let mut batches = 0u64;
    let mut first_seconds = None;
    loop {
        let batch_start = Instant::now();
        let next = cursor.next_batch()?;
        batch_seconds += batch_start.elapsed().as_secs_f64();
        let Some(batch) = next else { break };
        if first_seconds.is_none() {
            first_seconds = Some(total.elapsed().as_secs_f64());
        }
        batches += 1;
        let sink_start = Instant::now();
        for at in 0..batch.len() {
            require(count < n, "extra record")?;
            let row = batch.row(at).ok_or("row")?;
            for d in 0..20 {
                let expected = match d {
                    0 => count / 1024 + 1,
                    19 => count % 1024 + 1,
                    _ => 1,
                };
                let actual = u64::from(row.key(d).ok_or("key")?.get());
                require(actual == expected, "record key/order mismatch")?;
                digest = (digest.rotate_left(7) ^ actual).wrapping_mul(0x9e3779b185ebca87);
            }
            for (f, field) in ValueField::ALL.iter().enumerate() {
                let expected = 0x3ff0000000000000
                    | ((count * 17 + f as u64 * 12345 + 0x1640) & 0xfffffffffffff);
                require(
                    row.value(*field) == Some(Value::NumberBits(expected)),
                    "value bits mismatch",
                )?;
                digest = (digest.rotate_left(7) ^ expected).wrapping_mul(0x9e3779b185ebca87);
            }
            count += 1;
        }
        sink_seconds += sink_start.elapsed().as_secs_f64();
    }
    require(count == n, "missing records")?;
    let work = cursor.statistics();
    let close = Instant::now();
    drop(cursor);
    let close_seconds = close.elapsed().as_secs_f64();
    let seconds = total.elapsed().as_secs_f64();
    let budget = reader.budget_statistics();
    let io = reader.io_statistics();
    println!(
        "{{\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{seconds},\"open_seconds\":{open_seconds},\"setup_seconds\":{setup_seconds},\"batch_seconds\":{batch_seconds},\"sink_seconds\":{sink_seconds},\"close_seconds\":{close_seconds},\"first_seconds\":{},\"batches\":{batches},\"decoded_rows\":{},\"returned_rows\":{},\"read_calls\":{},\"bytes_read\":{},\"decoded_bytes\":{},\"decoded_buffers\":{},\"root_limit\":{ROOT},\"root_peak\":{},\"os_cap_verified\":true}}",
        first_seconds.ok_or("empty recipe")?,
        work.decoded_rows,
        work.returned_rows,
        io.read_calls,
        io.bytes_read,
        io.decoded_bytes,
        io.decoded_buffers,
        budget.peak_bytes,
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
