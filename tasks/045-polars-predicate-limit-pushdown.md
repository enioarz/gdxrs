# 045 — Exact predicate, projection and global limits

Milestone: M7. Prerequisite: 044 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Implement sound predicate/projection/limit pushdown for the gdxrs batch source.
Translate only predicates with proven equivalent core semantics; keep unsupported
expressions as residual predicates and retain their required columns until they
have been evaluated. Preserve case-sensitive labels, empty-filter behavior, null
and special-value policies and original record order. Push a limit below a residual
only when equivalence is proven; otherwise count globally accepted rows after
residual filtering across batches and partitions, then apply final projection.

Acceptance: compare optimized scans with an unoptimized reference plan using
independent fixture expectations. Cover sparse/all-rejected filters, matches only
in later batches, mixed pushed/residual predicates, predicates on projected-away
columns, zero limits, `head(n)` across boundaries, and supported null/value cases.
Validate exact ordered rows and counters for serial, indexed and explicitly
parallel execution. No per-batch limit, hidden index build or post-output retry may
substitute for a global accepted-row limit.

Evidence: `docs/evidence/045.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
