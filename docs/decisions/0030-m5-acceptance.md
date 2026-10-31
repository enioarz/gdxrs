# 0030 — Measured persistent slice acceptance

Task 035 follows merged task 034 PR #29 (`d34176d`). No dependency, parallel worker,
postings structure, serialized checkpoint format or GDX compatibility change is
introduced. The fixed version-1 source-derived sidecar proof remains mandatory.

## Leading-key planning correction

A preliminary 65,536-interval probe exposed a directory walk: a warm middle-leading
slice took ~1.29 s and 327,680 page reads versus ~0.016 s serial. Merely reducing
record decoding was not enough. The baseline production hashes, worker hash and
raw measurements are retained with the evidence, not qualified as M5 acceptance.

Strict canonical replay proves lexicographic raw-key order, so the first-axis
interval maxima and minima are monotone. The planner now binary-searches the first
interval whose maximum can contain the smallest requested leading ID. It stops
when a verified minimum proves the remaining suffix is above the largest requested
ID. AnyOf/range-union interior holes still use exact residual filtering and ordinary
conservative summaries. Non-leading axes never gain an ordering assumption.

When observed pruning favors sequential decoding, another bounded search finds
the end of the leading-key window. A sequential tail stops there rather than
reading an unrelated suffix. A tail ending at the source's final ordinal still
owes its terminator unless the caller's accepted limit stops first. All probes use
the existing authenticated directory paths and one-page cache. At most one current
span and one transient probe are materialized, both root charged; no candidates,
positions or directory pages are collected.

`intervals_considered` counts inspected summaries, `boundary_probes` identifies
ordering searches, and `intervals_skipped_by_leading_bounds` counts proven bulk
skips. Total `intervals_pruned` includes bulk skips and may exceed summaries
considered. Poor-pruning decisions use the sequential observations, excluding
binary probes/bulk skips. Source/access/admission failures remain errors; the
before-output-only fallback and sticky acknowledged progress contract is unchanged.

## Acceptance procedure

`python tools/dev.py m5-check --profile smoke|scale` uses non-forking, streaming
workers with one 256 MiB root, self-verified soft/hard 512 MiB Linux RLIMIT_AS caps,
wait4 RSS, sampled descriptors/I/O and explicit 12 GiB owned-index / two-handle /
1024-byte cache ceilings. Generated files are exclusive task artifacts. They are
retired only after full native validation, ordered serial/indexed comparisons,
source-unchanged verification, independent source/header SHA-256 and sidecar hashing.

Scale uses growing 18M/36M wide record inputs, 18M/36M 63-byte UEL dictionaries,
and 4.2M/8.4M one-record checkpoint sidecars. Largest records and metadata files
are physically/logically >4× the process cap. The largest sidecar input is also
physically/logically >4× the cap, with its directory alone >512 MiB independently
of checkpoint data. Directory stress is separate from source-record/metadata stress;
no assertion is inferred from just total index size or a count-only sink.

Every case has three fresh cold serial, explicit build and strict reopen trials;
the first indexed query follows each strict reopen, then three warm queries reuse
that session. Source digest and canonical replay costs are separate. Functional
serial/indexed matrices check complete all-row ordered digests, narrow leading,
non-leading, misses, skew, head/zero/exact limits and late positions. Complete native
records/UEL IDs/value bits remain the reference; huge native metadata verification
uses its explicitly separate 16 GiB cap because upstream owns its dictionary eagerly.
The Rust label stream and universe queries remain under 512 MiB.

Cold means fresh process plus per-file fsync/POSIX_FADV_DONTNEED before each trial;
it is advisory, with device caches uncontrolled. First-query data may be warm from
strict replay; that cost/state is disclosed rather than folded into a warm speedup.
Material narrow-slice improvement requires >=2× median warm latency and >=4× decoded
row/source byte/buffer reductions; total physical query work includes index-page
reads. Metadata universe queries have no encoded record payload, so their work
comparison is decoded rows/latency, separately from full dictionary streaming.
Build/reopen break-even, all timings/counters and skew/miss costs are reported.
Smoke exercises recipes and rejection guards but cannot qualify M5.

Reader/writer/metadata-page, M3/M4 and native four-platform gates remain enabled.
Linux memory/performance results are not macOS/Windows measurements. Completion
records findings and exact scope; it does not invent independent approval or publish
an M5 release automatically.
