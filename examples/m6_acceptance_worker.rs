//! Linux-only development worker for task-040 M6 ordered parallel acceptance.
//! Never collects records; validates every row and reports bounded diagnostics.
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
                    "value bits mismatch",
                )?;
                *digest = step(*digest, expected);
            }
        }
        Ok(())
    }
    fn check_cap() -> R<()> {
        let text = std::fs::read_to_string("/proc/self/limits")?;
        let fields: Vec<_> = text
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
        Ok(())
    }
    fn open_reader(source: &std::ffi::OsStr) -> R<Reader> {
        Ok(Reader::open(
            source,
            OpenOptions {
                memory_budget_bytes: ROOT,
                metadata_cache_bytes: 0,
                maximum_batch_bytes: 1024 * 1024,
            },
        )?)
    }
    fn vm_peak() -> R<u64> {
        let status = std::fs::read_to_string("/proc/self/status")?;
        Ok(status
            .lines()
            .find(|l| l.starts_with("VmPeak:"))
            .ok_or("VmPeak")?
            .split_whitespace()
            .nth(1)
            .ok_or("VmPeak number")?
            .parse::<u64>()?
            * 1024)
    }
    fn tail(reader: &Reader, vm: u64) -> String {
        format!(
            "\"root_limit\":{ROOT},\"root_peak\":{},\"os_cap_verified\":true,\"vm_peak_bytes\":{vm}",
            reader.budget_statistics().peak_bytes
        )
    }
    fn serial(source: &std::ffi::OsStr, metadata: bool, n: u64) -> R<()> {
        check_cap()?;
        let total = Instant::now();
        let reader = open_reader(source)?;
        let symbol = reader.symbol(SymbolId::new(if metadata { 0 } else { 1 })?)?;
        let open_seconds = total.elapsed().as_secs_f64();
        let setup = Instant::now();
        let selection = reader.selection(&symbol, &[])?;
        let mut cursor = reader.scan_selected(
            &symbol,
            &selection,
            ScanOptions {
                ..reader.scan_options()
            },
        )?;
        let setup_seconds = setup.elapsed().as_secs_f64();
        let ctx = Ctx { metadata };
        let mut batch_seconds = 0.;
        let mut sink_seconds = 0.;
        let mut count = 0u64;
        let mut digest = INITIAL;
        let mut batches = 0u64;
        let mut first_seconds = 0.;
        loop {
            let batch_start = Instant::now();
            let next = cursor.next_batch()?;
            batch_seconds += batch_start.elapsed().as_secs_f64();
            let Some(batch) = next else { break };
            if count == 0 {
                first_seconds = total.elapsed().as_secs_f64();
            }
            batches += 1;
            let sink_start = Instant::now();
            for at in 0..batch.len() {
                require(count < n, "extra record")?;
                consume(&ctx, count, &batch.row(at).ok_or("row")?, &mut digest)?;
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
        let vm = vm_peak()?;
        require(
            vm <= CAP && reader.budget_statistics().peak_bytes <= ROOT,
            "cap exceeded",
        )?;
        println!(
            "{{\"mode\":\"serial\",\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{seconds},\"open_seconds\":{open_seconds},\"setup_seconds\":{setup_seconds},\"first_seconds\":{first_seconds},\"batch_seconds\":{batch_seconds},\"sink_seconds\":{sink_seconds},\"close_seconds\":{close_seconds},\"batches\":{batches},\"decoded_rows\":{},\"returned_rows\":{},{}}}",
            work.decoded_rows,
            work.returned_rows,
            tail(&reader, vm)
        );
        Ok(())
    }
    struct ParallelArgs<'a> {
        source: &'a std::ffi::OsStr,
        index_path: &'a std::ffi::OsStr,
        metadata: bool,
        n: u64,
        stride: u64,
        workers: usize,
        query: &'a str,
        repeats: usize,
    }
    fn parallel(a: &ParallelArgs<'_>) -> R<()> {
        let (source, index_path, metadata, n, stride, workers, query, repeats) = (
            a.source,
            a.index_path,
            a.metadata,
            a.n,
            a.stride,
            a.workers,
            a.query,
            a.repeats,
        );
        check_cap()?;
        let reader = open_reader(source)?;
        let symbol = reader.symbol(SymbolId::new(if metadata { 0 } else { 1 })?)?;
        let start = Instant::now();
        let index = reader
            .open_index(
                &symbol,
                index_path,
                IndexOptions {
                    record_stride: stride,
                    maximum_index_bytes: 12 * 1024 * 1024 * 1024,
                    maximum_open_files: 2,
                    page_cache_bytes: 1024,
                    ..IndexOptions::default()
                },
                IndexPolicy::UseIfValid,
            )?
            .index
            .ok_or("strict index unavailable")?;
        let reopen_seconds = start.elapsed().as_secs_f64();
        let validation = index.validation_statistics();
        require(
            validation.complete && validation.rows_decoded == n,
            "incomplete strict source validation",
        )?;
        let limit = match query {
            "all" | "cancel" => None,
            "head" => Some(13),
            "zero" => Some(0),
            _ => return Err("query".into()),
        };
        let ctx = Ctx { metadata };
        let mut phases = Vec::with_capacity(3);
        let mut index = Some(index);
        for _ in 0..=repeats {
            let scan_start = Instant::now();
            let mut cursor = reader.scan_parallel(
                &symbol,
                index.take().ok_or("lost validated index owner")?,
                ScanOptions {
                    limit,
                    ..reader.scan_options()
                },
                ParallelOptions {
                    workers,
                    maximum_in_flight_tasks: workers,
                    intervals_per_task: 4,
                    ..ParallelOptions::default()
                },
            )?;
            let mut count = 0u64;
            let mut digest = INITIAL;
            let mut batch_seconds = 0.;
            let mut sink_seconds = 0.;
            let mut first_seconds = 0.;
            loop {
                let batch_start = Instant::now();
                let next = cursor.next_batch()?;
                batch_seconds += batch_start.elapsed().as_secs_f64();
                let Some(batch) = next else { break };
                if count == 0 {
                    first_seconds = scan_start.elapsed().as_secs_f64();
                }
                let sink_start = Instant::now();
                for at in 0..batch.len() {
                    require(count < n, "extra record")?;
                    consume(&ctx, count, &batch.row(at).ok_or("row")?, &mut digest)?;
                    count += 1;
                }
                sink_seconds += sink_start.elapsed().as_secs_f64();
                if query == "cancel" {
                    cursor.cancel()?;
                    break;
                }
            }
            let seconds = scan_start.elapsed().as_secs_f64();
            let stats = cursor.statistics();
            require(
                stats.returned_rows == count && stats.joined_workers == stats.admitted_workers,
                "count/join mismatch",
            )?;
            require(
                stats.complete == (query != "cancel") && stats.cancelled == (query == "cancel"),
                "completion/cancel mismatch",
            )?;
            let admitted = if query == "zero" { 0 } else { workers };
            require(
                stats.requested_workers == workers
                    && stats.admitted_workers == admitted
                    && stats.peak_ready_batches <= stats.admitted_workers
                    && stats.maximum_buffered_batches == stats.admitted_workers,
                "unexpected admission/queue bound",
            )?;
            let phase = format!(
                "{{\"query\":\"{query}\",\"workers\":{workers},\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{seconds},\"first_seconds\":{first_seconds},\"batch_seconds\":{batch_seconds},\"sink_seconds\":{sink_seconds},\"requested\":{},\"admitted\":{},\"joined\":{},\"stack_reserved\":{},\"control_reserved\":{},\"maximum_buffered_batches\":{},\"peak_ready_batches\":{},\"tasks_dispatched\":{},\"tasks_completed\":{},\"decoded_rows\":{},\"produced_rows\":{},\"returned_rows\":{},\"complete\":{},\"cancelled\":{}}}",
                stats.requested_workers,
                stats.admitted_workers,
                stats.joined_workers,
                stats.stack_reserved_bytes,
                stats.control_reserved_bytes,
                stats.maximum_buffered_batches,
                stats.peak_ready_batches,
                stats.tasks_dispatched,
                stats.tasks_completed,
                stats.decoded_rows,
                stats.produced_rows,
                stats.returned_rows,
                stats.complete,
                stats.cancelled
            );
            phases.push(phase);
            if query == "cancel" {
                require(
                    0 < count && count <= if metadata { 65536 } else { 1024 },
                    "unbounded cancelled prefix",
                )?;
                cursor.close()?;
                break;
            }
            index = Some(cursor.into_index()?);
        }
        let vm = vm_peak()?;
        require(
            vm <= CAP && reader.budget_statistics().peak_bytes <= ROOT,
            "cap exceeded",
        )?;
        println!(
            "{{\"mode\":\"parallel\",\"count\":{n},\"reopen_seconds\":{reopen_seconds},\"phases\":[{}],{}}}",
            phases.join(","),
            tail(&reader, vm)
        );
        Ok(())
    }
    fn value_word(digest: &mut u64, value: Value) {
        let text = format!("{value:?}");
        for byte in text.as_bytes() {
            *digest = step(*digest, u64::from(*byte));
        }
    }
    fn generic_scan(
        reader: &Reader,
        symbol: &SymbolHandle,
        mode: &str,
        index_path: &std::ffi::OsStr,
        index_options: IndexOptions,
        index: Option<CheckpointIndex>,
    ) -> R<(String, u64, u64, u64, CheckpointIndex)> {
        let start = Instant::now();
        let mut count = 0u64;
        let mut specials = 0u64;
        let mut digest = INITIAL;
        let mut index = index;
        match mode {
            "serial" => {
                let selection = reader.selection(symbol, &[])?;
                let mut cursor = reader.scan_selected(symbol, &selection, reader.scan_options())?;
                while let Some(batch) = cursor.next_batch()? {
                    for at in 0..batch.len() {
                        let row = batch.row(at).ok_or("row")?;
                        for d in 0..20 {
                            if let Some(key) = row.key(d) {
                                digest = step(digest, u64::from(key.get()));
                            }
                        }
                        for field in ValueField::ALL.iter() {
                            if let Some(value) = row.value(*field) {
                                if !matches!(value, Value::NumberBits(_)) {
                                    specials += 1;
                                }
                                value_word(&mut digest, value);
                            }
                        }
                        count += 1;
                    }
                }
            }
            "indexed" => {
                let selection = reader.selection(symbol, &[])?;
                let mut cursor = reader.scan_selected_indexed(
                    symbol,
                    &selection,
                    reader.scan_options(),
                    IndexOpen {
                        status: IndexStatus::Ready,
                        index: Some(index.take().ok_or("index")?),
                    },
                )?;
                while let Some(batch) = cursor.next_batch()? {
                    for at in 0..batch.len() {
                        let row = batch.row(at).ok_or("row")?;
                        for d in 0..20 {
                            if let Some(key) = row.key(d) {
                                digest = step(digest, u64::from(key.get()));
                            }
                        }
                        for field in ValueField::ALL.iter() {
                            if let Some(value) = row.value(*field) {
                                if !matches!(value, Value::NumberBits(_)) {
                                    specials += 1;
                                }
                                value_word(&mut digest, value);
                            }
                        }
                        count += 1;
                    }
                }
                index = cursor.into_index();
            }
            _ => {
                let mut cursor = reader.scan_parallel(
                    symbol,
                    index.take().ok_or("index")?,
                    reader.scan_options(),
                    ParallelOptions {
                        workers: 4,
                        maximum_in_flight_tasks: 4,
                        intervals_per_task: 1,
                        ..ParallelOptions::default()
                    },
                )?;
                while let Some(batch) = cursor.next_batch()? {
                    for at in 0..batch.len() {
                        let row = batch.row(at).ok_or("row")?;
                        for d in 0..20 {
                            if let Some(key) = row.key(d) {
                                digest = step(digest, u64::from(key.get()));
                            }
                        }
                        for field in ValueField::ALL.iter() {
                            if let Some(value) = row.value(*field) {
                                if !matches!(value, Value::NumberBits(_)) {
                                    specials += 1;
                                }
                                value_word(&mut digest, value);
                            }
                        }
                        count += 1;
                    }
                }
                index = Some(cursor.into_index()?);
            }
        }
        let _ = index_path;
        let _ = index_options;
        let record = format!(
            "{{\"mode\":\"{mode}\",\"count\":{count},\"specials\":{specials},\"digest\":\"{digest:016x}\",\"seconds\":{}}}",
            start.elapsed().as_secs_f64()
        );
        Ok((record, count, specials, digest, index.ok_or("index lost")?))
    }
    fn special(
        source: &std::ffi::OsStr,
        index_path: &std::ffi::OsStr,
        symbol_id: u32,
        n: u64,
    ) -> R<()> {
        check_cap()?;
        let reader = open_reader(source)?;
        let symbol = reader.symbol(SymbolId::new(symbol_id)?)?;
        let index_options = IndexOptions {
            record_stride: 1,
            maximum_index_bytes: 12 * 1024 * 1024 * 1024,
            maximum_open_files: 2,
            page_cache_bytes: 1024,
            ..IndexOptions::default()
        };
        let mut builder = reader.index_builder(&symbol, index_path, index_options)?;
        let built = builder.finish()?;
        require(built.complete && built.published, "special build")?;
        drop(builder);
        let index = reader
            .open_index(&symbol, index_path, index_options, IndexPolicy::UseIfValid)?
            .index
            .ok_or("strict index unavailable")?;
        let mut modes = Vec::new();
        let mut index = Some(index);
        let mut reference = None;
        for mode in ["serial", "indexed", "parallel"] {
            let (record, count, specials, digest, owner) = generic_scan(
                &reader,
                &symbol,
                mode,
                index_path,
                index_options,
                index.take(),
            )?;
            require(count == n, "special count")?;
            if let Some((_, _, first)) = reference.as_ref() {
                require(
                    digest == *first,
                    "special-value digest differs across modes",
                )?;
            } else {
                reference = Some((count, specials, digest));
            }
            require(specials > 0, "fixture must contain special values")?;
            modes.push(record);
            index = Some(owner);
        }
        let vm = vm_peak()?;
        require(
            vm <= CAP && reader.budget_statistics().peak_bytes <= ROOT,
            "cap exceeded",
        )?;
        println!(
            "{{\"mode\":\"special\",\"count\":{n},\"specials\":{},\"modes\":[{}],{}}}",
            reference.ok_or("reference")?.1,
            modes.join(","),
            tail(&reader, vm)
        );
        Ok(())
    }
    pub fn run() -> R<()> {
        let args: Vec<_> = std::env::args_os().collect();
        let mode = args.get(1).and_then(|a| a.to_str()).ok_or("mode")?;
        let source = args.get(2).ok_or("source")?;
        let kind_of = |i: usize| match args.get(i).and_then(|a| a.to_str()) {
            Some("records") => Ok(false),
            Some("metadata") => Ok(true),
            _ => Err("kind"),
        };
        if mode == "serial" {
            if args.len() != 5 {
                return Err("usage: serial SOURCE records|metadata COUNT".into());
            }
            let metadata = kind_of(3)?;
            let n: u64 = args[4].to_str().ok_or("count")?.parse()?;
            require(
                (1024..=60_000_000).contains(&n),
                "bounded acceptance recipe",
            )?;
            return serial(source, metadata, n);
        }
        if mode == "special" {
            if args.len() != 6 {
                return Err("usage: special SOURCE INDEX SYMBOL COUNT".into());
            }
            let symbol_id: u32 = args[4].to_str().ok_or("symbol")?.parse()?;
            let n: u64 = args[5].to_str().ok_or("count")?.parse()?;
            require((1..=1_000_000).contains(&n), "bounded special recipe")?;
            return special(
                args.get(2).ok_or("source")?,
                args.get(3).ok_or("index")?,
                symbol_id,
                n,
            );
        }
        if mode == "parallel" {
            if args.len() != 10 {
                return Err(
                    "usage: parallel SOURCE INDEX records|metadata COUNT STRIDE WORKERS all|head|zero|cancel REPEATS"
                        .into(),
                );
            }
            let index = args.get(3).ok_or("index")?;
            let metadata = kind_of(4)?;
            let n: u64 = args[5].to_str().ok_or("count")?.parse()?;
            let stride: u64 = args[6].to_str().ok_or("stride")?.parse()?;
            let workers: usize = args[7].to_str().ok_or("workers")?.parse()?;
            let query = args[8].to_str().ok_or("query")?;
            let repeats: usize = args[9].to_str().ok_or("repeats")?.parse()?;
            require(
                (1024..=60_000_000).contains(&n)
                    && (1..=65536).contains(&stride)
                    && (1..=8).contains(&workers)
                    && repeats <= 2,
                "bounded acceptance recipe",
            )?;
            return parallel(&ParallelArgs {
                source,
                index_path: index,
                metadata,
                n,
                stride,
                workers,
                query,
                repeats,
            });
        }
        Err("mode".into())
    }
}
