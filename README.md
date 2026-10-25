# gdxrs

An independent Rust reader for modern GDX v7 files, designed around bounded memory and streaming reads. M0 and M1 are merged. M2 provides exact raw-ID selections, small exact-label lookup, bounded label pages, quota-controlled scratch sorting, large requests and unique-domain enumeration. Record-heavy and independently metadata-heavy scale qualification has passed on Linux x86_64. Writing, indexes, parallel decoding and Polars integration follow later.

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

Compression is enabled by default using one direct dependency (`miniz_oxide`) and one transitive dependency (`adler2`). Disabling default features removes both and rejects compressed input explicitly. Production code uses safe Rust and no native GDX/GAMS/Python runtime.

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
