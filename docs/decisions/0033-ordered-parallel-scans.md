# 0033 — Explicit ordered read workers

Task 038 builds on merged task 037 (`7f7e19a`). It adds opt-in parallel scans under
the existing `index` feature, using safe `std` threads and no new dependency.
Tasks 039/040 still gate automatic policy and performance/out-of-core acceptance.

## Explicit API and admission

`Reader::scan_parallel(symbol, index, scan_options, parallel_options)` consumes an
already strictly validated `CheckpointIndex`. `scan_selected_parallel` additionally
takes an exact `Selection`. Both return a lending `ParallelScanCursor` with
`next_batch`, `statistics`, fallible `cancel`/`close`, and joining `Drop`. There is
no implicit sidecar open/build, index fallback or serial retry. An admitted single
worker is still an actual worker, not hidden serial fallback.
`into_index` instead joins and recovers the explicitly validated owner for reuse;
close/cancel release it. Failures never return a reusable session silently.
Requested workers are a ceiling naturally capped by available tasks; Exact forbids
additional budget-driven reduction. The in-flight option must be at least requested
workers; capacity above admission is not allocated speculatively.

`ParallelOptions` specifies requested workers (1–8), maximum in-flight tasks (1–8),
intervals per task, explicit stack bytes (2–8 MiB) and `Exact`/`Reduce` admission.
Reduce may lower concurrency only during shared-root admission, never to mask an
I/O, source, corruption, allocation or spawn failure. Report requested/admitted
workers, stack/control allowances, task/output peaks, decoded/produced/acknowledged
rows and joins. Zero accepted limits, empty selections and verified empty symbols
validate structure/binding but launch no worker and read no record interval.

One root charges coordinator/Arc/mutex control, fallibly allocated fixed slot and
join-handle capacities, explicit rounded thread stacks plus guards/runtime allowance,
checkpoints, codec and batch buffers. Stacks round to 64 KiB, plus a 64 KiB guard
allowance; reserve another 256 KiB per worker for standard thread/TLS/control state.
The caller-provided stack size is set explicitly on `thread::Builder`. These are
conservative payload/control reservations, not a universal RSS/allocator-arena model;
actual VM/RSS and allocator/runtime overhead still require OS-capped measurement.
Fixed small standard Arc/thread control allocations retain the existing bounded
control-allocation limitation: Rust allocator exhaustion can abort. Variable-sized
storage uses fallible reservation before allocation; OS spawn failure is fallible.

Initial task cursors, restart decoders and full reusable output batches are actually
prepared before any spawn, so bulk buffers are admitted rather than estimated.
Preparation can read bounded record preambles/restart blocks but decodes no rows.
Later tasks are prepared by the coordinator before dispatch to an existing worker,
after retiring that slot's previous cursor. Other same-Reader allocations may cause
later preparation to fail; that is a sticky budget error, never an unreported change
of concurrency. No worker creates a new root, opens a source or revalidates an index.

## Fixed slots and order

Use root-charged fixed slot storage protected by a mutex/condition variable, avoiding
opaque channel allocations. A worker owns at most one task and one output batch.
It transfers the batch into its slot and waits until the consumer recycles it before
producing another. The coordinator borrows that same batch, then recycles it at the
next call. Thus queued/borrowed output has at most one batch per admitted worker;
there is no reorder map or whole-partition result. A slow first task backpressures
later workers. Consecutive authenticated intervals form tasks, assigned monotonically
and delivered in raw ordinal order. New tasks replace only completed frontier slots.
The cloneable cancellation handle retains only a small root-charged signal, not
task/results or a source handle. Its request wakes a coordinator on another thread;
the owning cursor still joins and reports errors. Predicate checks, wait registration
and notifications use a separate signal gate, with lock order state then gate.
Waiters drop the gate before reacquiring state, avoiding missed wakeups/deadlocks.

Workers use task-037 ranges with fresh counters and no local accepted limit.
Each step consumes at most 1024 raw rows before checking cancellation again,
including all-rejected spans. Ordinary scans compile out this polling path.
Global accepted limits apply at the coordinator after exact selection/projection;
an unpublished last batch can be truncated. At the limit, cancel and join before
acknowledging that last batch. Speculative work is counted and may discover errors
outside the eventual accepted prefix; an observed fatal error is never hidden.
The original global limit also bounds each reusable batch's capacity, without
becoming a per-worker accepted-row termination condition.

## Failure and lifecycle

Output acknowledgment and first-error publication serialize under the shared mutex.
A batch committed before a later error is an acknowledged prefix; no new batch is
committed after a known fatal error. Workers report work even for failed fills.
Read/decode failures and caught worker panics become sticky terminal errors. Stop
admission, publish cancellation, wake all task/result/recycle waiters and join all
started threads on every exit path, including partial construction and Drop.
Regular-file I/O cannot be forcibly interrupted; cancellation has no wall-clock
deadline while an OS read is blocked. At most one fixed-size error per worker/join
is retained, with secondary diagnostics appended without replacing an earlier cleanup
error. No unpublished batch escapes a failure and no acknowledged row is replayed.

`close`/`cancel` stop without draining, report errors and are idempotent. Explicit
cancellation is not successful exhaustion. Drop requests stop and joins, but cannot
return diagnostics. Normal exhaustion returns None only after source checks/joins;
limit completion is distinct from raw EOF. Source length/mtime assumptions remain.
Per-worker global I/O deltas would overlap: use Reader's aggregate source counters,
which can also include other same-Reader activity, rather than summing worker deltas.

Required tests cover frozen native order/values, all features/platforms, root and
stack admission, slow-first backpressure, each waiting state, drop, exact global
limits, source/read/decode/spawn/panic failures, partial spawn joins and primary plus
secondary diagnostics. No general speedup or new independently oversized acceptance
is established by this task's functional worker implementation.
