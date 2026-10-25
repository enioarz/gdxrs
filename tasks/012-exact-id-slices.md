# 012 — Exact bounded raw-ID slices

Milestone: M2. Prerequisite: M1 merged (`5cc766d`). Read AGENTS.md, implementation plan §5–6, reader/resource contracts, and decision 0006.

## Deliverables

Add source/symbol-bound, budgeted immutable selections with a conjunction of per-dimension All, one ID, allowed IDs, inclusive ranges and unions of ranges. ID order is registration order. Explicit empty sets select nothing; malformed dimensions/ranges, duplicate dimension clauses and out-of-source IDs fail before data I/O. Keep the existing scan API and add selected scanning. Filter complete decoded keys before projection and the exact accepted/public-row limit. Empty selections/zero limits allocate no decoder/output and read no record bytes. Preserve original IDs and row order; no implicit index or record prescan.

Primary files: src/selection.rs, src/scan.rs, src/error.rs, src/lib.rs, focused tests, docs/evidence/012.md. No labels, spill, sidecars, writer, workers or Polars in this task. All selection payload capacities compete for the existing root quota; fallible allocations, bounded shared controls and drop ownership remain explicit. Expose per-cursor decoded/returned row counters to distinguish filtering from disk pruning.

## Acceptance

Compare index-free slices to independently authored frozen record expectations for all ten fixtures: leading/non-leading/conjunctive constraints, union, empty/all, scalar/universe/alias, both endian/compression profiles and all fields. Exercise tiny batches, projection excluding filter dimensions, exact limits after rejection, source/symbol mismatch, invalid dimensions/IDs/ranges, duplicate clauses, data failure during filtered fill, quota rejection/clone retention/drop and zero data reads for empty/zero-limit. Count actual decoder work and verify terminal/no-partial-batch behavior. Run reader-check, m0-check, focused checks and diff validation; retain exact counts/revision and author review. Do not claim M2 completion or out-of-core release from this task.
