# 041 — Polars integration contract

Milestone: M7. Prerequisite: 040 complete/reviewed and merged.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Define the adapter contract in `enioarz/polars-gdx` before implementation. Pin the
qualified gdxrs revision and supported Rust, Python, PyO3, Arrow and Polars versions.
Inventory current metadata/domain/UEL operations, `value_field`, `key_filter`,
`n_rows`, `threads`, projection and batch-source behavior against the existing
adapter and independent native fixtures. Record compatibility requirements,
intentional exclusions and explicit special-value/acronym conversion policies.

Specify backend selection, schema/dictionary modes, batch ownership, cancellation,
error propagation and repeated scans. Define one end-to-end resource envelope for
core buffers, adapter conversions, dictionaries, queues and the streaming sink;
account separately for Python/Polars runtime overhead and caller-retained output.
Set concrete library/process/scratch/handle caps and growing record-heavy and
metadata-heavy acceptance recipes before implementation. A core reservation alone
cannot establish adapter RSS containment. Keep all adapter dependencies outside
gdxrs; any missing core primitive needs its own scoped proposal.

Acceptance: a reviewed behavior/resource matrix, named existing regression tests,
independently authored expected results for gaps, reproducible baseline commands
and a documented supported-version/platform matrix. Identify commands and fixtures
that remain to be implemented; do not report them as already passing.

Evidence: `docs/evidence/041.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
