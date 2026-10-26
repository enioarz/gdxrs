# 024 — Replay cleanup diagnostic review fix

Fix revision: `db6d093f276a8fb754221003eae97688673fb521`, addressing the
review of PR #19 at `5dd8c84`. [Fresh reports](024-review-fix-results.json)
retain the tested source revision and report hashes.

The reviewer reproduced `OutputLimitExceeded` without a secondary cleanup error
when merged-row replay failed and cursor cleanup was denied. The new regression
reproduced the missing `cleanup_error()` before the fix. This was a diagnostic
contract failure: publication remained blocked and parent cleanup could succeed.

Replay now saves its result and explicitly closes the cursor before propagating
any replay error to the parent. A close failure is attached as a secondary error
unless the primary error already has one. The primary operation kind remains
unchanged. Successful replay still requires sorted EOF/count/completion checks.
Cursor handles are closed before parent cleanup; retryable ownership is unchanged.

The regression covers both output-limit and malformed-record decode errors with
injected cursor cleanup denial. Each checks the primary kind, retained secondary
error, blocked publication, successful parent cleanup and successful abort. The
fault permits final-run publication before denying cursor removal, so it exercises
replay rather than sorter initialization/merge failure.

Fresh `reader-check` passes default/all 93 unit tests, 38 integration tests and
seven doctests; minimal passes 85, 37 and seven. Formatting, Clippy and ten frozen
fixtures / 141358 public records pass. Fresh `writer-check` passes all existing
cases and all five forced-merge unsorted native/Rust cases.
[Native validation run](https://github.com/enioarz/gdxrs/actions/runs/40382469389)
passes on Linux, macOS ARM64/Intel and Windows/MSVC, including debug
features and optimized default tests. The tested PR merge tree matches the fix
revision; native summaries and the matching tree identity are retained in JSON.

The reviewer confirmed the prior 47 tooling tests, label-page checks and eight
OS-capped smoke cases passed at `5dd8c84`; those unrelated gates were not rerun
for this diagnostic fix. Their original evidence remains in [024](024.md).
Review disposition here is implementer self-review, not independent signoff.
