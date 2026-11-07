//! Linux-only development sink for task-038 worker/stack containment diagnostics.
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
    fn require(ok: bool, message: &str) -> R<()> {
        if ok { Ok(()) } else { Err(message.into()) }
    }
    pub fn run() -> R<()> {
        let args: Vec<_> = std::env::args_os().collect();
        require(
            args.len() == 7,
            "usage: parallel_worker SOURCE INDEX COUNT STRIDE WORKERS all|head|zero|cancel",
        )?;
        let n: u64 = args[3].to_str().ok_or("count")?.parse()?;
        let stride: u64 = args[4].to_str().ok_or("stride")?.parse()?;
        let workers: usize = args[5].to_str().ok_or("workers")?.parse()?;
        let query = args[6].to_str().ok_or("query")?;
        require(
            (16384..=1_000_000).contains(&n)
                && (1..=65536).contains(&stride)
                && (workers == 2 || workers == 4),
            "bounded diagnostic recipe",
        )?;
        let limit = match query {
            "all" | "cancel" => None,
            "head" => Some(13),
            "zero" => Some(0),
            _ => return Err("query".into()),
        };
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
        let reader = Reader::open(
            &args[1],
            OpenOptions {
                memory_budget_bytes: ROOT,
                metadata_cache_bytes: 0,
                maximum_batch_bytes: 1024 * 1024,
            },
        )?;
        let symbol = reader.symbol(SymbolId::new(1)?)?;
        let start = Instant::now();
        let index = reader
            .open_index(
                &symbol,
                &args[2],
                IndexOptions {
                    record_stride: stride,
                    maximum_index_bytes: 12 * 1024 * 1024 * 1024,
                    ..IndexOptions::default()
                },
                IndexPolicy::UseIfValid,
            )?
            .index
            .ok_or("strict index unavailable")?;
        let validation = index.validation_statistics();
        require(
            validation.complete && validation.rows_decoded == n,
            "incomplete strict source validation",
        )?;
        let reopen_seconds = start.elapsed().as_secs_f64();
        let before = reader.io_statistics();
        let start = Instant::now();
        let mut cursor = reader.scan_parallel(
            &symbol,
            index,
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
        let mut digest = 0xcbf29ce484222325u64;
        while let Some(batch) = cursor.next_batch()? {
            for at in 0..batch.len() {
                let row = batch.row(at).ok_or("row")?;
                for d in 0..20 {
                    let expected = match d {
                        0 => count / 1024 + 1,
                        19 => count % 1024 + 1,
                        _ => 1,
                    };
                    let actual = u64::from(row.key(d).ok_or("key")?.get());
                    require(actual == expected, "key/order mismatch")?;
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
            if query == "cancel" {
                cursor.cancel()?;
                break;
            }
        }
        cursor.close()?;
        let s = cursor.statistics();
        let expected = match query {
            "all" => n,
            "head" => 13,
            "zero" => 0,
            "cancel" => count,
            _ => return Err("query".into()),
        };
        require(
            count == expected && s.returned_rows == count && s.joined_workers == s.admitted_workers,
            "count/join mismatch",
        )?;
        require(
            s.complete == (query != "cancel") && s.cancelled == (query == "cancel"),
            "completion/cancel mismatch",
        )?;
        require(
            s.admitted_workers == if query == "zero" { 0 } else { workers },
            "unexpected admission",
        )?;
        require(
            s.peak_ready_batches <= s.admitted_workers,
            "output slot bound exceeded",
        )?;
        let seconds = start.elapsed().as_secs_f64();
        let budget = reader.budget_statistics();
        let io = reader.io_statistics();
        let status = std::fs::read_to_string("/proc/self/status")?;
        let vm_peak_kib: u64 = status
            .lines()
            .find(|l| l.starts_with("VmPeak:"))
            .ok_or("VmPeak")?
            .split_whitespace()
            .nth(1)
            .ok_or("VmPeak number")?
            .parse()?;
        require(
            vm_peak_kib * 1024 <= CAP && budget.peak_bytes <= ROOT,
            "observed VM/root cap exceeded",
        )?;
        println!(
            "{{\"query\":\"{query}\",\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{seconds},\"reopen_seconds\":{reopen_seconds},\"root_limit\":{ROOT},\"root_peak\":{},\"os_cap_verified\":true,\"vm_peak_bytes\":{},\"requested\":{},\"admitted\":{},\"joined\":{},\"stack_reserved\":{},\"control_reserved\":{},\"maximum_buffered_batches\":{},\"peak_ready_batches\":{},\"tasks_dispatched\":{},\"tasks_completed\":{},\"decoded_rows\":{},\"produced_rows\":{},\"returned_rows\":{},\"complete\":{},\"cancelled\":{},\"query_bytes_read\":{},\"query_decoded_buffers\":{}}}",
            budget.peak_bytes,
            vm_peak_kib * 1024,
            s.requested_workers,
            s.admitted_workers,
            s.joined_workers,
            s.stack_reserved_bytes,
            s.control_reserved_bytes,
            s.maximum_buffered_batches,
            s.peak_ready_batches,
            s.tasks_dispatched,
            s.tasks_completed,
            s.decoded_rows,
            s.produced_rows,
            s.returned_rows,
            s.complete,
            s.cancelled,
            io.bytes_read - before.bytes_read,
            io.decoded_buffers - before.decoded_buffers
        );
        Ok(())
    }
}
