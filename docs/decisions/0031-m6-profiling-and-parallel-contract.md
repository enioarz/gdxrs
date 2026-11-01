# 0031 — M6 baseline and bounded read concurrency

The user authorized M6 after PR #30 merged at `efbd808`. Task 035's source-bound
acceptance remains historical evidence for its exact revision. No independent
review or release is inferred from the new authorization. Task 036 implements
development profiling and this contract; production execution remains serial.

## Measure before implementing

`python tools/dev.py m6-profile --profile smoke|small` reuses the pinned native
oracle and M5's exact sink/build/open/session validation. Small uses 1M/4M raw,
native-zlib and metadata inputs and 131,072/524,288 one-record checkpoint sidecars.
Each case has three cold serial/build/strict-open trials, first indexed queries
and three warm queries per session. Native and Rust verify every original label
ID and ordered key/value bit. Metadata universe results are synthetic and separate
from the full dictionary stream. Input and sidecar hashes and source immutability
are checked before owned artifacts are retired.

Full record scans have an additional instrumented sink: Reader open/symbol lookup,
cursor construction, `next_batch` calls, verifying sink and cursor destruction are
disjoint wall-clock regions. Batch time includes I/O, decompression, row decoding
and column assembly. Sink time includes exact recipe checks and ordered digest;
total time also includes loop/timer/counter overhead. These are phase profiles,
not sampled CPU stacks or allocation attribution. Do not label a phase total as a
specific hot function or subtract the sink to advertise an unvalidated throughput.
Keep all raw observations, first-batch latency, CPU, RSS, source work and dispersion.

All Rust workers retain one 256 MiB root/enforced, self-checked soft/hard 512 MiB
Linux address-space cap. Index workers retain 12 GiB disk/two handles/1024-byte page
cache; the serial timing worker uses one source handle. Actual descriptor sampling
is bounded including stdio; short workers can escape samples, so the enforced
declarations and wait4 observations remain explicit. Native eager metadata uses
its separate 16 GiB reference cap. Cold uses advisory per-file eviction; device
caches are uncontrolled, and strict replay warms first-query data. Small is a
diagnostic baseline, not new independently oversized or parallel acceptance.

No speedup threshold determines whether a profile is retained. If warm execution
does not beat serial, break-even is absent. Failed or incomplete validation cannot
produce a completed baseline. `small` requires committed, unchanged measured
sources; smoke only tests the recipes and guards. Reports identify exact hashes,
compiler, hardware and cache state. Later optimizations require like-for-like
controlled comparisons; existing M4 string/mapped throughput limitations remain.

## Ordinals, ownership and trust (task 037)

Partition a requested symbol into half-open **raw public row ordinals** `[start,end)`.
Validate arithmetic and `start <= end <= verified_total`. Selection and projection
do not change ordinals. A range ends before `end`, even when its last decoded row
is rejected. A range reaching source EOF still validates pending framing unless an
accepted limit intentionally stops first. Empty/zero-limit ranges allocate no
worker/codec buffers or read records. Scalar defaults, empty symbols, universe and
aliases retain serial semantics; explicit serial handling is permitted where a
parallel partition adds no benefit.

Only the existing source-derived strict proof can supply persistent restart state.
An explicit validated index session belongs to the same immutable Reader/requested
symbol. Checkpoints are opaque, root charged and independently owned for worker
lifetimes. The coordinator keeps the index owner alive, obtains a bounded number
of verified starts online, and never collects every interval or task. Workers use
the complete restart tuple, checksum whole compressed restart blocks and validate
the preamble; no compressed block offset substitutes for a record boundary.
Ranges splitting a block may repeat bounded decompression; measure that cost.

## Shared admission and scheduling (task 038)

Use safe `std` threads and bounded communication initially, with no runtime
dependency change. Explicit options specify requested worker count, batch payload,
maximum in-flight tasks/result slots and per-thread stack allowance. Set the thread
stack size explicitly and reserve a conservative rounded allowance against the
same root **before spawning**; measure actual VM/RSS as well. Reserve coordinator,
task/channel capacity, checkpoints, codecs, output buffers, errors and reorder
slots too. Do not charge only row payload or give each worker a fresh root.

Admission must use checked arithmetic and actual available root capacity, reducing
concurrency only under the documented explicit policy. Report requested/admitted
workers, reserved stack/queue/reorder bytes and peaks. Failure to admit one worker
is actionable; unrelated source/corruption/allocation failures cannot be disguised
as a serial fallback. OS/runtime memory outside the allocator also remains subject
to the enforced process cap. Native CI must verify platform-specific thread-stack
behavior; Linux results do not establish Windows/macOS containment.

Assign monotonically increasing partition IDs. Only a fixed number of partitions
can be active or queued. Each has bounded result slots; the coordinator returns
batches in partition/record order. A slow first partition must stop later workers
through backpressure instead of expanding a reorder map. Never retain a whole
partition or result. Batch storage remains charged while queued, borrowed or
recycled. Before every transfer/blocking operation, observe cancellation; cancellation
must wake senders and receivers without depending on the consumer draining output.

The coordinator applies **global accepted** row limits after exact residuals, then
projection. Local raw range ends and worker limits cannot stand in for the global
limit. At the limit, truncate only an unpublished batch if needed, acknowledge it
once, cancel unnecessary tasks and join workers. Bounded speculative decode is
counted in statistics. Do not implicitly build/open indexes or prescan a whole
symbol to optimize `head`. An explicit short-limit serial path may avoid spawn.

## Failure, cancellation and observable progress

The first observed fatal worker/coordinator error stops new acknowledgments and
admission. Signal cancellation, unblock communication, join all workers, release
owned buffers/reservations, and retain the primary error with secondary cleanup/
join diagnostics. Successfully acknowledged rows form an exact ordered prefix;
its length may depend on when a concurrent failure is observed. No buffered batch
is acknowledged after a known fatal error. Failure remains sticky. Never restart
a failed scan or replay acknowledged rows through a serial fallback.

Expose fallible explicit close/cancel that reports joins and cleanup. `Drop` must
request cancellation and join, and cannot imply successful completion; diagnostics
requiring a returned error belong to explicit close. Failed construction must
retain ownership until already-started workers are canceled/joined. Handle panics
at join as terminal errors rather than detaching workers. Synchronous regular-file
I/O is not forcibly interruptible, so cancellation cannot promise a wall-clock
deadline while an OS read is blocked; wakeups/joins are still mandatory once it returns.

Workers and the coordinator keep serial source-change checks and the immutable
length/mtime assumption. Do not infer competing-writer CAS or indistinguishable-
edit detection. Per-cursor decoded/accepted counts remain local and exact. Concurrent
global source I/O deltas overlap, so do not sum them as independent worker work;
define aggregate measurement and external same-Reader interference explicitly.

## Gates and sequencing

Task 037 proves serial ordinal ranges first. Task 038 adds explicit ordered workers
and adversarial cancellation/backpressure tests. Task 039 may choose among already
available explicit serial/indexed/parallel policies only after real cost comparisons.
Task 040 retains growing out-of-core, native interoperability, all prior milestone
and four-platform gates. See the numbered specifications for exact requirements.

Parallel compression, alternate codecs, SIMD/unsafe work, postings and writer/UEL
state parallelism require separate measured tasks and invariant reviews. Polars
integration remains in its separate repository. No thread, planner heuristic or
future-task skeleton is added to production by task 036.
