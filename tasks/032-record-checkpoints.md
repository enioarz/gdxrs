# 032 — Exact bounded record checkpoints

Milestone: M5. Prerequisite: 031 reviewed/merged (PR #26, merge `6b29a9e`).
Status: complete for the in-session scope; review pending.

Implement a complete record-boundary restart primitive before designing persistent
sidecars. Checkpoints are explicit, opaque, owned and charged to the same Reader
root. They bind to the exact open source and requested symbol (including aliases).
No wire-format checkpoint import, automatic indexing, whole-symbol prescan, new
dependency or general persistent-index/performance claim belongs in this task.

Deliver `ScanCursor::checkpoint`, `RecordCheckpoint::row_ordinal`, and
`Reader::scan_from_checkpoint`. The point is immediately before the next raw public
row. Raw unselected scans only; projections do not change raw checkpoint meaning.
Resume uses fresh projection/batch/accepted-row limit options and fresh work
counters. Selected scans explicitly reject checkpoint export until task 034 defines
that contract. Existing ordinary scans retain lazy/zero-limit behavior and lending.

Save source/requested/resolved symbol identity, schema/codec/order, physical or
compressed-block offset and decoded offset, consumed logical bytes, preceding keys,
key bounds/widths, original record ordinal/count and scalar/universe/EOF state.
Re-read the bounded data preamble on resume and compare bounds/widths before using
the saved tuple. Re-decompress and checksum an entire compressed restart block;
never treat a block start as a record restart. Validate structural state, source
immutability and record count/order exactly. A decoded offset equal to block end
is valid; out-of-range/raw nonzero decoded offsets are errors. Never synthesize a
second default scalar or skip validating a pending terminator.

Acceptance:

- Compare checkpointed suffixes against frozen independently authored expectations
  across all ten native fixtures (eight in minimal builds): LE/BE, key-width/delta
  boundaries, all value tags/acronyms, aliases, universe, empty scalars/sets.
- Exercise multiple points in one compression block, exact block-end points,
  records crossing raw/stored/zlib blocks, including a value crossing a block.
- Prove source/symbol mismatch, changed source, malformed private state, failed
  restore I/O/checksum, sticky cursor errors, exhausted/zero limits and exact EOF.
- Check per-cursor decoded/returned counts, source I/O work (no decoded prefix),
  root charging, budget denial and release. Retain no checkpoint collection in
  production; bounded small test collections are allowed.
- `python tools/dev.py reader-check`, `writer-check`, `label-pages-check`,
  `m0-check`, and all four native CI platforms stay enabled/passing. Record exact
  counts, source revisions, findings and limits in `docs/evidence/032.md`.

Task 033 is gated on this proof and review; no persistent reuse is claimed here.

## Completion

[032 evidence](../docs/evidence/032.md) records the qualification revision, exact
feature/focused counts, unchanged native fixture expectations, all four native
platforms, source hashes and self-review findings. No persistence or acceleration
claim follows from this task; 033 remains gated on review/merge.
