# Task 040 — review fixes

Review of PR #36 at `06bc946` reported one P1 and five P2 findings plus one
evidence typo; all are fixed on branch `vibe/m6-acceptance-040-51c6b4`. The
original [040 evidence](040.md) at `795a50f` is retained with the root-peak
correction (29,344,512 bytes).

## Fixes

1. **P1 tooling-test timeout** — `tools/tests/test_m6_acceptance.py` no longer
   computes discarded full-scan digests for `all` phases (previously hashed
   33.6M/67.2M metadata rows, then overwrote the digest). The six-test file now
   runs in ~0.1 s; the full tooling suite (83 tests) in ~10 s, well under M0's
   180 s budget.
2. **P2 RSS-growth guard** — the baseline is now the smallest case only
   (`rows[0]`), and every worker phase of the largest case (serial, parallel
   2/4/8, cancel/head/zero/warm) is checked against it with the fixed 64 MiB
   allowance. Mutating only a parallel run to 400 MiB now fails qualification.
3. **P2 empty phase arrays** — `validate_parallel` requires the exact expected
   phase count (1 for single runs, 3 for warm repeats) and matching query names;
   empty or truncated phase lists fail.
4. **P2 validation order/scope** — per-trial validation (`validate_trial`) now
   runs inside the trial loop for every profile, before any source/sidecar
   cleanup; `qualify` additionally revalidates so replayed/mutated reports are
   still rejected.
5. **P2 cache-state mismatch** — parallel query timings are now explicitly
   labeled: they follow a strict source reopen in the same process (hash/replay
   warms the OS cache), while cold serial uses advisory eviction. The
   like-for-like comparison is reported as parallel-vs-indexed
   (`speedup_vs_indexed`); parallel-vs-serial is kept as an explicitly
   not-like-for-like informational ratio.
6. **P2 special values** — new `special` worker mode scans the frozen
   `semantics-0.gdx` symbol 7 (16 records, 9 sentinel values) with stride-1
   partitions across serial, indexed and 4-worker parallel scans, hashing every
   key and value identity (including `Special`/`Acronym` forms) and requiring
   identical digests plus nonzero special-value count. Wired into the smoke
   profile and recorded in the report as `special_values`.
7. **Evidence typo** — `docs/evidence/040.md` root peak corrected to
   29,344,512 bytes (verified against `040-results.json`).

## Validation after fixes

- `cargo clippy --locked --all-targets` clean on default, `--all-features` and
  `--no-default-features --features index`.
- `python -m unittest discover -s tools/tests`: 83 passed in ~10 s.
- `python tools/dev.py m6-acceptance --profile smoke`: passed, including the
  special-value case (serial/indexed/parallel digests identical, 9 specials).
- The retained scale report from `795a50f` re-passes the strengthened
  `qualify` guards (per-trial validation, exact phase counts, smallest-case RSS
  baseline across all worker phases).
- Full `--profile scale` re-run at `aebe5ce`: passed all six oversized cases
  with per-trial validation before cleanup (report retained under
  `.work/reports/`).
