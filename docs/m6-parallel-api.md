# Explicit ordered parallel scans

Task 038 adds opt-in `index`-feature scans. See [decision 0033](decisions/0033-ordered-parallel-scans.md)
for admission, allocation and synchronization details, and [evidence](evidence/038.md)
for the qualified scope. There are no new dependencies or serialized formats.

Pass an already strictly opened `CheckpointIndex` to
`Reader::scan_parallel(symbol, index, scan_options, parallel_options)`, or add an
exact `Selection` with `scan_selected_parallel`. The index must belong to the same
Reader and requested symbol (including alias identity). Construction consumes it;
`cursor.into_index()` joins and returns it for further queries without repeating
source hashing/replay. `close`/`cancel` release it. No API opens/builds an index,
chooses concurrency automatically or retries serial execution after an error.

```rust,ignore
let mut cursor = reader.scan_parallel(&symbol, validated_index,
    reader.scan_options(), ParallelOptions {
        workers: 4,
        maximum_in_flight_tasks: 4,
        ..ParallelOptions::default()
    })?;
while let Some(batch) = cursor.next_batch()? {
    consume(batch)?;
}
let statistics = cursor.statistics();
let validated_index = cursor.into_index()?;
```

The compiled public Rustdoc example demonstrates the complete function signature;
the lending compile-fail example prevents overlapping batch access and advancement.
The consumer receives the same row order, IDs, value identities and projection as
serial execution. `ScanOptions::limit` counts globally accepted rows. Selection
rejections advance raw ordinals independently. A final unpublished batch is
truncated before acknowledgment, including every column's exposed length.

`ParallelOptions` defaults to two workers, two in-flight tasks, one authenticated
interval per task and two MiB stack bytes. Workers range from one through eight;
stack bytes range from two through eight MiB and round up to 64 KiB. The root also
charges a 64 KiB guard and 256 KiB thread/control allowance per admitted worker.
In-flight capacity must be at least requested workers and at most eight; the actual
implementation uses one task/batch slot per admitted worker. Available tasks cap
concurrency naturally. `ParallelAdmission::Exact` rejects budget-driven reduction;
`Reduce` may retry only budget denial before any spawn, never I/O, allocation,
corruption or spawn errors. Even one admitted worker runs on an explicit thread.

Worker stacks, control, checkpoints, codecs and batches share the Reader's existing
root. Initial task resources are allocated before threads start. Later tasks reuse
retired slots; concurrent allocations on the same Reader can still cause a sticky
budget failure during later preparation. Each worker transfers one bounded batch
and waits for recycling; a slow first partition cannot grow a reorder queue.
Zero accepted limits, empty selections and verified empty symbols launch no threads
and read no record intervals, while still validating options and source binding.

Always consume through `None` to establish successful completion, or call fallible
`close` when stopping early. `cancel` and `close` wake waiters and join without
draining; they are idempotent and preserve sticky errors. A cloneable
`cancellation_handle()` requests cancellation while another thread is blocked in
`next_batch`; the owner still performs joins and reports errors. Drop also joins,
but cannot return diagnostics. A retained handle keeps only a small charged signal.
Cancellation cannot forcibly interrupt synchronous OS reads: joins wait for such
reads to return. Worker steps check stop after at most 1024 raw rows, including
all-rejected spans; this is not a wall-clock cancellation deadline.

`statistics()` reports requested/admitted workers, stack/control reservations,
maximum/peak batch slots, dispatched/completed tasks, joins, completion/cancellation
and decoded/produced/returned rows. Active work counters are snapshots after decoder
steps; final joined counts include speculative/rejected work. Returned rows count
only acknowledged batches. Speculative failures beyond a requested prefix remain
fatal; no known failure is hidden to make a limit succeed. Source I/O uses the
Reader's aggregate counters because summing worker deltas would double-count.

Read/decode/source/spawn failures, worker panics and poisoned protocol state stop
all workers. No unpublished batch escapes after a known error; acknowledged output
is never replayed. The primary error retains secondary join/cleanup diagnostics.
Normal completion checks source integrity and joins before success. Existing
length/mtime source assumptions apply; this is not a competing-writer CAS.

The bounded library reservations do not imply a universal RSS limit. Small fixed
standard thread/Arc allocations retain Rust's allocator-abort limitation; actual
VM/RSS includes allocator/runtime behavior outside payload accounting. Linux capped
functional diagnostics are measured separately. Automatic policy and general
speedup/independently oversized acceptance remain tasks 039 and 040.
