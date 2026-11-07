# Task 039 — review fixes at `b8da11c`

Review of PR #35 at `7aa67c7` reported four P2 and two P3 findings; all are fixed
on branch `vibe/m6-policy-039-51c6b4` at `b8da11c` and pushed. The original
[039 evidence](039.md) at `124920f` is retained with a clarification of the filtered test count.

## Fixes

1. **P2 serial/indexed close()** — `ScanCursor::close` (src/scan.rs) now stops at
   the acknowledged prefix, releases decoder and batch buffers, is idempotent and
   reports sticky errors instead of hiding them; `PlannedScan::close` delegates to
   it for serial/indexed cursors. Regression test `serial_close_stops_at_the_acknowledged_prefix`.
2. **P2 empty symbols** — `ScanPolicy::plan` no longer rejects `rows == 0`;
   verified empty symbols plan (conservatively serial) including zero limits, and
   the pruning fraction guards the 0/0 division. Regression test
   `policy_allows_verified_empty_symbols`.
3. **P2 phantom reopen charge** — `reopen_seconds_charged` is now reported only
   when the chosen plan actually uses the index (indexed/parallel), on both the
   normal and explicit-override paths. Regression test
   `serial_plans_never_report_reopen_charges`.
4. **P2 construction excluded from timing** — `examples/m6_policy_worker.rs`
   starts the planned-scan timer before `scan_planned`, so admission, decoder
   preparation and worker startup are included in the reported seconds.
5. **P3 zero-worker override** — `ScanChoice::Parallel { workers: 0 }` overrides
   are rejected with InvalidOptions.
6. **P3 NaN min_prune_fraction** — validation now requires a finite fraction in
   (0, 1].

## Validation at `b8da11c`

- `cargo test --locked --lib --all-features`: 235 passed (10 policy tests; the broader `policy_` filter also matches 14 existing value-policy tests);
  `--no-default-features --features index` policy tests pass.
- `cargo clippy --locked --all-targets` clean for default, `--all-features` and
  `--no-default-features --features index`.
- `python -m unittest discover -s tools/tests`: 77 passed.
- `python tools/dev.py m6-policy --profile smoke` and `--profile small`: passed.
  (One smoke attempt run concurrently with the small profile failed its 1.5x
  timing-tolerance check under CPU contention; the sequential rerun passed.)

Four-platform CI on PR #35 remains the authoritative cross-platform gate.
