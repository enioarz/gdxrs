# gdxrs

An independent Rust reader for modern GDX v7 files, designed around bounded memory and streaming reads. M0 and M1 are merged. M2 provides exact raw-ID selections, small exact-label lookup, bounded label pages, quota-controlled scratch sorting, large requests and unique-domain enumeration. Record-heavy and independently metadata-heavy scale qualification has passed on Linux x86_64. M3 provides bounded metadata and sorted record writing. M4 adds unsorted, string, mapped and deferred-domain input with bounded error reporting. Indexes, parallel decoding and Polars integration remain planned.

```rust,no_run
use gdxrs::{OpenOptions, Reader};

fn main() -> gdxrs::Result<()> {
    let reader = Reader::open("model.gdx", OpenOptions::default())?;
    if let Some(symbol) = reader.find_symbol(b"demand")? {
        let mut options = reader.scan_options();
        options.limit = Some(100);
        let mut scan = reader.scan(&symbol, options)?;
        while let Some(batch) = scan.next_batch()? {
            // Consume borrowed columns here; the next call reuses the buffers.
            println!("{} rows", batch.len());
        }
    }
    Ok(())
}
```

Keys retain one-based file UEL IDs. Typed values preserve ordinary numeric bits, the five GDX special classes, and acronym identity. Metadata is streamed through lending views; cold lookups may scan tables without building a full dictionary. Independent cursors share the reader's configurable memory quota, including codec scratch and output buffers. Sources must remain immutable.

Compression is enabled by default using one direct dependency (`miniz_oxide`) and one transitive dependency (`adler2`). Disabling default features removes both and rejects compressed input/output explicitly. Production code uses safe Rust and no native GDX/GAMS/Python runtime.

```sh
cargo run --locked --release --example dump -- FILE.gdx
python3 tools/dev.py reader-check
```

The dump example streams normalized semantic TSV to stdout; its 64 KiB consumer-side output buffer is separate from the reader budget. It propagates read/write/flush failures. See [development](docs/development.md), [reader contract](docs/reader-contract.md), [resource accounting](docs/resource-contract.md), [M1 evidence](docs/evidence/011.md), and [implementation plan](IMPLEMENTATION_PLAN.md).

Supported profile: modern canonical v7 layout, uniform little/big endian, stored/zlib blocks, all symbol types and dimensions 0–20. Legacy v5/v6, mixed primitive byte orders and old producer heuristics are excluded. Native reader tests pass on Linux x86_64, macOS ARM64/Intel and Windows x64/MSVC; see [platform evidence](docs/evidence/017.md). Bounded library accounting is separate from the enforced OS address-space cap used in [M2 qualification](docs/evidence/016.md). Fixed standard shared-owner allocations retain Rust's allocator-OOM abort behavior; see [the recorded constraint](docs/decisions/0003-m1-advancement.md).

For exact ID slicing, compile source/symbol-bound `DimensionSelection` clauses with `reader.selection(&symbol, &clauses)`, then use `reader.scan_selected(&symbol, &selection, options)`. Clauses are conjoined across zero-based dimensions; IDs or inclusive ranges are unioned within a dimension. Filtering precedes projection and the accepted-row limit, preserving original IDs/order. Explicit empty sets avoid record reads. Selection payloads share the reader quota; oversized in-memory requests currently return `BudgetTooSmall`. `cursor.statistics()` reports decoded versus returned public rows; a selective result does not imply disk pruning. See [the selection contract](docs/decisions/0006-m2-selection.md) and [M2 task queue](tasks/README.md).

For labels, call `reader.lookup_labels(&[b"region-a", b"region-b"])` and pass the resulting `LabelMatches` to `reader.selection_from_labels(&symbol, dimension, &matches)`. Matching uses exact stored bytes: duplicates keep separate request ordinals and unknowns have `None`; all-unknown selections return no data. Requests exceeding the 128 KiB small-lookup payload allowance return `BudgetTooSmall` in the small path; use the streaming large-lookup API for bigger requests. Create a cursor with `reader.uel_labels()` and borrow ID-to-label bytes with `cursor.get(id)`. It works using at most two optional pages per cursor, sharing `metadata_cache_bytes` across cursors. Zero/tiny caches remain correct through bounded scans; cold random lookups may rescan prefixes. See [lookup/cache contracts](docs/decisions/0007-label-lookup-pages.md).

`reader.spill_sort(&SpillOptions::new(scratch_parent))` creates a private scratch sort for `(label bytes, opaque u64 ID)` records. Push records, call `finish()`, then stream borrowed entries with `next()`. Exact byte order and duplicates are preserved. Run buffers share the reader's memory quota; per-job temporary logical-byte and open-run-handle limits bound merging. `statistics().complete` becomes true only after validated EOF and cleanup. Explicit `cancel`/`close` report cleanup failures; `Drop` is best effort. Scratch storage must support same-directory hard links. These temporary sorting primitives also support large lookup and observed-domain extraction; they are not a GDX writer. See [spill ownership and format](docs/decisions/0008-spill-runs.md).


For large requests, create `reader.large_label_lookup(&limits)`, call `push(label)`
and `finish()`, then lend results through `next()`. Results are sorted by exact
label bytes and carry original request ordinals; unknowns have `None`. To filter,
pass a fresh lookup cursor and an existing base selection to
`reader.selection_from_large_labels(&symbol, dimension, &base, lookup, &limits)`.
The disk membership intersects the base filters before projection/limits.
`reader.unique_domains(&symbol, &selection, &dimensions, scan_options, &limits)`
externally sorts/deduplicates observed `(original dimension, original UEL ID)` pairs
from the accepted limited rows. Both preparation operations are blocking.

Large lookup divides temporary-byte/handle limits into three shares; at least nine
total handles are required and fan-in is reduced to fit each share. All concurrent
memory competes for the reader root budget. Close lookup/domain cursors explicitly
to report cleanup failures; call `selection.close_spill()` after dropping its scans
and clones. Drop cleanup is best effort. See [large lookup/domain ownership and
limitations](docs/decisions/0009-large-labels-domains.md). See [task 016 evidence](docs/evidence/016.md) for OS-capped independently
oversized metadata qualification and measured throughput limitations.


M2 qualification streamed **20 million 20-dimensional records** (2.52 GB input,
2.40 GB logical key/value output) and a separate **36-million-label** dictionary
(2.30 GB), each larger than four times a 512 MiB hard process address-space cap.
The reader used a 256 MiB root budget; observed tracked peak stayed below 9 MB and
process high-water RSS below 14 MB, including fork/startup overhead. It also
resolved 9.56 million label requests with spill and validated all results.
Reproduce with `python3 tools/dev.py m2-scale-check --profile scale`.

A safe scalar-buffer fast path reduced the wide synthetic full-read median from
6.83 s to **3.95 s**, versus **4.60 s** for a fresh pinned-native comparison, with
identical full field/digest validation. This closes the measured gap on that
workload; cross-workload throughput parity remains unqualified. Slices still
decode the full source stream. See [throughput follow-up](docs/evidence/016-throughput.md)
and [qualification/cache/platform limits](docs/evidence/016.md). These are measured
results, not portable speed guarantees. Writing, indexes, parallel decoding and
Polars integration remain outside M2.


The [native CI matrix](.github/workflows/reader-platforms.yml) runs on every pull
request and main push. Each target executes clean-checkout Cargo tests, optimized
default-feature tests, all feature combinations and the complete frozen expected
record stream. Unicode paths, concurrent positioned reads and spill handle cleanup
are included. The 512 MiB OS-cap and throughput measurements above remain
Linux-only; macOS/Windows qualification currently covers functionality and logical
resource accounting. Cargo/core use requires no Python or native GDX installation.


M3 supports bounded metadata and sorted raw record output:

```rust,no_run
use gdxrs::{Writer, WriteOptions};
# fn example() -> gdxrs::Result<()> {
let mut writer = Writer::create("new.gdx", WriteOptions::default())?;
writer.finish()?;
# Ok(()) }
```

It creates little-endian v7 files with registered metadata and sorted symbols. Set
`WriteOptions::compression` to `WriteCompression::Zlib` for independent 32 KiB
level-1 zlib blocks with stored fallback; the default is uncompressed.
Output is buffered under a root budget, finalized/synced/closed and published without
replacing an existing file. `abort()` reports retryable temporary cleanup failures;
Drop never publishes. Inspect `statistics().published` if finish fails after
publication. Use `reader.create_writer(path, reader.write_options())` to share an
existing reader root budget; the writer does not retain its input file.
See [writer ownership and resource contract](docs/decisions/0012-writer-contract.md)
and [bounded encoder allocation audit](docs/decisions/0013-bounded-writer-codec.md).


Start registration with `writer.begin_metadata(&MetadataOptions::new(scratch_parent))`.
`register_uel` preserves exact native-compatible bytes and returns a writer-bound
ID; duplicate labels keep their first ID. Incremental duplicate lookup scans disk.
For large fresh dictionaries, `register_uels_fresh` consumes a fallible iterator
of borrowed bytes or bounded owned byte arrays and streams `(one-based request
ordinal, writer-bound ID)` through a fallible callback. It detects duplicates by
external sorting and preserves first-registration order. Producers/consumers must
budget their own retained memory; the writer retains no full dictionary.

`register_symbol(&WriteSymbol::new(name, kind, dimension))` returns a writer-bound
symbol ID. Set descriptions/alias targets in the spec, then stream `add_comment`,
`add_set_text`, `add_acronym` and `set_domains` calls. Strict domains accept an
existing writer-bound symbol or a forward name; relaxed domains preserve names.
`freeze_metadata()` resolves/validates references and prevents further mutation;
`finish()` also freezes automatically. Aliases and empty scalar defaults follow
native semantics. Sorted raw records are available through `begin_symbol` below.

Metadata uses checksummed disk journals and explicit aggregate scratch-byte/handle
limits. Disk scans for incremental names/references can be quadratic; the bulk
UEL path is intended for large dictionaries. Logical quota accounting is separate
from OS RSS. See [metadata ownership, phases and quotas](docs/decisions/0014-writer-metadata.md).


### Sorted raw records

Register all metadata first. `begin_symbol(id, bounds)` freezes registration and
borrows the output transaction; `write(keys, values)` accepts writer-owned UEL
IDs and typed `Value`s. Keys must be strictly increasing. Supplied `KeyBounds`
are inclusive; `None` uses conservative four-byte widths without a prescan.
Explicitly finish each symbol, then finish the file:

```rust,no_run
# use gdxrs::*;
# fn example() -> gdxrs::Result<()> {
let mut writer = Writer::create("records.gdx", WriteOptions::default())?;
writer.begin_metadata(&MetadataOptions::new("scratch"))?;
let key = writer.register_uel(b"a")?;
let symbol = writer.register_symbol(&WriteSymbol::new(b"P", SymbolType::Parameter, 1))?;
let mut records = writer.begin_symbol(symbol, None)?;
records.write(&[key], &[Value::NumberBits(2.5f64.to_bits())])?;
records.finish()?;
writer.finish()?;
# Ok(()) }
```

Complete strict domain sets before writing dependent symbols. Alias IDs remain
in domain metadata; membership checks follow the underlying set with a bounded
disk index. Untouched symbols remain empty (scalars expose native defaults).
Duplicate/out-of-order rows, invalid references, failed rows and abandoned symbol
writers make publication fail. Set values are registered set-text IDs. The
[record contract](docs/decisions/0015-sorted-record-writer.md) describes storage,
ordering, bounds and value semantics. Full metadata/finalization and Linux
OS-capped pipeline acceptance are qualified in
[tasks 022–023](docs/evidence/023.md). See the [M3 API inventory](docs/m3-api.md).

The acceptance run writes 52 million rows (2.39 GB raw output), streams a reader
into the writer under one shared 256 MiB root, and independently writes 34 million
63-byte labels (2.17 GB output), all under an enforced 512 MiB Linux process cap.
Native GDX and Rust validate complete ordered digests and original IDs. Native
large-metadata reference validation uses a separate 16 GiB cap because native GDX
owns that dictionary eagerly; Rust writers and validators retain the 512 MiB cap.

Repeated representative raw encoding meets native throughput on the measured
Linux host. Bulk registration remains about five times slower than eager native
registration; incremental metadata scans may be quadratic. Compression timings
compare level-1 Rust to native default level 6, with file sizes recorded separately.
Native Linux/macOS/Windows functionality passes; OS/RSS/performance qualification
is Linux-only. Full results, source revisions and limits are in the acceptance
evidence. General unsorted/string/mapped record adapters and append remain M4.

### Unsorted raw records (M4 task 024)

`writer.begin_unsorted_symbol(id, bounds)` accepts registered writer-owned UEL
IDs in arbitrary order. Its borrowing `write`/`finish` flow mirrors the sorted
writer; finish returns the record count and `SpillStatistics`. It uses the run
memory, scratch directory, byte quota and handle settings explicitly configured
by `begin_metadata`. Journal and sort shares remain within that aggregate quota.

The implementation spills complete bounded records and merges them through the
sorted encoder. IDs and typed value bits are preserved. Duplicate raw keys are
errors even when values differ; invalid or abandoned work prevents publication.
Strict domain sets must already be complete. Use `statistics()` during ingestion
or after an ingestion error; parent abort retries owned cleanup after failure.
See the [unsorted contract](docs/decisions/0018-unsorted-raw-records.md) and
[task queue](tasks/README.md). String/mapped record adapters and append remain
pending; this first task does not complete M4's larger-than-RAM acceptance gate.


### String records

Register symbol metadata first, then call `begin_string_records()`. Its
`begin_symbol(id)` starts a contiguous input group and closes the previous one;
`write(&[&[u8]], &[Value])` accepts arbitrary-order rows without retaining label
slices or token arrays. Finish the string phase, then finish the writer to publish.
Existing UEL IDs come first; new exact-byte labels receive IDs in first-encounter
order across all groups. Domain sets must precede dependent symbols.

This is a two-pass disk dictionary/remap and record replay, using the same root
and aggregate scratch quota as metadata and unsorted raw writes. Configure at
least seven scratch handles. Labels are case-sensitive exact bytes, including
non-UTF-8; numeric bits retain the core's policy. These deliberately preserve more
identity than native string authoring's case folding / negative-zero normalization.
The phase supports at most i32::MAX label occurrences plus existing UELs.
Duplicates, invalid references, failed rows and abandonment are terminal, including
failures discovered during replay. See [decision 0019](docs/decisions/0019-string-records.md).
General-input OS-capped scale acceptance remains task 031.


### Mapped caller IDs

`MappedUelId` accepts caller IDs from zero through i32::MAX; raw `UelId`s remain
one-based and on-disk IDs never change. `Writer::register_mapped_uels` builds one
sparse bijection while preserving first-registration raw IDs. Use
`begin_mapped_symbol`, write caller keys and typed values, then finish the symbol
and writer. Duplicate assignments are idempotent; ambiguous collisions fail.

For reads, create `Reader::mapped_uels(options)`, register explicit raw/caller
pairs, then use `Reader::scan_mapped`. Each borrowed row exposes caller keys,
original raw keys and typed values. This explicit mapped-order operation prescans
and sorts the symbol, then streams fixed rows; positive limits apply after sorting.
Missing entries fail by default; `SkipRecord` explicitly filters them and reports
counts. Raw scans retain their streaming behavior. Close the cursor and map to
report cleanup errors; abort can retry removal.

Maps and row sorters share one root and the existing scratch halves; configure
seven available handles for the index plus merge work. IDs near i32::MAX do not
allocate dense arrays. Mapped throughput/OS-scale qualification remains task 031.
See [mapped contract](docs/decisions/0020-mapped-ids.md) and
[API inventory](docs/mapped-api.md) for limits and native semantic differences.


Task 027 adds `Writer::begin_deferred_records` to stage registered raw, mapped
or string keys before their domain sets are written. `finish()` rejects invalid
rows terminally; `finish_reporting(ErrorLimits, callback)` explicitly recovers
domain/duplicate rows through a bounded fallible callback. First valid input
wins among duplicate keys. `Reader::scan_mapped_reporting` reports missing-map
rows with original raw IDs. See the [domain/error API inventory](docs/domain-error-api.md)
and [ownership, quotas and partial-result policy](docs/decisions/0021-domain-error-streams.md).
[Task 027 evidence](docs/evidence/027.md) records local and four-platform native checks.
General-input scale/performance acceptance remains task 031.


Task 028 adds explicit [value/acronym policies](docs/value-acronym-api.md). Writer
`write_f64` adapters use per-Writer bitwise sentinels and optionally normalize
IEEE values; typed writes bypass the policy. Reader `ValueSession` retains original
typed values alongside explicit numeric projections, with quota-controlled
acronym mappings/next IDs and resets. Floating-point identity loss requires
`FloatConversion::AllowLoss`. Acronym metadata can be updated before freeze through
a bounded journal rewrite. See [ownership, quotas and exclusions](docs/decisions/0022-value-acronym-policies.md).
