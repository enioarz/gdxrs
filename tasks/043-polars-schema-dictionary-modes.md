# 043 — Stable schemas and bounded dictionary modes

Milestone: M7. Prerequisite: 042 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Implement the schema and conversion policies chosen in task 041 in the adapter.
Provide a bounded large-dictionary mode using original UEL IDs with explicit label
resolution and/or bounded string batches. Keep full-dictionary Enum as an explicit
budget-admitted mode. Reject insufficient admission clearly; never silently change
a planned schema or reinterpret batch-local category codes as global UEL IDs.

Choose schema before execution and preserve it across empty batches, partitions
and repeated scans. Define label byte/encoding behavior, domain names, value fields,
special values and acronym conversion without accidental lossy coercion. Account
for simultaneous core, conversion and Arrow/Polars buffers, including dictionary
unification and temporary copies; document ownership and unavoidable copies.

Acceptance: exact schema/value tests across the supported Polars versions; disjoint
labels across batches, non-UTF-8 labels, empty/scalar symbols, special values and
category identity cases. Exercise dictionary admission failure and growing label
tables through a bounded mode without materializing all categories. Full OS-capped
end-to-end qualification remains task 047.

Evidence: `docs/evidence/043.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
