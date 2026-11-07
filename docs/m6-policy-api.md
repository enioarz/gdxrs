# Measured explicit scan policy

Task 039 adds an opt-in policy that chooses between serial, indexed and parallel
scans from caller-supplied reproducible measurements. See
[decision 0034](decisions/0034-measured-scan-policy.md) for the contract and
[evidence](evidence/039.md) for the qualified scope. The policy never measures,
scans, builds or opens anything by itself.

```rust,ignore
let policy = ScanPolicy::default(); // explicit conservative limits, adjustable
let plan = policy.plan(&measurements, scan_options.limit)?;
// plan.choice, plan.estimated_seconds, plan.reopen_seconds_charged,
// plan.build_break_even_queries, plan.alternatives (serial/indexed/parallel
// with allowed/reason) are all observable.
let (plan, mut scan) = reader.scan_planned(
    &symbol, &selection, scan_options, Some(index_open), &policy, &measurements)?;
while let Some(batch) = scan.next_batch()? { /* identical rows for every choice */ }
```

`ScanMeasurements` carries verified `rows`, a pruning estimate `selected_rows`,
`session_valid`, and measured `serial_full_scan_seconds`, `indexed_selected_seconds`,
`parallel_full_scan_seconds`, `strict_reopen_seconds` and `build_seconds`. Build
seconds are reported for break-even only; a plan never builds. `ScanPolicy` fields
are `choice_override`, `max_workers` (1–8), `allow_strict_reopen`, `small_rows`,
`head_limit`, `min_prune_fraction` and `required_parallel_advantage`.

Conservative defaults: head/zero limits and at most `small_rows` rows choose
serial; without a valid session and with reopen disallowed only serial is
available; indexed needs the selected fraction strictly below
`min_prune_fraction`; parallel needs the measured advantage
`required_parallel_advantage` over serial. Ties resolve serial, indexed,
parallel. `scan_planned` requires the caller's Ready `IndexOpen` for
indexed/parallel choices and preserves exact filters, global limits, source trust
and acknowledged progress across every choice. No automatic release or universal
threshold is implied; task 040 gates performance and oversized acceptance.


Defaults are adjustable guardrails rather than universal performance guarantees.
Measurements should represent the intended query and concurrency. The
parallel measurement must use the selected worker count (including overrides),
one interval per task and at most that many in-flight tasks, matching dispatch.
Build break-even uses the chosen indexed or parallel warm saving against serial;
serial plans, missing validated sessions and nonpositive savings report `None`.
The planner's
parallel cost estimate does not reserve memory; dispatch still performs the
explicit worker admission checks and reports resource failures. Both indexed and
parallel dispatch require a consistent Ready status with an index owner.

The `Reader::scan_planned` Rustdoc contains a compiling complete example. Calling
`close()` stops subsequent batches and retains raw cursor checkpoint positions;
errors stay sticky. The policy benchmark's report validator rejects duplicate query
shapes, missing worker measurements and non-finite/negative planned durations.
Its measured-best comparison includes every measured worker count (2, 4 and 8),
and calibration reports verify one dispatched task per index interval.
