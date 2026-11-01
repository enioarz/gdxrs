# Serial ordinal ranges

Task 037 adds `Reader::scan_range` and `Reader::scan_selected_range` as the serial
foundation for future parallel reads. See [decision 0032](decisions/0032-ordinal-range-cursors.md)
and [task evidence](evidence/037.md). No worker, automatic planner or new dependency
is introduced; the APIs also work without `index` or compression features.

`scan_range(symbol, checkpoint, end, options)` scans
`[checkpoint.row_ordinal(), end)`. The start is always the supplied opaque record
checkpoint; `end` is an absolute exclusive **raw public row ordinal**, not a length,
UEL ID, byte offset or accepted-row limit. There is no implicit prefix traversal.
Use `scan_selected_range(symbol, selection, checkpoint, end, options)` for exact
selection within the same raw window. Projection does not change the window;
`options.limit` counts only matching returned rows. Rejected rows still advance
the raw boundary, so the reader never continues beyond `end` to fill that limit.

Checkpoints can come from explicit raw cursor capture or a strictly opened index's
authenticated interval. Capturing a raw point before the first batch reads bounded
preamble/read-ahead, not a symbol prescan. Persistent reuse retains the existing
full source hashing/canonical replay contract; these APIs never open/build an index.

```rust,no_run
use gdxrs::{Reader, SymbolHandle, RecordCheckpoint, Result};

fn consume(reader: &Reader, symbol: &SymbolHandle,
           point: RecordCheckpoint, end: u64) -> Result<u64> {
    let mut cursor = reader.scan_range(symbol, &point, end, reader.scan_options())?;
    drop(point); // Cursor retains a charged copy and the open source.
    while let Some(batch) = cursor.next_batch()? {
        println!("{} rows", batch.len());
    }
    Ok(cursor.statistics().returned_rows)
}
```

The same Reader/requested symbol must own the checkpoint, including alias identity.
`start <= end <= raw_record_count` is validated from the resolved catalog count;
scalar default rows and universe UEL rows count as public rows. Bounds errors are
`InvalidOptions`; source/symbol mismatch is `SourceMismatch`. Catalog bounds alone
do not prove a corrupt file's complete stream. Decoder errors remain visible as
rows are consumed; persistent proof still requires strict source verification.

The range cursor owns its restart state and can outlive its checkpoint/index owner.
A coordinator retains the index owner when obtaining additional starts. Each
cursor/checkpoint/batch/decoder shares the existing root budget; no partition array,
full dictionary or full-result buffer is retained. Compressed resume verifies its
whole bounded restart block, so it can read/decompress bytes outside the raw window
without decoding prefix records. Source length/mtime checks retain the existing
immutability assumption; there is no competing-writer CAS guarantee.

Empty windows (including `[total,total)`), zero accepted limits and empty selections
perform no record/codec work. Constructors still validate identities/options and
can resolve alias catalog metadata. A nonempty window consumed through EOF must
validate pending count/framing; an explicit accepted limit may stop first, like an
ordinary scan. Always consume `next_batch()` through `None` to establish completion:
a full final batch can leave the EOF check for the next call. Drop cancels a cursor.

Unselected ranges export a checkpoint at the next raw ordinal, including interior
endpoints and accepted-limit stops. A new suffix/range does not inherit the old end.
Selected checkpoint export remains unsupported. Work counters are fresh per cursor;
decoded rows include rejections/failed fills, returned rows include only acknowledged
batches. Errors are sticky and discard unpublished output; no fallback/replay occurs.

The next task is explicit bounded workers after this primitive is reviewed. There
is no parallel speedup or new independently oversized qualification in task 037.
