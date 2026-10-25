//! Development validating sink. Does not collect GDX output.
use gdxrs::*;
use std::{error::Error, path::Path, time::Instant};
type R<T> = std::result::Result<T, Box<dyn Error>>;
const INITIAL: u64 = 14695981039346656037;
fn step(h: u64, v: u64) -> u64 {
    (h.rotate_left(7) ^ v).wrapping_mul(0x9e3779b185ebca87)
}
fn label(n: u64) -> [u8; 63] {
    let mut out = [b'x'; 63];
    out[0] = b'u';
    let mut value = 1_000_000_000 + n;
    for i in (1..11).rev() {
        out[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out
}
fn require(ok: bool, message: &str) -> R<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn spill(path: &Path) -> SpillOptions {
    let mut o = SpillOptions::new(path);
    o.run_bytes = 8 * 1024 * 1024;
    o.maximum_temporary_bytes = 10 * 1024 * 1024 * 1024;
    o.maximum_open_files = 27;
    o.merge_fan_in = 8;
    o
}
fn statistics(r: &Reader) -> String {
    let b = r.budget_statistics();
    let io = r.io_statistics();
    format!(
        "\"root_peak\":{},\"root_current\":{},\"read_calls\":{},\"bytes_read\":{},\"decoded_bytes\":{},\"decoded_buffers\":{}",
        b.peak_bytes,
        b.current_bytes,
        io.read_calls,
        io.bytes_read,
        io.decoded_bytes,
        io.decoded_buffers
    )
}
fn spill_stats(s: SpillStatistics) -> String {
    format!(
        "{{\"peak_temp\":{},\"temp\":{},\"peak_files\":{},\"files\":{},\"passes\":{},\"runs\":{},\"read\":{},\"written\":{},\"input\":{},\"complete\":{}}}",
        s.peak_temporary_bytes,
        s.temporary_bytes,
        s.peak_open_files,
        s.open_files,
        s.merge_passes,
        s.published_runs,
        s.bytes_read,
        s.bytes_written,
        s.input_records,
        s.complete
    )
}
fn scan(r: &Reader, mode: &str) -> R<()> {
    let start = Instant::now();
    let symbol = r.symbol(SymbolId::new(1)?)?;
    let range = UelRange::new(
        UelId::new(if mode == "leading" { 2 } else { 1 })?,
        UelId::new(3)?,
    )?;
    let clauses = [DimensionSelection {
        dimension: if mode == "leading" { 0 } else { 19 },
        keys: KeySelection::Range(range),
    }];
    let selection = r.selection(
        &symbol,
        if mode == "leading" || mode == "nonleading" {
            &clauses
        } else {
            &[]
        },
    )?;
    let mut options = r.scan_options();
    if mode == "head" {
        options.limit = Some(13);
    }
    if mode == "zero" {
        options.limit = Some(0);
    }
    let mut cursor = r.scan_selected(&symbol, &selection, options)?;
    let mut count = 0;
    let mut digest = INITIAL;
    let mut previous = None;
    let mut first_seconds = 0.;
    let mut first_bytes = 0;
    while let Some(batch) = cursor.next_batch()? {
        if count == 0 {
            first_seconds = start.elapsed().as_secs_f64();
            first_bytes = r.io_statistics().bytes_read;
        }
        for i in 0..batch.len() {
            let row = batch.row(i).ok_or("missing row")?;
            let first = u64::from(row.key(0).ok_or("missing first key")?.get());
            let last = u64::from(row.key(19).ok_or("missing last key")?.get());
            let n = (first - 1) * 1024 + last - 1;
            require(previous.is_none_or(|p| n > p), "record order")?;
            previous = Some(n);
            if mode == "leading" {
                require((2..=3).contains(&first), "leading predicate")?;
            }
            if mode == "nonleading" {
                require(last <= 3, "nonleading predicate")?;
            }
            for d in 0..20 {
                let expected = match d {
                    0 => n / 1024 + 1,
                    19 => n % 1024 + 1,
                    _ => 1,
                };
                let actual = u64::from(row.key(d).ok_or("missing projected key")?.get());
                require(actual == expected, "key differs from recipe")?;
                digest = step(digest, actual);
            }
            for (f, field) in ValueField::ALL.iter().enumerate() {
                let expected =
                    0x3ff0000000000000 | ((n * 17 + f as u64 * 12345 + 0x1640) & 0xfffffffffffff);
                require(
                    row.value(*field) == Some(Value::NumberBits(expected)),
                    "value differs from recipe",
                )?;
                digest = step(digest, expected);
            }
            count += 1;
        }
    }
    let work = cursor.statistics();
    drop(cursor);
    drop(selection);
    drop(symbol);
    println!(
        "{{\"count\":{count},\"digest\":\"{digest:016x}\",\"decoded_rows\":{},\"first_seconds\":{first_seconds},\"first_bytes\":{first_bytes},\"seconds\":{},{} }}",
        work.decoded_rows,
        start.elapsed().as_secs_f64(),
        statistics(r)
    );
    Ok(())
}
fn metadata(r: &Reader, labels: u64) -> R<()> {
    let start = Instant::now();
    let mut cursor = r.uels()?;
    let mut n = 0;
    let mut digest = INITIAL;
    while let Some(entry) = cursor.next()? {
        n += 1;
        require(
            u64::from(entry.id.get()) == n && entry.label == label(n),
            "UEL recipe",
        )?;
        digest = step(digest, n);
        for &b in entry.label {
            digest = step(digest, u64::from(b));
        }
    }
    require(n == labels, "UEL count")?;
    drop(cursor);
    println!(
        "{{\"count\":{n},\"digest\":\"{digest:016x}\",\"seconds\":{},{} }}",
        start.elapsed().as_secs_f64(),
        statistics(r)
    );
    Ok(())
}
fn lookup(r: &Reader, labels: u64, parent: &Path, membership: bool) -> R<()> {
    let start = Instant::now();
    let options = spill(parent);
    let selected = labels / 4;
    let mut builder = r.large_label_lookup(&options)?;
    for k in 0..selected {
        let n = selected - k;
        builder.push(&label(n))?;
        if k % 16 == 15 {
            builder.push(&label(n))?;
        }
    }
    builder.push(&label(labels + 1))?;
    let built = Instant::now();
    let mut cursor = builder.finish()?;
    let prepare_seconds = start.elapsed().as_secs_f64();
    if membership {
        let symbol = r.symbol(SymbolId::new(0)?)?;
        let base = r.selection(&symbol, &[])?;
        let mut selection = r.selection_from_large_labels(&symbol, 0, &base, cursor, &options)?;
        let mut scan = r.scan_selected(
            &symbol,
            &selection,
            ScanOptions {
                limit: Some(13),
                ..r.scan_options()
            },
        )?;
        let mut n = 0;
        while let Some(batch) = scan.next_batch()? {
            for id in batch.key_column(0).ok_or("missing universe keys")? {
                n += 1;
                require(u64::from(id.get()) == n, "membership ID")?;
            }
        }
        require(n == 13, "membership limit")?;
        let work = scan.statistics();
        drop(scan);
        let stats = selection.spill_statistics().ok_or("missing disk stats")?;
        selection.close_spill()?;
        drop(selection);
        drop(base);
        drop(symbol);
        println!(
            "{{\"count\":{n},\"decoded_rows\":{},\"prepare_seconds\":{prepare_seconds},\"seconds\":{},\"spill\":{},{} }}",
            work.decoded_rows,
            start.elapsed().as_secs_f64(),
            spill_stats(stats),
            statistics(r)
        );
    } else {
        let mut count = 0;
        let mut digest = INITIAL;
        let mut expected_id = 1;
        let mut duplicate = false;
        while let Some(entry) = cursor.next()? {
            let (id, ordinal) = if expected_id <= selected {
                let k = selected - expected_id;
                let ord = k + k / 16 + u64::from(duplicate);
                let id = expected_id;
                if k % 16 == 15 && !duplicate {
                    duplicate = true;
                } else {
                    duplicate = false;
                    expected_id += 1;
                }
                (id, ord)
            } else {
                (0, selected + selected / 16)
            };
            require(
                entry.ordinal == ordinal && entry.id.map_or(0, |v| u64::from(v.get())) == id,
                "lookup ID/ordinal recipe",
            )?;
            require(
                entry.label == label(if id == 0 { labels + 1 } else { id }),
                "lookup label recipe",
            )?;
            digest = step(step(digest, ordinal), id);
            for &b in entry.label {
                digest = step(digest, u64::from(b));
            }
            count += 1;
        }
        require(
            count == selected + selected / 16 + 1 && cursor.is_complete(),
            "lookup completion/count",
        )?;
        let (requests, uels) = cursor.statistics();
        let uels = uels.ok_or("missing UEL sort")?;
        drop(cursor);
        println!(
            "{{\"count\":{count},\"digest\":\"{digest:016x}\",\"push_seconds\":{},\"prepare_seconds\":{prepare_seconds},\"seconds\":{},\"requests\":{},\"uels\":{},{} }}",
            built.duration_since(start).as_secs_f64(),
            start.elapsed().as_secs_f64(),
            spill_stats(requests),
            spill_stats(uels),
            statistics(r)
        );
    }
    Ok(())
}
fn pages(r: &Reader, labels: u64) -> R<()> {
    let mut c = r.uel_labels()?;
    for n in [1, labels / 2, labels, 1, labels] {
        require(c.get(UelId::new(n as u32)?)? == label(n), "page ID label")?;
    }
    let stats = c.statistics();
    drop(c);
    println!(
        "{{\"count\":5,\"entries_decoded\":{},{} }}",
        stats.uel_entries_decoded,
        statistics(r)
    );
    Ok(())
}
fn domains(r: &Reader, rows: u64, parent: &Path) -> R<()> {
    let start = Instant::now();
    let s = r.symbol(SymbolId::new(1)?)?;
    let selection = r.selection(&s, &[])?;
    let limit = rows.min(5_000_000);
    let mut c = r.unique_domains(
        &s,
        &selection,
        &[19, 0],
        ScanOptions {
            limit: Some(limit),
            ..r.scan_options()
        },
        &spill(parent),
    )?;
    let mut count = 0;
    while let Some(e) = c.next()? {
        let (dimension, id) = if count < limit.div_ceil(1024) {
            (0, count + 1)
        } else {
            (19, count - limit.div_ceil(1024) + 1)
        };
        require(
            e.dimension == dimension && u64::from(e.id.get()) == id,
            "domain recipe",
        )?;
        count += 1;
    }
    require(
        count == limit.div_ceil(1024) + limit.min(1024),
        "domain count",
    )?;
    let stats = c.statistics();
    require(stats.complete, "domain completion")?;
    drop(c);
    drop(selection);
    drop(s);
    println!(
        "{{\"count\":{count},\"accepted_rows\":{limit},\"seconds\":{},\"spill\":{},{} }}",
        start.elapsed().as_secs_f64(),
        spill_stats(stats),
        statistics(r)
    );
    Ok(())
}
fn execute() -> R<()> {
    let args: Vec<_> = std::env::args().collect();
    require(args.len() == 5, "usage: MODE INPUT COUNT SCRATCH")?;
    // Linux-only acceptance executable. The standard core retains other platforms.
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let address = limits
        .lines()
        .find(|s| s.starts_with("Max address space"))
        .ok_or("missing OS address-space cap")?;
    require(
        address
            .split_whitespace()
            .filter(|v| *v == "536870912")
            .count()
            == 2,
        "512 MiB hard/soft OS cap not active",
    )?;
    let start = Instant::now();
    let r = Reader::open(
        &args[2],
        OpenOptions {
            memory_budget_bytes: 256 * 1024 * 1024,
            metadata_cache_bytes: 128 * 1024,
            ..OpenOptions::default()
        },
    )?;
    let baseline = r.budget_statistics().current_bytes;
    let n = args[3].parse()?;
    let parent = Path::new(&args[4]);
    match args[1].as_str() {
        "all" | "leading" | "nonleading" | "head" | "zero" => scan(&r, &args[1])?,
        "metadata" => metadata(&r, n)?,
        "lookup" => lookup(&r, n, parent, false)?,
        "membership" => lookup(&r, n, parent, true)?,
        "pages" => pages(&r, n)?,
        "domains" => domains(&r, n, parent)?,
        "cancel" => {
            let mut b = r.large_label_lookup(&spill(parent))?;
            for i in 1..100_000 {
                b.push(&label(i))?;
            }
            b.cancel()?;
            drop(b);
            println!("{{\"cancelled\":true,{} }}", statistics(&r));
        }
        _ => return Err("unknown mode".into()),
    }
    require(
        r.budget_statistics().current_bytes == baseline,
        "retained library working set after consumer cleanup",
    )?;
    eprintln!("complete wall_seconds={}", start.elapsed().as_secs_f64());
    Ok(())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
