//! Linux-only task-023 capped worker. Generators/consumers retain fixed state.
use gdxrs::*;
use std::{path::Path, time::Instant};
const INITIAL: u64 = 14695981039346656037;
const BUDGET: usize = 256 * 1024 * 1024;
const SEED: u64 = 0x1640;
fn step(h: u64, n: u64) -> u64 {
    (h.rotate_left(7) ^ n).wrapping_mul(0x9e3779b185ebca87)
}
fn label(n: u64) -> [u8; 63] {
    let mut b = [b'x'; 63];
    b[0] = b'u';
    let mut n = 1_000_000_000 + n;
    for i in (1..=10).rev() {
        b[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    b
}
fn bits(n: u64, f: u64) -> u64 {
    0x3ff0000000000000 | ((n * 17 + f * 12345 + SEED) & ((1 << 52) - 1))
}
fn execute() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 || args.len() > 7 {
        return Err(
            "usage: scale_writer records|metadata|pipeline OUT SCRATCH COUNT raw|zlib [INPUT]"
                .into(),
        );
    }
    let mode = args[1].as_str();
    if !["records", "metadata", "pipeline"].contains(&mode) {
        return Err("invalid mode".into());
    }
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let cap = limits
        .lines()
        .find(|l| l.starts_with("Max address space"))
        .ok_or("missing OS cap")?;
    if cap.split_whitespace().filter(|s| *s == "536870912").count() != 2 {
        return Err("512 MiB RLIMIT_AS not active".into());
    }
    let count: u64 = args[4].parse()?;
    if count == 0 || count > 100_000_000 {
        return Err("invalid bounded development count".into());
    }
    let labels = if mode == "metadata" {
        count
    } else {
        1024.max(count.div_ceil(1024))
    };
    if mode != "metadata" && labels > 65536 {
        return Err("fixed caller dictionary bound exceeded".into());
    }
    let compression = match args[5].as_str() {
        "raw" => WriteCompression::None,
        "zlib" => WriteCompression::Zlib,
        _ => return Err("invalid compression".into()),
    };
    let reader = if mode == "pipeline" {
        Some(Reader::open(
            args.get(6).ok_or("missing pipeline input")?,
            OpenOptions {
                memory_budget_bytes: BUDGET,
                ..OpenOptions::default()
            },
        )?)
    } else {
        None
    };
    let baseline = reader
        .as_ref()
        .map_or(0, |r| r.budget_statistics().current_bytes);
    let options = WriteOptions {
        memory_budget_bytes: BUDGET,
        compression,
        ..WriteOptions::default()
    };
    let mut w = if let Some(r) = &reader {
        r.create_writer(&args[2], options)?
    } else {
        Writer::create(&args[2], options)?
    };
    let mut scratch = MetadataOptions::new(Path::new(&args[3]));
    scratch.run_bytes = 32 * 1024 * 1024;
    scratch.maximum_temporary_bytes = 16 * 1024 * 1024 * 1024;
    scratch.maximum_open_files = 12;
    scratch.merge_fan_in = 8;
    w.begin_metadata(&scratch)?;
    // Fixed <=1 MiB caller-owned dictionary for record recipes only, independent
    // of row count. Metadata workloads retain no token per label.
    let mut ids = [None; 65536];
    let register = Instant::now();
    let mut label_digest = INITIAL;
    if let Some(r) = &reader {
        if u64::from(r.information()?.uel_count) != labels {
            return Err("pipeline UEL count".into());
        }
        let mut cursor = r.uels()?;
        let mut n = 0;
        let input = std::iter::from_fn(|| match cursor.next() {
            Ok(Some(v)) => {
                n += 1;
                if v.id.get() as u64 != n || v.label != label(n) {
                    return Some(Err(std::io::Error::other("pipeline label identity").into()));
                }
                Some(Ok(label(n)))
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        });
        w.register_uels_fresh(input, |ordinal, id| {
            if ordinal != id.get() as u64 {
                return Err(std::io::Error::other("pipeline original ID").into());
            }
            ids[ordinal as usize - 1] = Some(id);
            Ok(())
        })?;
    } else {
        w.register_uels_fresh(
            (1..=labels).map(|n| {
                let bytes = label(n);
                label_digest = step(label_digest, n);
                for b in bytes {
                    label_digest = step(label_digest, u64::from(b));
                }
                Ok(bytes)
            }),
            |ordinal, id| {
                if ordinal != u64::from(id.get()) {
                    return Err(std::io::Error::other("registration first ID").into());
                }
                if mode != "metadata" {
                    ids[ordinal as usize - 1] = Some(id);
                }
                Ok(())
            },
        )?;
    }
    let registration_seconds = register.elapsed().as_secs_f64();
    let mut digest = INITIAL;
    let mut emitted = 0u64;
    let encoding = Instant::now();
    if mode != "metadata" {
        let mut spec = WriteSymbol::new(b"data", SymbolType::Variable, 20);
        spec.description = b"scale recipe";
        let symbol = w.register_symbol(&spec)?;
        let mut out = w.begin_symbol(symbol, None)?;
        if let Some(r) = &reader {
            let h = r.symbol(SymbolId::new(1)?)?;
            let mut scan = r.scan(&h, r.scan_options())?;
            while let Some(batch) = scan.next_batch()? {
                for n in 0..batch.len() {
                    let mut keys = [ids[0].ok_or("missing UEL")?; 20];
                    let mut values = [Value::ZERO; 5];
                    for (d, key) in keys.iter_mut().enumerate() {
                        let raw = batch.key_column(d as u8).ok_or("missing dimension")?[n].get();
                        *key = ids
                            .get(raw as usize - 1)
                            .copied()
                            .flatten()
                            .ok_or("pipeline key")?;
                        digest = step(digest, u64::from(raw));
                    }
                    for (f, value) in values.iter_mut().enumerate() {
                        *value = batch
                            .value_column(ValueField::ALL[f])
                            .ok_or("missing field")?[n];
                        let actual = value.number_bits().ok_or("ordinary field")?;
                        if actual != bits(emitted, f as u64) {
                            return Err("pipeline value bits".into());
                        }
                        digest = step(digest, actual);
                    }
                    out.write(&keys, &values)?;
                    emitted += 1;
                }
            }
        } else {
            for n in 0..count {
                let mut keys = [ids[0].ok_or("missing UEL")?; 20];
                keys[0] = ids[(n / 1024) as usize].ok_or("leading UEL")?;
                keys[19] = ids[(n % 1024) as usize].ok_or("trailing UEL")?;
                let mut values = [Value::ZERO; 5];
                for key in keys {
                    digest = step(digest, key.get() as u64);
                }
                for (f, value) in values.iter_mut().enumerate() {
                    let b = bits(n, f as u64);
                    *value = Value::NumberBits(b);
                    digest = step(digest, b);
                }
                out.write(&keys, &values)?;
                emitted += 1;
            }
        }
        if emitted != count {
            return Err("emitted count".into());
        }
        out.finish()?;
    }
    let encoding_seconds = encoding.elapsed().as_secs_f64();
    let finish = Instant::now();
    let statistics = w.finish()?;
    let finalization_seconds = finish.elapsed().as_secs_f64();
    let root = w.budget_statistics();
    let scratch = w.metadata_statistics();
    if root.peak_bytes > BUDGET || scratch.temporary_bytes != 0 || scratch.open_files != 0 {
        return Err("resource completion".into());
    }
    drop(w);
    if let Some(r) = reader
        && r.budget_statistics().current_bytes != baseline
    {
        return Err("shared root release".into());
    }
    println!(
        "{{\"count\":{emitted},\"digest\":\"{digest:016x}\",\"labels\":{labels},\"label_digest\":\"{label_digest:016x}\",\"registration_seconds\":{registration_seconds},\"encoding_seconds\":{encoding_seconds},\"finalization_seconds\":{finalization_seconds},\"root_limit\":{},\"root_peak\":{},\"scratch_peak\":{},\"scratch_peak_handles\":{},\"merge_passes\":{},\"output_bytes\":{},\"os_cap_verified\":true,\"shared_root\":{}}}",
        root.limit_bytes,
        root.peak_bytes,
        scratch.peak_temporary_bytes,
        scratch.peak_open_files,
        scratch.merge_passes,
        statistics.output_bytes,
        mode == "pipeline"
    );
    Ok(())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
