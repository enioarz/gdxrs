# 0029 — Exact streamed indexed selections

Task 034 builds on merged PR #28 (`690cf30`). The optional `index` feature and
its dependency graph remain unchanged. Task 035 retains M5 OS-capped scale,
cold/warm trials, throughput and release gates.

`Reader::scan_selected_indexed(symbol, selection, options, open)` consumes an
explicit `IndexOpen`. It does not open, validate, hash or build a sidecar. Callers
explicitly pay task 033's source digest and canonical replay costs with
`Reader::open_index`; these remain separate from query work. `ScanCursor::into_index`
consumes/cancels the cursor and recovers the validated session for a warm query.
Ready sessions must match the Reader's open source and requested symbol, including
aliases. Inconsistent public open results and foreign sessions are rejected.

Off/Missing/Rejected execute ordinary exact serial selection, retaining rejection
diagnostics. Ready uses bounded interval-by-interval planning through authenticated
directory paths. Per-axis min/max overlap with normalized One/AnyOf/Range/Ranges
constraints is necessary, never sufficient. Disk membership is conservatively
ignored for pruning and evaluated exactly in the residual selection. Clauses are
conjoined, unions remain exact, and filtering precedes projection and the global
accepted-row limit. Scalars, universe rows and alias IDs retain their raw semantics.

No candidates, positions or directory entries are collected. One current half-open
ordinal span is retained. Adjacent candidates extend the existing decoder instead
of restarting; a gap releases its decoder before admitting the next checkpoint's
codec buffers. The final candidate still validates its pending terminator unless
a caller limit stops first. A proven-pruned suffix need not be decoded.

All/disk-only selections start sequentially. Other selections switch to a sequential
tail when at least eight intervals have been observed and at most one eighth were
pruned. This is an online heuristic, not an index/record prescan: a one-row limit
can stop in the first candidate. A sequential tail retains exact residual filtering.
The initial implementation uses task 033's one-page cache and bounded authenticated
point paths; planning may cost multiple directory reads per interval. Task 035 must
measure this cost rather than assuming a speedup.

An InvalidIndex encountered before any batch escapes can discard the unpublished
fill and restart serially. The fill's tentative accepted count/limit and buffers
are reset; actual decoded work is retained. Its original error is available via
`index_rejection`. Source changes, source/index access errors and admission failures
remain errors. After any batch escapes, failures are sticky and terminal. No
acknowledged rows are replayed. Partially filled failed batches never escape.
The invalidated session cannot become usable by recovering it from the cursor.

Cursor state (including the inline planner), session/cache, selections, one span's
checkpoint and codec/batch buffers share the Reader root. No additional handles,
spool, worker or dependency is introduced. Ordinary lending and cancellation apply.
Source integrity is checked at fill entry, interval access and successful fill exit.

`ScanStatistics` counts actual decoded rows (including rejected/discarded work) and
acknowledged returned rows. `indexed_statistics` separately reports considered/
pruned intervals, adjacent coalescing, checkpoint restarts, query index reads/cache
hits, the immutable successful-open validation snapshot, and source read/decoded-buffer deltas during
cursor calls. Shared-source concurrent I/O can contribute to source deltas, as with
`Reader::io_statistics`; unrelated work between calls is excluded. These are work
counters, not a claim of cold/warm latency or aggregate process memory containment.

An explicit warm-reuse sequence (error handling uses the ordinary Result API):

```rust
let open = reader.open_index(&symbol, &path, index_options, IndexPolicy::UseIfValid)?;
let mut query = reader.scan_selected_indexed(&symbol, &selection, options, open)?;
while let Some(batch) = query.next_batch()? {
    consume(batch); // Lending buffers cannot outlive the next call.
}
if let Some(index) = query.into_index() {
    let open = IndexOpen { status: IndexStatus::Ready, index: Some(index) };
    let next = reader.scan_selected_indexed(&symbol, &next_selection, next_options, open)?;
    // No source digest/replay is repeated by this query constructor.
}
```
