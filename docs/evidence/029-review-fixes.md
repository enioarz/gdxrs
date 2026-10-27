# Task 029 — PR #24 domain callback review fixes

Reviewed baseline: `b0f7a97f32d34226cf6b9869b9aca2ecac8bb371`.
Implementation: `db274d13c411bea3c5f405048189512cfd607e80`.
Working directory: `/workspace/gdxrs`; pinned Rust 1.99.1 and unchanged native oracle.
[Source-bound reports](029-review-fixes-results.json) retain the complete fresh results.

Both user-reported P2 findings reproduced before the fix: a valid 64-byte reader
batch cap failed with InvalidOptions; appending during the first callback still
returned two acknowledged callbacks and complete=true.

- Domain preparation now takes min(configured reader batch cap, 4096). A focused
  test visits twice with a 64-byte cap, verifies mapped order and raw identity,
  then checks the map remains usable.
- Replay checks source integrity before each entry, after acknowledging each
  successful callback, and before successful completion. A source-mutating
  successful callback remains acknowledged; the result is terminal SourceChanged
  with complete=false. Checks after acknowledgment run before cursor EOF cleanup,
  so cleanup denial cannot replace SourceChanged with an IO error.
- The replay regression covers source append in either the first or last callback,
  each with successful or denied cursor/parent cleanup. It checks exact partial
  counts, retained secondary errors, sticky failure, no replay and abort retry.
  Explicit cursor close still precedes parent cleanup.

All commands exited 0 at the implementation revision:

- `cargo test --locked domain_callbacks_ -- --nocapture`: three focused unit tests
  passed (151 other unit tests and 40 integration tests filtered intentionally).
- `python3 tools/dev.py reader-check`: default/all each 154 unit, 40 integration,
  11 doctests; minimal 146, 39, 11. Ten frozen fixtures compare 141358 records.
  Formatting, Clippy and feature/dependency checks pass.
- `python3 tools/dev.py writer-check`: complete existing native/Rust author/read
  suite passes, including five metadata author cases and 20 context streams.
- `cargo fmt --check` and `git diff --check`: pass.

[Four-platform CI run 40517730390](https://github.com/enioarz/gdxrs/actions/runs/40517730390)
was queued for this implementation. Subsequent authenticated GitHub API queries
returned HTTP 401; its result is unverified here. Earlier task 029 platform evidence
is historical and does not qualify this fix. No new performance/RSS qualification
is claimed. Other milestone gates were not rerun for this bounded callback fix.

Implementer self-review confirmed batch configuration, acknowledgment order,
terminal failure, cursor-before-parent cleanup and retained retry ownership. This
is not independent review signoff. Task 030 remains next; no append work is included.
