# 047 — Polars streaming acceptance and migration gate

Milestone: M7. Prerequisite: 041–046 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Qualify M7 in `polars-gdx` with the pinned core and supported version matrix.
Run the complete existing Polars suite plus the new backend/schema/batch/predicate/
lifecycle regressions on Linux x86_64, macOS ARM64/Intel and Windows x64. Compare
full ordered IDs, values and schemas with independent native fixtures, not merely
row counts or backend-to-backend agreement. Verify installation and streaming via
the gdxrs-only backend without a GAMS/native GDX runtime.

Use noncollecting streaming sinks on growing independently oversized record-heavy
and metadata-heavy inputs under the concrete task-041 process and library caps.
Include raw/compressed data, large dictionaries, sparse residuals, projection,
short limits and cancellation. Measure whole-process RSS/VM, core and adapter
buffers, dictionaries, queues, scratch/handles and cleanup. Separate engine/sink
costs and caller-retained output; `.collect()` is not out-of-core evidence. Keep
platform functionality separate from platforms on which OS caps are measured.

Measure at least three trials with documented hardware/storage/cache state;
separate open/metadata, index build/strict reopen, first batch, conversion, full scan
and sink costs. Compare the legacy and gdxrs backends on equivalent decoded work,
schema and value policies. Report regressions and limits without universal speedup
claims. Retain exact source/version/input hashes, commands, counts, failures,
skips, unrun cases and structured reports; preserve applicable core gates for the
pinned revision and rerun affected gates if core changes were required.

Publish the compatibility/migration matrix and review disposition. Change the
default backend or remove `gdx-sys` only after required behavior, packaging and
acceptance gates are verified in a separately reviewable migration change. This
task list does not itself authorize a release or claim M7 is complete.

Evidence: `docs/evidence/047.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
