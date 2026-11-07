# Task 040 — timing and replay-admission fixes

Implementation revision `20271414b5caea205498e39e5067a95673054c36` fixes both P2 findings reviewed at `e66fbb4`
on PR #36. The branch is rebased onto merged task 039 and targets `main`.

`validate_trial` now requires exactly one build phase and two session phases,
with finite positive elapsed durations for build, strict reopen and indexed full
scan. Both live and replayed trials run `m5.validate_worker` to enforce the same
index byte quota, handle limit and cache allowance. `performance` validates every
trial before aggregating costs, preventing direct callers from bypassing checks.

Regression tests cover NaN, both infinities, negative and zero durations in each
of the three phases; each index admission field in both worker modes; and
missing/extra phases. Valid trials still qualify and retain their expected cost
medians. No limits, assertions or workload sizes were relaxed.

## Validation

- Nine focused acceptance tests pass; `m0-check` passes all 91 tooling tests.
- Fresh capped smoke passes all six cases / 18 trials, plus the 16-row frozen
  special-value case: serial/indexed/parallel digests match, including nine
  sentinel/acronym values. Each worker uses the existing 256 MiB root and enforced
  512 MiB process cap.
- The retained six-case / 18-trial scale report at `795a50f` passes strengthened
  qualification and performance validation. All 21 deliberately invalid timing
  and admission mutations of that report are rejected. This revalidates retained
  measurements; it is not a fresh scale execution or new performance claim.
- Formatting and diff whitespace pass. Rust sources, examples, manifest and lock
  file are unchanged from the reviewed `e66fbb4` reader gate, which passed all four
  feature suites, Clippy and ten native fixtures / 141,358 records. That reader
  gate was not redundantly rerun for these Python-only fixes. Native CI is
  refreshed by the push and remains required before merge.

[Compact results, source hashes and full-report identities](040-validator-fixes-results.json)
record the checks above. Earlier [scale evidence](040.md) remains tied to its
measured revision. This follow-up does not claim independent approval, an M6
release or a cross-platform performance result.
