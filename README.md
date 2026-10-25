# gdxrs

An independent Rust reader for modern GDX v7 files, designed around bounded memory and streaming reads. M0 and M1 are merged. M2 is in progress: exact raw-ID selections are implemented; label lookup, spill and OS-capped scale qualification remain pending. Writing, indexes, parallel decoding and Polars integration follow later.

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

Supported profile: modern canonical v7 layout, uniform little/big endian, stored/zlib blocks, all symbol types and dimensions 0–20. Legacy v5/v6, mixed primitive byte orders and old producer heuristics are excluded. Tests currently run on Linux x86_64. Bounded library accounting is not an RSS limit; OS-capped larger-than-RAM verification remains M2. Fixed standard shared-owner allocations retain Rust's allocator-OOM abort behavior; see [the recorded constraint](docs/decisions/0003-m1-advancement.md).

For exact ID slicing, compile source/symbol-bound `DimensionSelection` clauses with `reader.selection(&symbol, &clauses)`, then use `reader.scan_selected(&symbol, &selection, options)`. Clauses are conjoined across zero-based dimensions; IDs or inclusive ranges are unioned within a dimension. Filtering precedes projection and the accepted-row limit, preserving original IDs/order. Explicit empty sets avoid record reads. Selection payloads share the reader quota; oversized in-memory requests currently return `BudgetTooSmall`. `cursor.statistics()` reports decoded versus returned public rows; a selective result does not imply disk pruning. See [the selection contract](docs/decisions/0006-m2-selection.md) and [M2 task queue](tasks/README.md).
