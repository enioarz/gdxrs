//! Development-only Linux task-033 containment worker.
#[cfg(feature = "index")]
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(not(feature = "index"))]
fn main() {
    eprintln!("requires index feature");
    std::process::exit(1);
}
#[cfg(feature = "index")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use gdxrs::*;
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 6 {
        return Err("usage: build|open SOURCE INDEX COUNT CAP_BYTES".into());
    }
    let mode = args[1].to_str().ok_or("mode")?;
    let n: u64 = args[4].to_str().ok_or("count")?.parse()?;
    let cap: u64 = args[5].to_str().ok_or("cap")?.parse()?;
    if !(1024..=2_000_000).contains(&n) || cap != 64 * 1024 * 1024 {
        return Err("fixed development containment recipe".into());
    }
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let fields: Vec<_> = limits
        .lines()
        .find(|l| l.starts_with("Max address space"))
        .ok_or("limit")?
        .split_whitespace()
        .collect();
    if fields.get(3).ok_or("soft")?.parse::<u64>()? != cap
        || fields.get(4).ok_or("hard")?.parse::<u64>()? != cap
    {
        return Err("OS cap not enforced".into());
    }
    let reader = Reader::open(
        &args[2],
        OpenOptions {
            memory_budget_bytes: 8 * 1024 * 1024,
            metadata_cache_bytes: 0,
            maximum_batch_bytes: 4096,
        },
    )?;
    let symbol = reader.symbol(SymbolId::new(1)?)?;
    let options = IndexOptions {
        record_stride: 1,
        decoded_byte_stride: u64::MAX,
        maximum_index_bytes: 2 * 1024 * 1024 * 1024,
        ..IndexOptions::default()
    };
    let stats = if mode == "build" {
        let mut owner = reader.index_builder(&symbol, &args[3], options)?;
        owner.finish()?
    } else if mode == "open" {
        let mut index = reader
            .open_index(&symbol, &args[3], options, IndexPolicy::UseIfValid)?
            .index
            .ok_or("index rejected")?;
        for ordinal in [0, 1, 15, 16, 255, 256, 65535, n / 2, n - 1] {
            if ordinal >= n {
                continue;
            }
            let interval = index.interval(ordinal)?;
            if interval.start != ordinal || interval.end != ordinal + 1 {
                return Err("ordinal interval".into());
            }
            let mut scan = reader.scan_from_checkpoint(
                &symbol,
                &interval.checkpoint,
                ScanOptions {
                    limit: Some(1),
                    ..reader.scan_options()
                },
            )?;
            let batch = scan.next_batch()?.ok_or("record")?;
            let row = batch.row(0).ok_or("row")?;
            for d in 0..20 {
                let expected = if d == 0 {
                    ordinal / 1024 + 1
                } else if d == 19 {
                    ordinal % 1024 + 1
                } else {
                    1
                };
                if u64::from(row.key(d).ok_or("key")?.get()) != expected {
                    return Err("native recipe keys".into());
                }
            }
            for (f, field) in ValueField::ALL.iter().enumerate() {
                let bits = 0x3ff0000000000000
                    | ((ordinal * 17 + f as u64 * 12345 + 0x1640) & 0xfffffffffffff);
                if row.value(*field) != Some(Value::NumberBits(bits)) {
                    return Err("native recipe value bits".into());
                }
            }
            if scan.next_batch()?.is_some() {
                return Err("suffix limit".into());
            }
        }
        index.statistics()
    } else {
        return Err("mode".into());
    };
    let root = reader.budget_statistics();
    if !stats.complete
        || stats.intervals != n
        || stats.rows_decoded != n
        || root.peak_bytes > 8 * 1024 * 1024
    {
        return Err("worker completion counts/root".into());
    }
    println!(
        "{{\"count\":{n},\"intervals\":{},\"directory_bytes\":{},\"index_bytes\":{},\"root_peak\":{},\"root_limit\":{},\"source_bytes_hashed\":{},\"digest_seconds\":{},\"replay_seconds\":{},\"pages_read\":{},\"pages_written\":{},\"os_cap_verified\":true}}",
        stats.intervals,
        stats.directory_pages * 1024,
        stats.index_bytes,
        root.peak_bytes,
        root.limit_bytes,
        stats.source_bytes_hashed,
        stats.digest_time.as_secs_f64(),
        stats.replay_time.as_secs_f64(),
        stats.pages_read,
        stats.pages_written
    );
    Ok(())
}
