# 036 — Reproducible M6 phase profiles and parallel-read contract

Milestone: M6. Prerequisite: 035 complete, PR #30 merged at `efbd808`.
Status: complete for baseline/contract scope; PR #31 merged at `58af12b`. See [evidence](../docs/evidence/036.md).

Establish a reproducible baseline before optimizing or launching workers. Deliver
`python tools/dev.py m6-profile --profile smoke|small`: smoke validates recipes;
small measures two growing sizes of raw/zlib records, metadata and dense sidecars,
with three trials each. These are diagnostic profiles, not a replacement for M5's
independently oversized acceptance or a claim of parallel speedup.

Reuse the pinned native generators, full native/serial/indexed ordered digests,
original IDs/value bits, explicit build and strict reopen from M5. Separate cold
serial, build, source hashing, canonical replay, first query and warm session reuse.
For full record scans additionally time Reader open/symbol lookup, cursor setup,
batch production and the validating sink separately. Keep complete sink verification
enabled. These are wall-clock phase measurements, not sampled function/CPU profiles;
do not attribute a phase total to a specific hot function without further evidence.

Every Rust worker retains one 256 MiB library root, self-checked hard/soft 512 MiB
Linux address-space limits, observed RSS/CPU/I/O/descriptors, streamed output and
bounded diagnostic records. Preserve index quota/cache/handle admission. Record
revision, source/binary/input hashes, hardware/compiler/storage, trial dispersion,
cache policy and failures. Reject incomplete trials, wrong digests, missing caps,
changed measured sources and impossible timing accounting. Retire only owned inputs
after full validation and unchanged-source checks. The small baseline must run on
committed measured sources. Keep portable reader tooling free of Linux-only imports.

Write a design decision specifying ordinal partition semantics, checkpoint trust,
one shared root, worker stacks and bounded queues/reorder buffers, global accepted
limits, backpressure, cancellation/join, acknowledged progress, source changes and
failure ownership. No production worker, new dependency, automatic planner, public
API or serialized-format change belongs in this task. Specify bounded follow-up
tasks; leave them pending and preserve the M5 source-bound evidence as historical.

Acceptance: negative tooling tests for the diagnostic contract; all tooling tests;
`reader-check`; both smoke and committed-source small profiles; local links/diff;
four-platform native functionality CI stays enabled. Record exact counts, findings,
limitations and next candidate in `docs/evidence/036.md`. Runtime sources are unchanged;
full M3/M4 scale reruns are required only if a later implementation affects them.
