# Persistent exact selections (`index` feature)

M5 provides opaque record checkpoints, explicit source-verified persistent
sidecars and exact indexed selections. See [acceptance scope and measurements](evidence/035.md)
and [ownership/trust/resource contract](decisions/0030-m5-acceptance.md). PR #30 merged at
`efbd808`; qualification does not automatically publish an M5 release.

Enable `index` in Cargo features. It adds the [audited optional SHA-256 graph](decisions/0028-index-digest-audit.md).
Default compression dependencies are unchanged; no features retains zero runtime
dependencies. Production Rust does not require GDX/GAMS, Python, C++, Arrow or Polars.

```rust,no_run
use gdxrs::{DimensionSelection, IndexOpen, IndexOptions, IndexPolicy,
            IndexStatus, KeySelection, OpenOptions, Reader, UelId};

fn example() -> gdxrs::Result<()> {
    let reader = Reader::open("model.gdx", OpenOptions::default())?;
    if let Some(symbol) = reader.find_symbol(b"demand")? {
        let selection = reader.selection(&symbol, &[DimensionSelection {
            dimension: 0, keys: KeySelection::One(UelId::new(42)?),
        }])?;
        // Explicit open: valid caches are fully source-verified here.
        let open = reader.open_index(&symbol, "demand.gdxrs-index",
                                     IndexOptions::default(), IndexPolicy::UseIfValid)?;
        let mut query = reader.scan_selected_indexed(
            &symbol, &selection, reader.scan_options(), open)?;
        while let Some(batch) = query.next_batch()? {
            println!("{} matching rows", batch.len());
        }
        let work = query.indexed_statistics();
        let rejection = query.index_rejection();
        println!("{work:?}; cache rejection: {rejection:?}");
        if let Some(index) = query.into_index() {
            let open = IndexOpen { status: IndexStatus::Ready, index: Some(index) };
            let next = reader.scan_selected_indexed(
                &symbol, &selection, reader.scan_options(), open)?;
            // Consume next; its constructor does not repeat strict validation.
            drop(next);
        }
    }
    Ok(())
}
```

The snippet requires `index`. `Off` does no sidecar work. `UseIfValid` never builds:
missing/rejected caches fall back serially before output. Source/access/resource
errors remain errors. Strict open hashes the entire source and regenerates the
symbol's checkpoints/summaries, rejecting forged state even if unkeyed hashes are
recomputed. Hashing/replay costs are explicit and separately measured.

Build only when requested with `reader.index_builder(&symbol, path, options)`;
retain the owner and call `finish`. CreateNew protects existing destinations.
Replace is explicit and requires exclusive pathname ownership. Keep a failed owner
for fallible `abort`/cleanup retries, and handle primary plus secondary diagnostics.
Directory/data extent, cache, handles and all controls share the specified quotas.
No scan automatically creates, validates or rebuilds a cache.

IDs remain original one-based raw UEL IDs. Bounds prune only proven misses; exact
residuals precede projections and globally accepted limits. Leading-key bounds use
ordering proved during strict replay; non-leading axes remain conservative.
Adjacent spans share a decoder, with bounded restart after gaps. No candidates,
dictionaries or directory roots are collected in RAM. Scalar/universe/alias and
special-value semantics remain those of the serial reader.

`into_index` consumes/cancels the cursor, releases its batches/decoder, and recovers
the session. It belongs to the same immutable open source/requested symbol, not a
new Reader over identical bytes. A cache-invalidated session stays invalid. Before
any batch escapes, integrity failure may discard an unpublished fill and fall back;
after output, failure is sticky and acknowledged rows are never silently replayed.

`ScanStatistics` distinguishes actual decoded work from acknowledged returned rows.
`indexed_statistics` reports validation separately from query pages/cache hits,
inspected/pruned intervals, binary probes, bulk skips, coalescing, restarts and
source I/O. Bulk-pruned intervals can exceed summaries inspected. Source counters
can include simultaneous same-Reader I/O; work between calls is excluded.

The measured acceptance uses a 256 MiB root / enforced 512 MiB Linux process cap
with independently oversized records, metadata and sidecar directories. Cold cache
control is advisory; first query follows strict replay; warm queries reuse a session.
Build/validation break-even is workload-dependent. Native macOS/Windows functionality
is tested, without generalizing Linux RSS/timings. No general adapter throughput
parity, parallel execution, postings, competing-writer CAS or crash recovery is claimed.
