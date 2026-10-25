# 0010 — M2 scale qualification protocol (task 016)

The gate retains the specified 256 MiB root budget and an enforced 512 MiB
process cap. On Linux use hard/soft RLIMIT_AS, verify the limit inside consumers,
measure wait4 peak RSS/CPU and sample /proc RSS/VM/FD/I/O. A child allocation
negative control must fail beyond the cap. Address-space containment is stronger
than an RSS ceiling for these non-forking, unmapped streaming consumers; this is
not a process-tree cgroup or filesystem-cache cap. Record unavailable platforms.

Development-only C++ streaming generation emits the documented v7 layout with
fixed bounded buffers; production remains safe Rust with unchanged dependencies.
Record-heavy inputs have 20 original dimensions and five ordinary value fields,
explicit key framing, deterministic values and strictly ordered keys. The large
case has physical file and decoded key/value output >=4 times the cap. Separate
metadata-heavy inputs have negligible records and >=4 times the cap in physically
stored 63-byte unique labels. Smaller cases demonstrate growth under the same cap.
No sparse padding or repeated-file concatenation counts toward the ratios.

Generator output includes independent expected ordered digests/counts. Rust sinks
validate every key/value/label against the recipe and compare complete digests.
Small generated files must also match the pinned native reader. Native record
scans validate equivalent full decoded fields and digest under the same cap;
metadata-heavy native loading may be infeasible and must be reported honestly.
Labels and requests are streamed, including duplicate and unknown requests, exact
byte ordering, source-bound disk membership and observed-domain enumeration.

Fresh-process first batches have no library cache/index; OS page cache state is
reported separately. Linux POSIX_FADV_DONTNEED is advisory, not proof of a cold
physical device cache. Measure full scans, leading/non-leading slices, exact
limits/zero, metadata streaming/pages, large request lookup, multi-pass spill,
cleanup/cancellation and growing input sizes. Repeated full scans establish
median/range/stdev; one-off functional scenarios are explicitly not dispersion
samples. Counts/digests must pass before timing can be reported. Logical root,
scratch and handle counters do not substitute for observed RSS/OS limits.

The runner fails closed on missing containment/measurement, wrong digests/counts,
insufficient scale ratios, leaked scratch, excessive RSS or failed regression
checks. A smoke profile exercises tooling but never qualifies M2. Scale reports
bind source/compiler/generator hashes, inputs/SHA256, seed, hardware, cache policy,
limits, individual observations and limitations. No writer/index/parallel claim.

Before full-scale runs, fix growth bounds: doubling input may add at most 32 MiB
peak RSS and 1 MiB tracked root for the same streaming mode. These leave runtime
noise headroom while rejecting a dictionary/record-proportional retained buffer.
Full scans of each largest workload and the largest native record case run three
times; smaller and functional scenarios run once. Full scale additionally requires
fresh reader/M0/label-page regression reports at the measured committed revision.
The native reader checks the small metadata recipe too; oversized native metadata
loading remains an explicitly unrun comparison.


The large metadata sort has 36,000,000 × 73-byte run frames (2.628 GB).
Its final merge needs inputs plus output, about 5.256 GB in one session. Set
18 GiB total temporary logical quota, split into 6 GiB shares. This leaves required
merge headroom without changing the 256/512 MiB memory limits. Per-job statistics
and actual workspace free space remain separate. Review caught the initial
10 GiB total allowance as insufficient before the large metadata merge ran.

A first full attempt completed both record sizes but hit Linux PermissionDenied
while sampling /proc on an exiting metadata generator. This is a harness race,
not a passed scale gate. Count optional sampling permission denials in each trial;
mandatory wait4 peak RSS/CPU, child hard/soft limit checks, allocation negative
control and complete library work counters remain unchanged. Missing required
containment/measurements or wrong results still fail. wait4 high-water RSS can
include the Python fork/pre-exec footprint; report sampled post-exec peaks too.
Polling adds up to one 50 ms interval to external wall timing; use worker-reported
first-batch timings for latency rather than that external polling duration.
