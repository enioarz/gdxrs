//! Linux-only development worker for task-039 measured scan policy comparisons.
//! Measures serial/indexed/parallel once per case, then executes planned scans.
#[cfg(not(feature = "index"))]
fn main() {
    eprintln!("requires index feature");
    std::process::exit(1);
}
#[cfg(feature = "index")]
fn main() {
    if let Err(error) = worker::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(feature = "index")]
mod worker {
    use gdxrs::*;
    use std::{error::Error, time::Instant};
    type R<T> = std::result::Result<T, Box<dyn Error>>;
    const ROOT: usize = 256 * 1024 * 1024;
    const CAP: u64 = 512 * 1024 * 1024;
    const INITIAL: u64 = 0xcbf29ce484222325;
    fn step(h: u64, v: u64) -> u64 {
        (h.rotate_left(7) ^ v).wrapping_mul(0x9e3779b185ebca87)
    }
    fn require(ok: bool, message: &str) -> R<()> {
        if ok { Ok(()) } else { Err(message.into()) }
    }
    struct Ctx {
        metadata: bool,
        n: u64,
    }
    fn consume(ctx: &Ctx, count: u64, row: &RecordView<'_>, digest: &mut u64) -> R<()> {
        if ctx.metadata {
            let key = row.key(0).ok_or("universe key")?.get();
            require(
                u64::from(key) == count + 1 && row.value(ValueField::Level) == Some(Value::ZERO),
                "universe identity/value bits",
            )?;
            *digest = step(step(*digest, u64::from(key)), 0);
        } else {
            for d in 0..20 {
                let expected = match d {
                    0 => count / 1024 + 1,
                    19 => count % 1024 + 1,
                    _ => 1,
                };
                let actual = u64::from(row.key(d).ok_or("key")?.get());
                require(actual == expected, "record key/order mismatch")?;
                *digest = step(*digest, actual);
            }
            for (f, field) in ValueField::ALL.iter().enumerate() {
                let expected = 0x3ff0000000000000
                    | ((count * 17 + f as u64 * 12345 + 0x1640) & 0xfffffffffffff);
                require(
                    row.value(*field) == Some(Value::NumberBits(expected)),
                    "value bits",
                )?;
                *digest = step(*digest, expected);
            }
        }
        Ok(())
    }
    fn drain(ctx: &Ctx, cursor: &mut PlannedScan, ordinal_base: u64) -> R<(u64, u64)> {
        let mut count = 0u64;
        let mut digest = INITIAL;
        while let Some(batch) = cursor.next_batch()? {
            for at in 0..batch.len() {
                let ordinal = ordinal_base + count;
                require(ordinal < ctx.n, "extra record")?;
                consume(ctx, ordinal, &batch.row(at).ok_or("row")?, &mut digest)?;
                count += 1;
            }
        }
        Ok((count, digest))
    }
    fn recover(scan: PlannedScan) -> R<CheckpointIndex> {
        match scan {
            PlannedScan::Serial(cursor) => cursor.into_index().ok_or("lost index owner".into()),
            PlannedScan::Parallel(cursor) => Ok(cursor.into_index()?),
        }
    }
    fn timed<F: FnOnce() -> R<(u64, u64)>>(f: F) -> R<(f64, u64, u64)> {
        let start = Instant::now();
        let (count, digest) = f()?;
        Ok((start.elapsed().as_secs_f64(), count, digest))
    }
    pub fn run() -> R<()> {
        let args: Vec<_> = std::env::args_os().collect();
        require(
            args.len() == 5,
            "usage: m6_policy_worker SOURCE INDEX records|metadata COUNT",
        )?;
        let source = args[1].clone();
        let index_path = args[2].clone();
        let metadata = match args[3].to_str() {
            Some("records") => false,
            Some("metadata") => true,
            _ => return Err("kind".into()),
        };
        let n: u64 = args[4].to_str().ok_or("count")?.parse()?;
        let stride: u64 = 65536;
        require((1024..=40_000_000).contains(&n), "bounded policy recipe")?;
        let limits = std::fs::read_to_string("/proc/self/limits")?;
        let fields: Vec<_> = limits
            .lines()
            .find(|l| l.starts_with("Max address space"))
            .ok_or("limit")?
            .split_whitespace()
            .collect();
        require(
            fields.get(3).ok_or("soft")?.parse::<u64>()? == CAP
                && fields.get(4).ok_or("hard")?.parse::<u64>()? == CAP,
            "process cap not enforced",
        )?;
        let reader = Reader::open(
            &source,
            OpenOptions {
                memory_budget_bytes: ROOT,
                metadata_cache_bytes: 0,
                maximum_batch_bytes: 1024 * 1024,
            },
        )?;
        let symbol = reader.symbol(SymbolId::new(if metadata { 0 } else { 1 })?)?;
        let ctx = Ctx { metadata, n };
        let index_options = IndexOptions {
            record_stride: stride,
            maximum_index_bytes: 12 * 1024 * 1024 * 1024,
            maximum_open_files: 2,
            page_cache_bytes: 1024,
            ..IndexOptions::default()
        };
        let empty = reader.selection(&symbol, &[])?;
        let (serial_seconds, serial_count, serial_digest) = timed(|| {
            let mut scan = PlannedScan::Serial(Box::new(reader.scan_selected(
                &symbol,
                &empty,
                reader.scan_options(),
            )?));
            drain(&ctx, &mut scan, 0)
        })?;
        require(serial_count == n, "serial count")?;
        let build_start = Instant::now();
        let mut builder = reader.index_builder(&symbol, &index_path, index_options)?;
        let built = builder.finish()?;
        let build_seconds = build_start.elapsed().as_secs_f64();
        require(
            built.complete && built.published && built.rows_decoded == n,
            "build",
        )?;
        drop(builder);
        let reopen_start = Instant::now();
        let mut index = reader
            .open_index(&symbol, &index_path, index_options, IndexPolicy::UseIfValid)?
            .index
            .ok_or("strict index unavailable")?;
        let reopen_seconds = reopen_start.elapsed().as_secs_f64();
        let (indexed_seconds, indexed_count, indexed_digest) = timed(|| {
            let mut scan = PlannedScan::Serial(Box::new(reader.scan_selected_indexed(
                &symbol,
                &empty,
                reader.scan_options(),
                IndexOpen {
                    status: IndexStatus::Ready,
                    index: Some(index),
                },
            )?));
            drain(&ctx, &mut scan, 0)
        })?;
        require(
            indexed_count == n && indexed_digest == serial_digest,
            "indexed full digest mismatch",
        )?;
        index = reader
            .open_index(&symbol, &index_path, index_options, IndexPolicy::UseIfValid)?
            .index
            .ok_or("strict index unavailable")?;
        let (k_first, k_last) = if metadata {
            (n / 2, (n / 2 + 31).min(n))
        } else {
            let k = (n / 2) / 1024 + 1;
            (k, k + 1)
        };
        let leading_selection = reader.selection(
            &symbol,
            &[DimensionSelection {
                dimension: 0,
                keys: KeySelection::Range(UelRange::new(
                    UelId::new(u32::try_from(k_first)?)?,
                    UelId::new(u32::try_from(k_last)?)?,
                )?),
            }],
        )?;
        let (leading_seconds, leading_count, leading_digest) = timed(|| {
            let mut scan = PlannedScan::Serial(Box::new(reader.scan_selected_indexed(
                &symbol,
                &leading_selection,
                reader.scan_options(),
                IndexOpen {
                    status: IndexStatus::Ready,
                    index: Some(index),
                },
            )?));
            drain(
                &ctx,
                &mut scan,
                if metadata {
                    k_first - 1
                } else {
                    (k_first - 1) * 1024
                },
            )
        })?;
        index = reader
            .open_index(&symbol, &index_path, index_options, IndexPolicy::UseIfValid)?
            .index
            .ok_or("strict index unavailable")?;
        let mut parallel_seconds = Vec::new();
        let mut parallel_tasks = Vec::new();
        let index_intervals = index.interval_count();
        let mut parallel_digest = 0u64;
        for workers in [2usize, 4, 8] {
            let start = Instant::now();
            let mut scan = PlannedScan::Parallel(Box::new(reader.scan_selected_parallel(
                &symbol,
                &empty,
                index,
                reader.scan_options(),
                ParallelOptions {
                    workers,
                    maximum_in_flight_tasks: workers,
                    ..ParallelOptions::default()
                },
            )?));
            let (count, digest) = drain(&ctx, &mut scan, 0)?;
            let PlannedStatistics::Parallel(work) = scan.statistics() else {
                return Err("parallel calibration statistics missing".into());
            };
            // Calibration uses the same default task grouping as scan_planned.
            require(
                work.tasks_dispatched == index_intervals,
                "calibration task grouping",
            )?;
            parallel_tasks.push(work.tasks_dispatched.to_string());
            index = recover(scan)?;
            let seconds = start.elapsed().as_secs_f64();
            require(
                count == n && digest == serial_digest,
                "parallel digest mismatch",
            )?;
            parallel_digest = digest;
            parallel_seconds.push(seconds);
        }
        let mut shapes = Vec::new();
        for (name, selection, limit, selected_estimate, base) in [
            ("all", &empty, None, n, 0u64),
            (
                "leading",
                &leading_selection,
                None,
                if metadata { 32 } else { 2048 },
                if metadata {
                    k_first - 1
                } else {
                    (k_first - 1) * 1024
                },
            ),
            ("head", &empty, Some(13), 13, 0),
            ("zero", &empty, Some(0), 0, 0),
        ] {
            let measurements = ScanMeasurements {
                rows: n,
                selected_rows: selected_estimate,
                session_valid: true,
                serial_full_scan_seconds: serial_seconds,
                indexed_selected_seconds: if name == "leading" {
                    leading_seconds
                } else {
                    indexed_seconds
                },
                parallel_full_scan_seconds: parallel_seconds[2],
                strict_reopen_seconds: reopen_seconds,
                build_seconds,
            };
            let policy = ScanPolicy::default();
            let again = policy.plan(&measurements, limit)?;
            let construct = Instant::now();
            let (plan, mut scan) = reader.scan_planned(
                &symbol,
                selection,
                ScanOptions {
                    limit,
                    ..reader.scan_options()
                },
                Some(IndexOpen {
                    status: IndexStatus::Ready,
                    index: Some(index),
                }),
                &policy,
                &measurements,
            )?;
            require(plan.choice == again.choice, "policy not deterministic")?;
            let (count, digest) = drain(&ctx, &mut scan, base)?;
            let seconds = construct.elapsed().as_secs_f64();
            index = match scan {
                PlannedScan::Serial(_) => reader
                    .open_index(&symbol, &index_path, index_options, IndexPolicy::UseIfValid)?
                    .index
                    .ok_or("strict index unavailable")?,
                PlannedScan::Parallel(cursor) => cursor.into_index()?,
            };
            let expected_count = match name {
                "all" => n,
                "leading" => leading_count,
                "head" => 13.min(n),
                _ => 0,
            };
            require(count == expected_count, "planned count")?;
            if name == "all" || name == "leading" {
                let expected = if name == "all" {
                    serial_digest
                } else {
                    leading_digest
                };
                require(digest == expected, "planned digest mismatch")?;
            }
            let workers = match plan.choice {
                ScanChoice::Parallel { workers } => workers,
                _ => 0,
            };
            shapes.push(format!(
                "{{\"shape\":\"{name}\",\"choice\":\"{}\",\"workers\":{workers},\"estimated_seconds\":{},\"seconds\":{seconds},\"count\":{count},\"digest\":\"{digest:016x}\",\"break_even\":{}}}",
                match plan.choice {
                    ScanChoice::Serial => "serial",
                    ScanChoice::Indexed => "indexed",
                    ScanChoice::Parallel { .. } => "parallel",
                },
                plan.estimated_seconds,
                match plan.build_break_even_queries {
                    Some(q) => q.to_string(),
                    None => "null".to_string(),
                }
            ));
        }
        let vm: u64 = std::fs::read_to_string("/proc/self/status")?
            .lines()
            .find(|l| l.starts_with("VmPeak:"))
            .ok_or("VmPeak")?
            .split_whitespace()
            .nth(1)
            .ok_or("VmPeak number")?
            .parse::<u64>()?
            * 1024;
        let budget = reader.budget_statistics();
        require(vm <= CAP && budget.peak_bytes <= ROOT, "cap exceeded")?;
        println!(
            "{{\"count\":{n},\"serial_seconds\":{serial_seconds},\"serial_digest\":\"{serial_digest:016x}\",\"build_seconds\":{build_seconds},\"reopen_seconds\":{reopen_seconds},\"indexed_seconds\":{indexed_seconds},\"leading_seconds\":{leading_seconds},\"leading_count\":{leading_count},\"leading_digest\":\"{leading_digest:016x}\",\"parallel_seconds\":[{}],\"parallel_tasks_dispatched\":[{}],\"index_intervals\":{index_intervals},\"parallel_digest\":\"{parallel_digest:016x}\",\"shapes\":[{}],\"root_limit\":{ROOT},\"root_peak\":{},\"os_cap_verified\":true,\"vm_peak_bytes\":{vm}}}",
            parallel_seconds
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(","),
            parallel_tasks.join(","),
            shapes.join(","),
            budget.peak_bytes
        );
        Ok(())
    }
}
