# 0034 — Measured explicit scan policy

Task 039 builds on merged task 038 (`c191cb3`). It adds an opt-in, caller-measured
choice between serial, indexed and parallel scans under the existing `index`
feature. Task 040 still gates performance and independently oversized acceptance.

## Policy inputs and choice

`ScanPolicy::plan(measurements, limit)` is a pure deterministic function over
explicitly supplied `ScanMeasurements` (serial/indexed/parallel seconds, strict
reopen and build seconds, verified rows, a pruning estimate and session validity)
plus caller-specified limits (`max_workers`, `allow_strict_reopen`, `small_rows`,
`head_limit`, `min_prune_fraction`, `required_parallel_advantage`) and an optional
`choice_override`. It performs no I/O and never scans a source or directory to
choose a plan. There is no unmeasured universal threshold and no hidden cache:
every bound is a public field.

Conservative rules: head/zero accepted limits and small inputs choose serial;
without a valid session and with reopen disallowed only serial is available;
indexed requires the selected fraction to be strictly below `min_prune_fraction`
(poor pruning); parallel additionally requires the measured parallel cost to beat
serial by `required_parallel_advantage` (admission pressure). Otherwise the
allowed alternative with the lowest estimated cost wins, ties resolving serial,
indexed, parallel. The plan reports every alternative with its cost, disposition
and reason, the charged reopen seconds, and the build+reopen break-even in queries
when the warm saving is positive.

## Execution and invariants

`Reader::scan_planned` validates the selection, computes the plan, then executes
through the existing exact cursors: serial `scan_selected`, indexed
`scan_selected_indexed`, parallel `scan_selected_parallel`. An `Indexed`/`Parallel`
choice requires the caller's already opened Ready `IndexOpen`; nothing is built or
opened implicitly. `PlannedScan` exposes one `next_batch`/`statistics` surface, so
exact filters, projection, global accepted limits, source trust, ordered delivery
and acknowledged progress are identical across every choice. An explicit override is
validated against the caller's worker limit and otherwise bypasses only the cost
rules.

## Evidence requirements

Deterministic policy unit tests cover overrides, head/zero, small inputs, poor
pruning, admission pressure, reopen charging, tie-breaking, break-even and invalid
inputs; dispatch tests prove identical ordered digests across all three choices on
a frozen fixture. `tools/m6_policy.py` (`m6-policy --profile smoke|small`) performs
actual end-to-end comparisons on held-out representative raw/zlib/metadata inputs:
each case measures serial, build, strict reopen, warm indexed and 2/4/8-worker
parallel scans, feeds those measurements to the real policy, executes the planned
scan for all/leading/head/zero shapes and requires exact ordered digests, the
conservative serial choice for head/zero, and planned timings within 1.5x of the
measured best for all/leading. All workers keep the 256 MiB root and enforced,
self-checked 512 MiB Linux address-space cap. No automatic release, general
throughput parity or universal threshold is claimed.
