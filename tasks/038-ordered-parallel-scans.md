# 038 — Explicit bounded ordered parallel scans

Milestone: M6. Prerequisite: 037 complete/reviewed.
Status: implementation complete; qualification in progress.

Implement opt-in parallel record scans from trusted ordinal ranges, following
decision 0031. Use safe Rust and `std` threads/channels initially. Reserve one shared
root for worker state, explicit thread-stack allowances, codecs, batches, queues,
reorder slots, coordinator and checkpoints before spawning. Bound concurrent tasks
and queued results; never allocate one full result per partition. Expose explicit
requested/admitted concurrency and fallible cancellation/close with joins. Preserve
lazy zero limits, ordered output, exact global accepted limits and sticky errors
with acknowledged progress. No silent restart after output or detached workers.

Test slow-first-partition backpressure, cancellation during every blocking state,
consumer drop, spawn/read/decode failures, source changes and cap denial. Compare
complete ordered output against serial/native expectations across all fixture kinds
and feature/platform configurations. A serial execution fallback must be explicit
and must not mask source/resource/corruption errors. No automatic policy or general
speedup claim yet. Evidence: `docs/evidence/038.md`.
