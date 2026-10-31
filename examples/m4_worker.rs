//! Development-only task-031 generators/sinks. Fixed caller state, no collected rows.
use gdxrs::*;
use std::{error::Error, path::Path, time::Instant};
type R<T> = std::result::Result<T, Box<dyn Error>>;
fn invalid(message: &'static str) -> gdxrs::Error {
    std::io::Error::other(message).into()
}
struct Label {
    bytes: [u8; 63],
    len: usize,
}
impl AsRef<[u8]> for Label {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
const ROOT: usize = 256 * 1024 * 1024;
const INITIAL: u64 = 14695981039346656037;
const SEED: u64 = 0x1640;
const LABELS: u64 = 65536;
fn require(ok: bool, message: &'static str) -> R<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn step(h: u64, n: u64) -> u64 {
    (h.rotate_left(7) ^ n).wrapping_mul(0x9e3779b185ebca87)
}
fn label(n: u64) -> [u8; 63] {
    let mut out = [b'x'; 63];
    out[0] = b'u';
    let mut n = 1_000_000_000 + n;
    for at in (1..11).rev() {
        out[at] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out
}
fn bits(n: u64, f: usize) -> u64 {
    0x3ff0000000000000 | ((n * 17 + f as u64 * 12345 + SEED) & ((1 << 52) - 1))
}
fn caller(n: u64) -> Result<MappedUelId> {
    MappedUelId::new(if n == 1 {
        0
    } else {
        i64::from(i32::MAX) - 31 * (n as i64 - 2)
    })
}
fn keys(n: u64, dim: usize) -> [u32; 20] {
    let mut k = [1; 20];
    k[0] = (n / 65536 + 1) as u32;
    k[1] = (n % 65536 + 1) as u32;
    for (d, key) in k.iter_mut().enumerate().take(dim).skip(2) {
        *key = ((n * (2 * d as u64 + 1) + SEED) & 65535) as u32 + 1;
    }
    k
}
fn label_count(kind: &str, n: u64) -> u64 {
    if kind == "dictionary" {
        n
    } else {
        LABELS + u64::from(kind == "errors")
    }
}
fn dimension(kind: &str) -> usize {
    if kind == "strings" {
        2
    } else if kind == "dictionary" {
        1
    } else {
        20
    }
}
fn require_cap() -> R<()> {
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let line = limits
        .lines()
        .find(|s| s.starts_with("Max address space"))
        .ok_or("missing AS limit")?;
    require(
        line.split_whitespace()
            .filter(|s| *s == "536870912")
            .count()
            == 2,
        "512 MiB hard/soft RLIMIT_AS missing",
    )
}
fn read_options() -> OpenOptions {
    OpenOptions {
        memory_budget_bytes: ROOT,
        ..OpenOptions::default()
    }
}
fn check_labels(r: &Reader, kind: &str, n: u64) -> R<u64> {
    let wanted = label_count(kind, n);
    require(u64::from(r.information()?.uel_count) == wanted, "UEL count")?;
    let mut c = r.uels()?;
    let mut seen = 0;
    let mut digest = INITIAL;
    while let Some(e) = c.next()? {
        seen += 1;
        let text = label(seen);
        let len = if kind == "strings" { 11 } else { 63 };
        require(
            e.id.get() as u64 == seen && e.label == &text[..len],
            "UEL bytes/original ID",
        )?;
        digest = step(digest, seen);
        for b in e.label {
            digest = step(digest, u64::from(*b));
        }
    }
    require(seen == wanted, "UEL EOF count")?;
    Ok(digest)
}
fn validate(path: &Path, kind: &str, n: u64) -> R<()> {
    let r = Reader::open(path, read_options())?;
    let labels = check_labels(&r, kind, n)?;
    let expected_symbols = if kind == "errors" || kind == "append" {
        2
    } else {
        1
    };
    require(
        r.information()?.symbol_count == expected_symbols,
        "symbol count",
    )?;
    if kind == "errors" {
        let h = r.symbol(SymbolId::new(1)?)?;
        let m = h.metadata()?;
        require(
            m.name == b"I"
                && m.kind == SymbolType::Set
                && m.dimension == 1
                && m.physical_record_count == LABELS,
            "domain set metadata",
        )?;
        let mut scan = r.scan(&h, r.scan_options())?;
        let mut seen = 0u64;
        while let Some(b) = scan.next_batch()? {
            for at in 0..b.len() {
                seen += 1;
                let row = b.row(at).ok_or("set row")?;
                require(
                    row.key(0).ok_or("set key")?.get() as u64 == seen
                        && row.value(ValueField::Level) == Some(Value::ZERO),
                    "domain set records",
                )?;
            }
        }
        require(seen == LABELS, "domain set EOF")?;
    }
    let id = if kind == "errors" { 2 } else { 1 };
    let h = r.symbol(SymbolId::new(id)?)?;
    let metadata = h.metadata()?;
    let dim = dimension(kind);
    let rows = if kind == "dictionary" { 5 } else { n };
    require(
        metadata.name == b"data"
            && metadata.dimension as usize == dim
            && metadata.kind
                == if kind == "dictionary" {
                    SymbolType::Parameter
                } else {
                    SymbolType::Variable
                }
            && metadata.user_information == if kind == "dictionary" { 0 } else { 5 }
            && metadata.error_count == 0
            && !metadata.has_set_text
            && metadata.physical_record_count == rows,
        "data metadata",
    )?;
    let mut domains = r.domains(&h)?;
    for axis in 0..dim {
        let d = domains.next()?.ok_or("domain axis")?;
        require(
            d.strict_symbol.map(|s| s.get())
                == if kind == "errors" {
                    Some(if axis == 0 { 1 } else { 0 })
                } else {
                    None
                },
            "strict domain identity",
        )?;
    }
    require(domains.next()?.is_none(), "domain EOF")?;
    drop(domains);
    let mut scan = r.scan(&h, r.scan_options())?;
    let mut count = 0;
    let mut digest = INITIAL;
    let sparse = [1, 2, n / 2, n - 1, n];
    while let Some(batch) = scan.next_batch()? {
        for at in 0..batch.len() {
            let row = batch.row(at).ok_or("row")?;
            let want = if kind == "dictionary" {
                sparse[count as usize]
            } else {
                count
            };
            let raw = if kind == "dictionary" {
                let mut k = [1; 20];
                k[0] = want as u32;
                k
            } else {
                keys(want, dim)
            };
            for (d, key) in raw.iter().enumerate().take(dim) {
                let actual = row.key(d as u8).ok_or("key")?.get();
                require(actual == *key, "ordered raw keys")?;
                digest = step(digest, u64::from(actual));
            }
            for f in 0..if kind == "dictionary" { 1 } else { 5 } {
                let value = row.value(ValueField::ALL[f]).ok_or("value")?;
                require(
                    value == Value::NumberBits(bits(want, f)),
                    "exact value bits",
                )?;
                digest = step(digest, bits(want, f));
            }
            count += 1;
        }
    }
    require(count == rows, "record EOF/count")?;
    if kind == "append" {
        let h = r.symbol(SymbolId::new(2)?)?;
        let m = h.metadata()?;
        require(
            m.name == b"added"
                && m.dimension == 0
                && m.kind == SymbolType::Parameter
                && m.physical_record_count == 1,
            "append symbol ID/schema",
        )?;
        let mut s = r.scan(&h, r.scan_options())?;
        let b = s.next_batch()?.ok_or("append scalar")?;
        require(
            b.len() == 1
                && b.row(0).ok_or("scalar")?.value(ValueField::Level)
                    == Some(Value::NumberBits(42f64.to_bits())),
            "append scalar value",
        )?;
        require(s.next_batch()?.is_none(), "append scalar EOF")?;
    }
    println!(
        "{{\"count\":{count},\"digest\":\"{digest:016x}\",\"label_digest\":\"{labels:016x}\",\"root_peak\":{},\"root_limit\":{ROOT},\"os_cap_verified\":true}}",
        r.budget_statistics().peak_bytes
    );
    Ok(())
}
fn run() -> R<()> {
    let args: Vec<_> = std::env::args_os().collect();
    require(
        args.len() == 7 || args.len() == 8,
        "usage: write|validate KIND OUT SCRATCH COUNT raw|zlib [INPUT]",
    )?;
    require_cap()?;
    let action = args[1].to_str().ok_or("action")?;
    let kind = args[2].to_str().ok_or("kind")?;
    require(
        [
            "unsorted",
            "strings",
            "mapped",
            "dictionary",
            "errors",
            "append",
        ]
        .contains(&kind),
        "kind",
    )?;
    let n: u64 = args[5].to_str().ok_or("count")?.parse()?;
    require((8..=60_000_000).contains(&n), "development count")?;
    let path = Path::new(&args[3]);
    if action == "validate" {
        return validate(path, kind, n);
    }
    require(action == "write", "action")?;
    let compression = match args[6].to_str() {
        Some("raw") => WriteCompression::None,
        Some("zlib") => WriteCompression::Zlib,
        _ => return Err("codec".into()),
    };
    let options = WriteOptions {
        memory_budget_bytes: ROOT,
        maximum_output_bytes: 8 * 1024 * 1024 * 1024,
        compression,
    };
    let mut scratch = MetadataOptions::new(&args[4]);
    scratch.run_bytes = if n < 1_000_000 {
        256 * 1024
    } else {
        32 * 1024 * 1024
    };
    scratch.lookup_cache_bytes = 64 * 1024 * 1024;
    scratch.maximum_temporary_bytes = 32 * 1024 * 1024 * 1024;
    scratch.maximum_open_files = 12;
    scratch.merge_fan_in = 8;
    let mut w = if kind == "append" {
        let mut a = AppendOptions::new(AppendPolicy::CreateNew);
        a.maximum_transaction_bytes = 40 * 1024 * 1024 * 1024;
        a.maximum_open_files = 14;
        Writer::open_append(
            args.get(7).ok_or("append source")?,
            path,
            read_options(),
            options,
            a,
        )?
    } else {
        Writer::create(path, options)?
    };
    let mut registration_seconds = 0.;
    let mut remapping_seconds = 0.;
    let mut ingestion_seconds = 0.;
    let mut merge_seconds = 0.;
    let mut replay_seconds = 0.;
    let mut copy_seconds = 0.;
    let mut errors = 0u64;
    let mut error_digest = INITIAL;
    let mut error_bytes = 0;
    let mut input_records = n;
    // Fixed caller token table only for the bounded 65537-label record recipes.
    // Dictionary and string families never retain tokens per label/record.
    let mut ids = [None; 65538];
    let labels = label_count(kind, n);
    let registration = Instant::now();
    if kind == "append" {
        w.begin_append(&scratch)?;
        copy_seconds = registration.elapsed().as_secs_f64();
        require(
            w.append_statistics()
                .ok_or("append statistics")?
                .rewritten_records
                == n,
            "append inherited records",
        )?;
        let id = w.register_symbol(&WriteSymbol::new(b"added", SymbolType::Parameter, 0))?;
        let mut rows = w.begin_symbol(id, None)?;
        rows.write(&[], &[Value::NumberBits(42f64.to_bits())])?;
        rows.finish()?;
    } else {
        w.begin_metadata(&scratch)?;
        if kind == "mapped" || kind == "dictionary" {
            w.register_mapped_uels((1..=labels).map(|i| Ok((caller(i)?, label(i)))))?;
        } else {
            w.register_uels_fresh(
                (1..=labels).map(|i| {
                    let b = label(i);
                    Ok(Label {
                        bytes: b,
                        len: if kind == "strings" { 11 } else { 63 },
                    })
                }),
                |ordinal, id| {
                    if kind != "strings" {
                        ids[ordinal as usize - 1] = Some(id);
                    }
                    require(ordinal == u64::from(id.get()), "registration order")
                        .map_err(|_| invalid("registration order"))
                },
            )?;
        }
        registration_seconds = registration.elapsed().as_secs_f64();
        let domain = if kind == "errors" {
            Some(w.register_symbol(&WriteSymbol::new(b"I", SymbolType::Set, 1))?)
        } else {
            None
        };
        let data = w.register_symbol(&WriteSymbol::new(
            b"data",
            if kind == "dictionary" {
                SymbolType::Parameter
            } else {
                SymbolType::Variable
            },
            dimension(kind) as u8,
        ))?;
        if let Some(i) = domain {
            let mut domains = [WriteDomain::Strict(SymbolReference::Universe); 20];
            domains[0] = WriteDomain::Strict(SymbolReference::Registered(i));
            w.set_domains(data, &domains)?;
        }
        let ingest = Instant::now();
        if kind == "dictionary" {
            let selected = [1, 2, n / 2, n - 1, n];
            let mut rows = w.begin_mapped_symbol(data)?;
            for raw in selected.into_iter().rev() {
                rows.write(&[caller(raw)?], &[Value::NumberBits(bits(raw, 0))])?;
            }
            ingestion_seconds = ingest.elapsed().as_secs_f64();
            let stats = rows.finish()?;
            merge_seconds = stats.merge_preparation_time.as_secs_f64();
            replay_seconds = stats.record_replay_time.as_secs_f64();
            input_records = 5;
        } else if kind == "strings" {
            let mut rows = w.begin_string_records()?;
            rows.begin_symbol(data)?;
            for i in (0..n).rev() {
                let k = keys(i, 2);
                let a = label(u64::from(k[0]));
                let b = label(u64::from(k[1]));
                let values = std::array::from_fn::<_, 5, _>(|f| Value::NumberBits(bits(i, f)));
                rows.write(&[&a[..11], &b[..11]], &values)?;
            }
            ingestion_seconds = ingest.elapsed().as_secs_f64();
            let stats = rows.finish()?;
            remapping_seconds = (stats.registration_time + stats.record_staging_time).as_secs_f64();
            merge_seconds = stats.merge_preparation_time.as_secs_f64();
            replay_seconds = stats.record_replay_time.as_secs_f64();
        } else if kind == "mapped" {
            let mut rows = w.begin_mapped_symbol(data)?;
            for i in (0..n).rev() {
                let k = keys(i, 20);
                let mut mapped = [caller(1)?; 20];
                for d in 0..20 {
                    mapped[d] = caller(u64::from(k[d]))?;
                }
                let values = std::array::from_fn::<_, 5, _>(|f| Value::NumberBits(bits(i, f)));
                rows.write(&mapped, &values)?;
            }
            ingestion_seconds = ingest.elapsed().as_secs_f64();
            let stats = rows.finish()?;
            merge_seconds = stats.merge_preparation_time.as_secs_f64();
            replay_seconds = stats.record_replay_time.as_secs_f64();
        } else if kind == "unsorted" {
            let mut rows = w.begin_unsorted_symbol(data, None)?;
            for i in (0..n).rev() {
                let k = keys(i, 20);
                let mut raw = [ids[0].ok_or("first token")?; 20];
                for d in 0..20 {
                    raw[d] = ids[k[d] as usize - 1].ok_or("token")?;
                }
                let values = std::array::from_fn::<_, 5, _>(|f| Value::NumberBits(bits(i, f)));
                rows.write(&raw, &values)?;
            }
            ingestion_seconds = ingest.elapsed().as_secs_f64();
            let stats = rows.finish()?;
            merge_seconds = stats.merge_preparation_time.as_secs_f64();
            replay_seconds = stats.record_replay_time.as_secs_f64();
        } else {
            let mut rows = w.begin_deferred_records()?;
            rows.begin_symbol(data)?;
            for i in (0..n).rev() {
                let k = keys(i, 20);
                let mut raw = [ids[0].ok_or("first token")?; 20];
                for d in 0..20 {
                    raw[d] = ids[k[d] as usize - 1].ok_or("token")?;
                }
                let values = std::array::from_fn::<_, 5, _>(|f| Value::NumberBits(bits(i, f)));
                rows.write(&raw, &values)?;
                if i % 2 == 0 {
                    let mut bad = raw;
                    bad[0] = ids[LABELS as usize].ok_or("bad token")?;
                    rows.write(&bad, &values)?;
                }
                if i % 3 == 0 {
                    rows.write(&raw, &values)?;
                }
            }
            rows.begin_symbol(domain.ok_or("domain")?)?;
            for id in ids.iter().take(LABELS as usize) {
                rows.write(&[id.ok_or("domain token")?], &[Value::ZERO])?;
            }
            ingestion_seconds = ingest.elapsed().as_secs_f64();
            let expected = n.div_ceil(2) + n.div_ceil(3);
            let stats = rows.finish_reporting(
                ErrorLimits {
                    maximum_records: expected,
                    maximum_bytes: 8 * 1024 * 1024 * 1024,
                },
                |error| {
                    require(
                        error.symbol.get() == 2
                            && error.origin == DataOrigin::RawWrite
                            && error.raw_keys.len() == 20
                            && error.values.len() == 5,
                        "diagnostic shape",
                    )
                    .map_err(|_| invalid("diagnostic shape"))?;
                    let Value::NumberBits(first) = error.values[0] else {
                        return Err(invalid("diagnostic value"));
                    };
                    let mantissa = first & ((1 << 52) - 1);
                    let i = mantissa
                        .checked_sub(SEED)
                        .filter(|v| v % 17 == 0)
                        .ok_or_else(|| invalid("diagnostic row identity"))?
                        / 17;
                    let (tag, mask) = match error.kind {
                        DataErrorKind::DomainViolation { dimensions: 1 } => (1, 1),
                        DataErrorKind::Duplicate => (2, 0),
                        _ => {
                            return Err(invalid("diagnostic kind"));
                        }
                    };
                    let prior = n - 1 - i;
                    let base = prior
                        + (n.div_ceil(2) - (i + 1).div_ceil(2))
                        + (n.div_ceil(3) - (i + 1).div_ceil(3))
                        + 1;
                    let ordinal = base
                        + if tag == 1 {
                            1
                        } else {
                            1 + u64::from(i % 2 == 0)
                        };
                    let mut expected_keys = keys(i, 20);
                    if tag == 1 {
                        expected_keys[0] = (LABELS + 1) as u32;
                    }
                    let valid = i < n
                        && error.input_ordinal == ordinal
                        && u64::from(error.symbol_ordinal) == ordinal
                        && (tag != 1 || i % 2 == 0)
                        && (tag != 2 || i % 3 == 0);
                    if !valid {
                        return Err(invalid("diagnostic ordinal"));
                    }
                    error_digest = step(step(step(error_digest, tag), ordinal), mask);
                    for (key, wanted) in error.raw_keys.iter().zip(expected_keys) {
                        if key.get() != wanted {
                            return Err(invalid("diagnostic raw ID"));
                        }
                        error_digest = step(error_digest, u64::from(key.get()));
                    }
                    for (f, value) in error.values.iter().enumerate() {
                        if *value != Value::NumberBits(bits(i, f)) {
                            return Err(invalid("diagnostic bits"));
                        }
                        error_digest = step(error_digest, bits(i, f));
                    }
                    errors += 1;
                    error_bytes += error.logical_bytes()?;
                    Ok(())
                },
            )?;
            require(
                stats.errors.reported == expected
                    && errors == expected
                    && stats.accepted_records == n + LABELS,
                "accepted/rejected counts",
            )?;
            input_records = stats.input_records;
            merge_seconds = stats.merge_preparation_time.as_secs_f64();
            replay_seconds = stats.record_replay_time.as_secs_f64();
        }
    }
    let finalize = Instant::now();
    let stats = w.finish()?;
    let finalization_seconds = finalize.elapsed().as_secs_f64();
    let root = w.budget_statistics();
    let scratch = w.metadata_statistics();
    require(
        stats.published
            && root.limit_bytes == ROOT
            && root.peak_bytes <= ROOT
            && scratch.temporary_bytes == 0
            && scratch.open_files == 0,
        "completion ownership/root",
    )?;
    println!(
        "{{\"count\":{},\"labels\":{labels},\"input_records\":{input_records},\"errors\":{errors},\"error_bytes\":{error_bytes},\"error_digest\":\"{error_digest:016x}\",\"registration_seconds\":{registration_seconds},\"remapping_seconds\":{remapping_seconds},\"ingestion_seconds\":{ingestion_seconds},\"merge_seconds\":{merge_seconds},\"replay_encoding_seconds\":{replay_seconds},\"copy_seconds\":{copy_seconds},\"finalization_seconds\":{finalization_seconds},\"root_peak\":{},\"root_limit\":{ROOT},\"scratch_peak\":{},\"scratch_peak_handles\":{},\"output_bytes\":{},\"os_cap_verified\":true,\"shared_root\":{}}}",
        if kind == "dictionary" { 5 } else { n },
        root.peak_bytes,
        scratch.peak_temporary_bytes,
        scratch.peak_open_files,
        stats.output_bytes,
        kind == "append"
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
