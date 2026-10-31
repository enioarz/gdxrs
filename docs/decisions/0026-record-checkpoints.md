# 0026 — Complete, opaque in-session record checkpoints

Task: [032](../../tasks/032-record-checkpoints.md). PR #26 merged as `6b29a9e`;
the user's instruction to begin M5 clears the advancement gate. The M4 measured
scope and throughput limitations remain recorded in [031 evidence](../evidence/031.md).
M5 persistent sidecars and indexed planning are separately gated tasks 033–035.

## Public contract

`ScanCursor::checkpoint()` returns an owned `RecordCheckpoint` immediately before
the next raw public row. Its `row_ordinal()` counts rows preceding the point,
including synthesized empty scalar defaults and universe rows. It is not a byte
offset or a retained result batch. Export before the first row explicitly parses
the bounded data preamble/read-ahead; it does not decode a record prefix. Normal
scans, including zero limits, keep their existing lazy behavior.

`Reader::scan_from_checkpoint(symbol, point, options)` starts a fresh unselected
raw suffix scan. The same open source (Reader clones are allowed) and same requested
symbol are required. Alias checkpoints stay bound to the alias, even when another
handle resolves to the same underlying set. An identical reopened file is another
source and is rejected. This is session identity, not a content digest or pathname
lock. Existing immutable-source length/mtime observations still apply.

Projection does not alter raw checkpoint state: unprojected fields/keys are still
decoded for framing. Resume takes fresh projections, batch cap and accepted-row
limit; counters measure only the new scan's work. Selected cursors explicitly reject
export without poisoning the cursor. Selection-aware indexed interval planning is
task 034, not an implicit part of this API.

Root reservation denial when exporting a point leaves the cursor usable. Source,
I/O or decoder failure is terminal and repeated next/checkpoint calls preserve the
original error/context. A failed fill exposes neither a partial batch nor point.
Existing points never trigger an implicit fallback/retry or duplicate delivered rows.

Example (within one immutable open Reader):

```rust
use gdxrs::{OpenOptions, Reader, ScanOptions, SymbolId};
fn example() -> gdxrs::Result<()> {
let reader = Reader::open("model.gdx", OpenOptions::default())?;
let symbol = reader.symbol(SymbolId::new(1)?)?;
let mut scan = reader.scan(&symbol, reader.scan_options())?;
if let Some(batch) = scan.next_batch()? {
    // Consume this batch before checkpointing the cursor.
    println!("{} rows", batch.len());
}
let point = scan.checkpoint()?;
drop(scan);
let mut suffix = reader.scan_from_checkpoint(
    &symbol, &point,
    ScanOptions { limit: Some(100), ..reader.scan_options() },
)?;
while let Some(batch) = suffix.next_batch()? {
    println!("{} suffix rows", batch.len());
}
Ok(())
}
```

## State and replay proof

The fixed state carries source/requested/resolved identity, data region, physical
record count, kind/subtype/dimension/byte order/codec, bounds/widths, preceding keys,
original emitted ordinal, EOF state and byte-stream checkpoint. The latter records
raw byte position or independent compressed block start plus decoded offset and
consumed logical bytes. Every restored tuple is checked against the freshly parsed
bounded data preamble; inactive axes retain sentinel state. Original count/order
checks remain enabled, including a pending terminator after the last physical row.
An already consumed empty scalar default is not emitted twice.

A resumed decoder reads the preamble, releases those stream buffers, then creates
an independent stream at the target block. Stored/zlib blocks are fully read and
validated before skipping the saved decoded bytes. No inflater state is serialized:
each GDX compressed block is an independent wrapped stream. A record may cross
blocks, and a block may contain many checkpoints. Offset equal to decoded block
length is valid; above 32 KiB, beyond actual decoded length, or nonzero decoded
offset in raw mode is invalid. Logical byte counters continue across repeated
resumes instead of restarting at zero.

Public checkpoint fields are private and no serialization/import API is exposed.
Structural checks are not authentication of an arbitrary forged previous tuple or
summary. Task 033 must explicitly resolve the sidecar trust/identity/validation
contract before importing any serialized state; source digests and unkeyed cache
checksums alone do not prove attacker-authored checkpoints correct.

## Resources and scope

Each point reserves its fixed inline size from the Reader root and retains that
source until dropped. It is not implicitly Clone: applications explicitly export
additional charged points. Cursor inline saved state is covered by its existing
size-based root reservation; it preserves a boundary after releasing decoder buffers
at limits/EOF. There is no production checkpoint vector, directory, cache, file,
background task or new dependency. Tests collect only bounded authored fixtures.

Ordinary scan hot-row decoding is unchanged. An explicit restore may re-read the
first data block for the preamble plus the target block; decoded-work/I/O counters
include this work. No prefix records are decoded. No persistent-index, large-index
OS/RSS, warm latency or general acceleration claim is made by this task.
