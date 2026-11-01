//! Development-only task-035 non-collecting Linux acceptance worker.
#[cfg(not(feature = "index"))]
fn main() {
    eprintln!("requires index feature");
    std::process::exit(1);
}
#[cfg(feature = "index")]
fn main() {
    if let Err(e) = worker::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(feature = "index")]
mod worker {
    use gdxrs::*;
    use std::{error::Error, path::Path, time::Instant};
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
    fn io(before: IoStatistics, after: IoStatistics) -> String {
        format!(
            "\"read_calls\":{},\"bytes_read\":{},\"decoded_bytes\":{},\"decoded_buffers\":{}",
            after.read_calls - before.read_calls,
            after.bytes_read - before.bytes_read,
            after.decoded_bytes - before.decoded_bytes,
            after.decoded_buffers - before.decoded_buffers
        )
    }
    fn index_stats(s: IndexStatistics, seconds: f64) -> String {
        format!(
            "{{\"seconds\":{seconds},\"source_bytes_hashed\":{},\"rows_decoded\":{},\"intervals\":{},\"directory_bytes\":{},\"index_bytes\":{},\"pages_read\":{},\"pages_written\":{},\"digest_seconds\":{},\"replay_seconds\":{},\"complete\":{},\"published\":{}}}",
            s.source_bytes_hashed,
            s.rows_decoded,
            s.intervals,
            s.directory_pages * 1024,
            s.index_bytes,
            s.pages_read,
            s.pages_written,
            s.digest_time.as_secs_f64(),
            s.replay_time.as_secs_f64(),
            s.complete,
            s.published
        )
    }
    fn label(n: u64) -> [u8; 63] {
        let mut b = [b'x'; 63];
        b[0] = b'u';
        let mut n = 1_000_000_000 + n;
        for i in (1..11).rev() {
            b[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        b
    }
    fn labels(reader: &Reader, n: u64) -> R<String> {
        let start = Instant::now();
        let before = reader.io_statistics();
        let mut c = reader.uels()?;
        let mut count = 0;
        let mut digest = INITIAL;
        while let Some(row) = c.next()? {
            count += 1;
            require(
                u64::from(row.id.get()) == count && row.label == label(count),
                "metadata ID/label recipe",
            )?;
            digest = step(digest, count);
            for b in row.label {
                digest = step(digest, u64::from(*b));
            }
        }
        require(count == n, "label count")?;
        Ok(format!(
            "{{\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{},{} }}",
            start.elapsed().as_secs_f64(),
            io(before, reader.io_statistics())
        ))
    }
    struct Recipe {
        n: u64,
        metadata: bool,
        query: String,
        first: u64,
        last: u64,
        limit: Option<u64>,
        axis: u8,
    }
    impl Recipe {
        fn new(n: u64, metadata: bool, query: &str) -> R<Self> {
            let (first, last, limit, axis) = match query {
                "all" => (1, n, None, 0),
                "head" => (1, n, Some(13), 0),
                "zero" => (1, n, Some(0), 0),
                "leading" if metadata => (n / 2, (n / 2 + 31).min(n), None, 0),
                "edge" if metadata => (n, n, Some(1), 0),
                "leading" => {
                    let k = (n / 2) / 1024 + 1;
                    (k, k + 1, None, 0)
                }
                "edge" => {
                    let k = (n - 1) / 1024 + 1;
                    (k, k, Some(13), 0)
                }
                "nonleading" if !metadata => (1, 3, None, 19),
                "miss" if !metadata => (2, 2, None, 1),
                "skew" if !metadata => (1, 1, None, 1),
                _ => return Err("query".into()),
            };
            Ok(Self {
                n,
                metadata,
                query: query.to_owned(),
                first,
                last,
                limit,
                axis,
            })
        }
        fn expected_count(&self) -> u64 {
            let raw = match self.query.as_str() {
                "zero" | "miss" => 0,
                "all" | "head" | "skew" => self.n,
                "nonleading" => self.n / 1024 * 3 + (self.n % 1024).min(3),
                _ if self.metadata => self.last - self.first + 1,
                _ => self
                    .n
                    .min(self.last * 1024)
                    .saturating_sub((self.first - 1) * 1024),
            };
            raw.min(self.limit.unwrap_or(u64::MAX))
        }
        fn expected_ordinal(&self, at: u64) -> u64 {
            match self.query.as_str() {
                "all" | "head" | "skew" => at,
                "nonleading" => at / 3 * 1024 + at % 3,
                _ if self.metadata => self.first - 1 + at,
                _ => (self.first - 1) * 1024 + at,
            }
        }
    }
    fn query(
        reader: &Reader,
        symbol: &SymbolHandle,
        recipe: &Recipe,
        open: Option<IndexOpen>,
    ) -> R<(String, Option<CheckpointIndex>)> {
        let start = Instant::now();
        let before = reader.io_statistics();
        let clauses = [DimensionSelection {
            dimension: recipe.axis,
            keys: KeySelection::Range(UelRange::new(
                UelId::new(u32::try_from(recipe.first)?)?,
                UelId::new(u32::try_from(recipe.last)?)?,
            )?),
        }];
        let selection = reader.selection(
            symbol,
            if ["all", "head", "zero"].contains(&recipe.query.as_str()) {
                &[]
            } else {
                &clauses
            },
        )?;
        let options = ScanOptions {
            limit: recipe.limit,
            ..reader.scan_options()
        };
        let mut cursor = match open {
            Some(open) => reader.scan_selected_indexed(symbol, &selection, options, open)?,
            None => reader.scan_selected(symbol, &selection, options)?,
        };
        let mut count = 0u64;
        let mut digest = INITIAL;
        let mut first_seconds = 0.;
        while let Some(batch) = cursor.next_batch()? {
            if count == 0 {
                first_seconds = start.elapsed().as_secs_f64();
            }
            for at in 0..batch.len() {
                let row = batch.row(at).ok_or("row")?;
                let n = recipe.expected_ordinal(count);
                require(n < recipe.n, "row outside recipe")?;
                if recipe.metadata {
                    let key = row.key(0).ok_or("universe key")?.get();
                    require(
                        u64::from(key) == n + 1
                            && row.value(ValueField::Level) == Some(Value::ZERO),
                        "universe identity",
                    )?;
                    digest = step(step(digest, u64::from(key)), 0);
                } else {
                    for d in 0..20 {
                        let expected = match d {
                            0 => n / 1024 + 1,
                            19 => n % 1024 + 1,
                            _ => 1,
                        };
                        let actual = u64::from(row.key(d).ok_or("key")?.get());
                        require(actual == expected, "record key/order recipe")?;
                        digest = step(digest, actual);
                    }
                    for (f, field) in ValueField::ALL.iter().enumerate() {
                        let b = 0x3ff0000000000000
                            | ((n * 17 + f as u64 * 12345 + 0x1640) & 0xfffffffffffff);
                        require(
                            row.value(*field) == Some(Value::NumberBits(b)),
                            "record value bits",
                        )?;
                        digest = step(digest, b);
                    }
                }
                count += 1;
            }
        }
        require(count == recipe.expected_count(), "query output count")?;
        let seconds = start.elapsed().as_secs_f64();
        let work = cursor.statistics();
        let stats = cursor.indexed_statistics();
        let index = cursor.into_index();
        let extra = if let Some(s) = stats {
            format!(
                "\"mode\":\"{:?}\",\"considered\":{},\"pruned\":{},\"coalesced\":{},\"restarts\":{},\"index_pages_read\":{},\"cache_hits\":{},\"validation_bytes_hashed\":{},\"validation_pages\":{}",
                s.mode,
                s.intervals_considered,
                s.intervals_pruned,
                s.adjacent_intervals_coalesced,
                s.checkpoint_restarts,
                s.index_pages_read,
                s.index_cache_hits,
                s.validation.source_bytes_hashed,
                s.validation.pages_read
            )
        } else {
            "\"mode\":\"Serial\"".to_owned()
        };
        Ok((
            format!(
                "{{\"query\":\"{}\",\"count\":{count},\"digest\":\"{digest:016x}\",\"seconds\":{seconds},\"first_seconds\":{first_seconds},\"decoded_rows\":{},\"returned_rows\":{}, {},{} }}",
                recipe.query,
                work.decoded_rows,
                work.returned_rows,
                extra,
                io(before, reader.io_statistics())
            ),
            index,
        ))
    }
    pub fn run() -> R<()> {
        let args: Vec<_> = std::env::args_os().collect();
        if args.len() != 9 {
            return Err("usage: build|open|session|serial|labels SOURCE INDEX records|metadata COUNT STRIDE QUERY REPEATS".into());
        }
        let mode = args[1].to_str().ok_or("mode")?;
        let metadata = match args[4].to_str() {
            Some("records") => false,
            Some("metadata") => true,
            _ => return Err("kind".into()),
        };
        let n: u64 = args[5].to_str().ok_or("count")?.parse()?;
        let stride: u64 = args[6].to_str().ok_or("stride")?.parse()?;
        let selector = args[7].to_str().ok_or("query")?;
        let repeats: usize = args[8].to_str().ok_or("repeats")?.parse()?;
        require(
            (1024..=60_000_000).contains(&n) && (1..=65536).contains(&stride) && repeats <= 3,
            "bounded development recipe",
        )?;
        let limits = std::fs::read_to_string("/proc/self/limits")?;
        let f: Vec<_> = limits
            .lines()
            .find(|s| s.starts_with("Max address space"))
            .ok_or("limit")?
            .split_whitespace()
            .collect();
        require(
            f.get(3).ok_or("soft")?.parse::<u64>()? == CAP
                && f.get(4).ok_or("hard")?.parse::<u64>()? == CAP,
            "OS cap not enforced",
        )?;
        let reader = Reader::open(
            &args[2],
            OpenOptions {
                memory_budget_bytes: ROOT,
                metadata_cache_bytes: 0,
                maximum_batch_bytes: 1024 * 1024,
            },
        )?;
        let symbol = reader.symbol(SymbolId::new(if metadata { 0 } else { 1 })?)?;
        let options = IndexOptions {
            record_stride: stride,
            decoded_byte_stride: if stride == 1 { u64::MAX } else { 1024 * 1024 },
            maximum_index_bytes: 12 * 1024 * 1024 * 1024,
            maximum_open_files: 2,
            page_cache_bytes: 1024,
            ..IndexOptions::default()
        };
        let mut phases = Vec::with_capacity(13); // Fixed <=13 bounded diagnostic objects, never rows.
        if mode == "build" {
            let time = Instant::now();
            let mut owner = reader.index_builder(&symbol, Path::new(&args[3]), options)?;
            let stats = owner.finish()?;
            require(stats.complete && stats.published, "publication incomplete")?;
            phases.push(index_stats(stats, time.elapsed().as_secs_f64()));
        } else if mode == "labels" {
            phases.push(labels(&reader, n)?);
        } else if mode == "serial" || mode == "serial-matrix" {
            if mode == "serial-matrix" {
                let names: &[&str] = if metadata {
                    &["all", "leading", "head", "zero", "edge"]
                } else if selector == "directory" {
                    &["all", "leading", "edge", "zero"]
                } else {
                    &["all", "nonleading", "miss", "head", "zero", "edge", "skew"]
                };
                for name in names {
                    phases.push(query(&reader, &symbol, &Recipe::new(n, metadata, name)?, None)?.0);
                }
            } else {
                phases.push(query(&reader, &symbol, &Recipe::new(n, metadata, selector)?, None)?.0);
            }
        } else if mode == "open" || mode == "session" || mode == "matrix" {
            let time = Instant::now();
            let open = reader.open_index(
                &symbol,
                Path::new(&args[3]),
                options,
                IndexPolicy::UseIfValid,
            )?;
            let mut index = open.index.ok_or("index unavailable")?;
            phases.push(index_stats(
                index.validation_statistics(),
                time.elapsed().as_secs_f64(),
            ));
            if mode == "open" {
                for ordinal in [0, n / 2, n - 1] {
                    let number = if stride == 1 {
                        ordinal
                    } else {
                        index.interval_count().saturating_sub(1)
                    };
                    let interval = index.interval(number)?;
                    let mut scan = reader.scan_from_checkpoint(
                        &symbol,
                        &interval.checkpoint,
                        ScanOptions {
                            limit: Some(1),
                            ..reader.scan_options()
                        },
                    )?;
                    let row = scan.next_batch()?.ok_or("suffix")?;
                    let key = row
                        .row(0)
                        .ok_or("suffix row")?
                        .key(0)
                        .ok_or("suffix key")?
                        .get();
                    let expected = if metadata {
                        interval.start + 1
                    } else {
                        interval.start / 1024 + 1
                    };
                    require(u64::from(key) == expected, "suffix ordinal identity")?;
                }
            } else {
                let suite = mode == "session" && selector.starts_with("suite");
                let names: &[&str] = if suite && metadata {
                    &["leading", "all", "head", "zero", "edge"]
                } else if suite && selector == "suite-directory" {
                    &["leading", "all", "edge", "zero"]
                } else if suite {
                    &[
                        "leading",
                        "all",
                        "nonleading",
                        "miss",
                        "head",
                        "zero",
                        "edge",
                        "skew",
                    ]
                } else if mode != "matrix" {
                    &[selector]
                } else if metadata {
                    &["all", "leading", "head", "zero", "edge"]
                } else if selector == "directory" {
                    &["all", "leading", "edge", "zero"]
                } else {
                    &["all", "nonleading", "miss", "head", "zero", "edge", "skew"]
                };
                for name in names {
                    let recipe = Recipe::new(n, metadata, name)?;
                    for _ in 0..=if mode == "matrix" || suite && *name != "leading" {
                        0
                    } else {
                        repeats
                    } {
                        let (snapshot, owner) = query(
                            &reader,
                            &symbol,
                            &recipe,
                            Some(IndexOpen {
                                status: IndexStatus::Ready,
                                index: Some(index),
                            }),
                        )?;
                        index = owner.ok_or("lost warm owner")?;
                        require(
                            index.validation_statistics().source_bytes_hashed
                                == std::fs::metadata(&args[2])?.len(),
                            "validation identity work",
                        )?;
                        phases.push(snapshot);
                    }
                }
            }
        } else {
            return Err("mode".into());
        }
        let b = reader.budget_statistics();
        require(b.peak_bytes <= ROOT, "root excess")?;
        println!(
            "{{\"root_limit\":{ROOT},\"root_peak\":{},\"root_current\":{},\"os_cap_verified\":true,\"index_quota\":{},\"handle_limit\":2,\"cache_limit\":1024,\"phases\":[{}]}}",
            b.peak_bytes,
            b.current_bytes,
            options.maximum_index_bytes,
            phases.join(",")
        );
        Ok(())
    }
}
