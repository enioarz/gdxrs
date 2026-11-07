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
