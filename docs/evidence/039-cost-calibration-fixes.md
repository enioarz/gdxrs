# Task 039 — selected costs and matching calibration

Implementation revision `f70de1a4eef80cecafbd48da9cdbd29f9e276d69` addresses the
three P2 findings reviewed at `ef99091` on PR #35.

1. Build break-even uses the chosen indexed or parallel warm cost against serial.
   The reported reproduction (10s serial, 9s indexed, 4s parallel, 22s setup)
   now reports four queries for parallel and 22 for indexed. Serial plans and
   nonpositive savings report no break-even; validated-session requirements remain.
2. Parallel calibration uses the execution defaults: one interval per task,
   with maximum in-flight tasks equal to the worker count. Calibration records
   actual dispatched tasks and index interval counts; both the worker and report
   validator reject a grouping mismatch. The planned full-scan regression checks
   the same task count while retaining exact cross-choice result comparisons.
3. The measured-best guard includes every measured eligible worker count (2/4/8),
   along with serial and indexed. Deterministic regressions reject a 1s planned
   scan when any measured worker count takes 0.01s.

The 1.5x tolerance, workload sizes, worker counts, memory caps, exact digests and
native reference checks remain enabled. There are no new dependencies or format
changes. Earlier qualification remains tied to its recorded revisions.

Validation and source-bound reports are recorded in
[the structured record](039-cost-calibration-results.json). This follow-up is
not independent approval or task-040 performance/oversized acceptance.

## Validation

- All 12 focused policy Rust tests and nine policy tooling tests pass.
- Full `reader-check` passes: default 192/48/6/7; minimal 179/47/6/7;
  all features 239/59/8/8; index-only 224/58/8/8
  (unit/integration/ordinary-doctest/compile-fail). All feature configurations run
  without filters or skips; all-target Clippy, dependency audits, ten frozen
  native fixtures and 141,358 public records pass.
- `m0-check` passes, including all 82 tooling tests.
- All six capped smoke cases pass with exact native/Rust digests and matching
  dispatched task/interval counts at each measured worker count.
- All six capped 1M/4M comparisons pass, including the strengthened measured-best
  guard over 2/4/8 workers. Smoke and small each passed on their first run; no
  retries or failed timing observations occurred in this follow-up.
- Formatting, diff whitespace and local documentation links pass.

[Native CI](https://github.com/enioarz/gdxrs/actions/runs/42407630470) runs on the
implementation revision: Linux passed; macOS ARM64/Intel and Windows were still
running when this record was written. Those native checks remain required before
merge. Task 040 remains separate and is the next M6 acceptance task.
