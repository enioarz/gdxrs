# 006 — Oracle baseline and M0 review

**Milestone:** M0. **Prerequisites:** tasks 001–005 complete. **Goal:** establish a reproducible measurement method and verify that M1 can begin without unresolved format or contract decisions.

## Deliverables

- Deterministic streaming workload generators and measurement tooling under `tools/bench/`, reusing the pinned oracle driver.
- `tools/dev.py benchmark-oracle --profile small` and `m0-check`.
- `docs/benchmarks/baseline.md` and a compact machine-readable baseline report with execution revision, environment and validation results.
- `docs/reviews/m0.md`: findings and disposition from an independent review of the exact candidate revision.
- `docs/evidence/006.md`, an updated queue, and draft M1 task specifications written from verified contracts only after the review gate passes.

## Benchmark scope

This measures the upstream reference, not a Rust speedup. Provide a mandatory small profile suitable for ordinary development and a documented large profile for later dedicated runs. Large-file or memory-cap results are not required to claim the small profile passed, but unrun large cases must remain explicitly unrun.

The small profile uses deterministic sorted raw data and contains, at minimum:

- Uncompressed and compressed two-dimensional parameters with one million records, bounded UEL cardinality, exact reference counts and a streaming semantic result digest.
- A variable workload covering all five fields, plus a metadata-heavy workload distinct from the record-heavy cases.
- Separate timings for generation/setup, opening/metadata, raw record scan and raw sorted write/finalization. Label registration and compression settings are reported rather than hidden.
- One warmup and at least five measured repetitions per scenario, with median and dispersion. Distinguish application cache state from OS cache state. Do not label a run “cold” without a documented controlled method.

Generate inputs without collecting all records. Read and validate the entire measured output as a stream. Expected digests must derive independently from the deterministic input recipe/canonical representation, not only from the oracle output being timed. Document which native oracle operations may themselves allocate large metadata tables.

Report records/s, file bytes, elapsed time, CPU time where available and peak RSS using a supported process measurement method. Record unavailable metrics as unavailable, never zero. Keep semantic dump formatting outside the raw scan timing; a native sink can count/hash decoded fields for validation. Explain the sink's overhead and use the same work definition for later Rust comparisons.

## Aggregate check

`m0-check` runs the implemented bootstrap, oracle, fixture, specification and contract checks, validates the current small-baseline report provenance, and checks review status. It must report these separately:

- Automated prerequisites passed/failed, with actual counts and fresh run evidence.
- Benchmark executed/failed/unrun and the revision/environment measured.
- Independent review accepted/changes required/unavailable, with the exact reviewed revision.

Do not silently rebuild expectations, approve reviews or mark tasks complete. Reject an empty fixture/test selection and reports from unrelated revisions. A documentation-only status/evidence follow-up may refer to the preceding reviewed code revision if the diff is recorded and contains no implementation/contract changes.

## Independent review checklist

A reviewer other than the implementation author must examine the source evidence and record findings. This can be a human or an independently assigned agent when delegation is authorized; do not spawn agents solely because this document mentions that option. No repeated user confirmation is required for ordinary task commands.

The reviewer must check:

1. Exact source/dependency pins, build isolation and correct oracle library identity.
2. Fixture expectations independent of the decoder under development; mandatory coverage and intentional failure checks actually executed.
3. Byte-layout/tag/width/default rules grounded in source and experiments, including compression crossings and scalar behavior.
4. Complete public API inventory with explicit exclusions rather than silent omissions.
5. Reader/value/resource contracts free of hidden eager metadata/record allocations or ambiguous normalization.
6. Honest benchmark methodology, result validation, and separation of measured versus proposed performance.
7. Remaining questions assigned to later milestones without leaving an M1 prerequisite unresolved.

Record severity, affected artifact, required action, fix and verification for each finding. Re-review substantive fixes. Without an available independent reviewer, complete the automated work and mark the M1 gate pending; do not fabricate approval or begin implementation across the gate.

## Acceptance commands

```sh
python3 tools/dev.py benchmark-oracle --profile small
python3 tools/dev.py m0-check
git diff --check
```

The benchmark must execute all declared small-profile scenarios and repetitions with correct results. `m0-check` succeeds only when required automated checks, a valid baseline and the independent review all pass. It must fail with an explicit pending-review reason when that is the only missing prerequisite.

## Completion gate and next tasks

After acceptance, write separate M1 task specifications for checked primitives/header validation, bounded compression blocks, lazy metadata navigation, record decoding and serial batch cursors. Each must name exact rule/fixture IDs, allowed files, concrete expected error behavior, resource checks and review requirements. Do not implement them in this task.

M0 completion means implementation prerequisites are verified. It does **not** mean the Rust library reads GDX, meets a performance target, or supports larger-than-RAM operation yet.
