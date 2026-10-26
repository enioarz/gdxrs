# M3 writer acceptance and measured serial changes (task 023)

Qualification keeps the established 256 MiB shared library root under an enforced
512 MiB RLIMIT_AS on Linux. Record counts grow from 13 to 52 million; metadata
from 9 to 34 million distinct 63-byte labels. The largest raw record output,
read -> write output and independently oversized UEL output must each exceed four
times that cap, as must their logical content. Raw/zlib record outputs and raw
one-root pipelines are validated completely by both readers, with ordered digests,
label bytes and original IDs. Growing-size RSS may increase by at most a fixed
64 MiB allowance; all Rust workers must remain within the original OS/root caps.
Smaller smoke cases never qualify M3. Large temporary artifacts are removed only
after full validation and SHA-256, preserving compact reports and provenance.

Record workers retain a fixed 65536-token caller dictionary (at most one MiB),
independent of row count, with an explicit upper bound on this tooling recipe.
Metadata workers retain no per-label token. This caller storage is fixed process
headroom, not relabeled as library accounting. The shared pipeline reads batches
and copies at most 20 keys/five values per row. It registers source labels by a
fallible streamed iterator, checks their original IDs, and shares the Reader root
with Writer. No full input/output, label dictionary or record collection is used.

Small before/after probes identified fragmented record emission and repeated
tiny journal I/O. A record now packs into a checked fixed 126-byte stack buffer
(the maximum 1+20*4+5*9) and emits once after the same validation. Each Store reuses its existing quota-charged 32 KiB append buffer as a
32756-byte read/write page after flushing appends;
its size is a multiple of 19, so ordinal frames do not straddle pages. Variable
frames may straddle and retain offset-bound checksums. Each logical frame checks
extent/modification metadata; detected changes invalidate a clean page and fail a
dirty page. Appends and page flushes maintain the local snapshot. The trusted
private namespace/immutable-external-source contract remains; timestamps are not
cryptographic identity. Cache capacity is constant, uses the existing fallible buffer and is included in
the root without raising the one-MiB creation/registration contract. An initial
separate cache allocation exceeded an existing compressed one-MiB fixture; buffer
reuse fixes that admission regression without changing its cap. Existing corruption, allocation, fault, quota and cleanup tests stay enabled;
new tests cover shuffled ordinal updates, dirty page eviction, cross-page frames
and external corruption of a cached page. No unsafe code or dependency is added.

Preliminary 100000-row probes improved raw encoding from ~59 ms to ~18 ms
(native ~18 ms); 100000-label total worker time decreased from ~2.1 s to ~0.56 s.
These are investigative single observations, not performance qualification.
The gate records three alternating native/Rust repetitions on one-million-row
raw/zlib encoding and 250000-label registration. Registration, encoding and finish
phases, dispersion, sizes, cache treatment and platform/compiler are separate.
Rust finish includes sync_all; native gdxClose has no equivalent durability promise.
Do not claim equal finalization policy or cold-cache throughput.

Native GDX eagerly owns metadata, so complete large-metadata reference validation
runs in a separate process with an explicit 16 GiB AS cap. Rust metadata workers
and streaming Rust validators still enforce 512 MiB. Native reference RSS/cap is
reported separately and never presented as bounded Rust containment. Record
native validation and benchmarks fit the ordinary 512 MiB process cap. Complete
reference counts/label bytes/IDs/digests remain mandatory. Python independently
calibrates the small C++ recipe before large runs; native and Rust full streams
must agree with the source-backed generator recipe. Native remains development-only.

Task completion still requires successful measured scale results, all correctness
and interoperability gates, native platform compatibility and documented findings.
No writer performance/RSS claim is established merely by adding this runner.
Append, general unsorted/string/mapped adapters and broader native API parity stay M4.
