# Reader resource contract

Status: implemented M1/M2 accounting; [task 011](evidence/011.md) records the original reader tests and [task 016](evidence/016.md) records completed Linux scale qualification. Read together with [reader-contract.md](reader-contract.md) and rule F04.

## Accounting scope and minimum

One `Budget` belongs to the opened source. Its cap is the reader's `memory_budget_bytes` (default 64 MiB, minimum 1 MiB). All shared metadata, cursor allocations, output buffers, codec scratch and bounded diagnostics compete for this same cap. Cloning a handle does not create another unaccounted allowance. A budget uses safe atomic/locked quota bookkeeping; it is not a decoder mutex or global process quota. No unsafe allocation instrumentation is introduced into the production library.

Before allocation, a reservation acquires checked requested-layout bytes. Data-sized buffer allocation is fallible. Fixed standard `Arc`/`Box` controls use bounded charged storage but retain Rust allocator-OOM abort behavior, as documented in [decision 0003](decisions/0003-m1-advancement.md). The exact-capacity buffer helper is restricted to nonzero-sized elements and charges every vector's capacity times actual `size_of::<T>()`, outer vector headers, owned byte strings, codec state and persistent fixed cursor/header storage. Verify capacity/layout assumptions on supported toolchains; reject/free an unexpected larger capacity before using/publishing a buffer, and record it as allocation failure. Never use `reserve` growth or `collect` to bypass the helper. Hidden allocator block overhead is outside the logical-capacity accounting and belongs in RSS headroom; the cap is not an exact physical allocator/RSS ceiling.

Reservation ownership follows storage, not a temporary function scope. On allocation failure, read error, eviction, EOF cleanup or drop, release exactly once. Do not release a reservation while a buffer/cursor/Arc still retains its storage. A rejected reservation performs no oversized allocation. User-owned copies of borrowed views are outside the budget and must be explicit; borrowing itself cannot retain a previous internal batch across the next fill.

## Initial bounded working sets

| Owner | Bound / reservation rule |
| --- | --- |
| Shared file/header/options/diagnostic context | 16 KiB conservative fixed reservation; longer diagnostic paths are explicitly truncated, not retained without accounting |
| Temporary path conversion at open | Before calling `File::open`, reserve checked `2 * path.as_os_str().len() + 32 KiB` for platform path conversion/scratch; release after the open operation. Reject an insufficient budget before that call. This reservation is separate from retained diagnostic context. |
| Metadata page cache | Allowance at most `metadata_cache_bytes`, default 128 KiB; M1 retains zero persistent cache entries and streams bounded 32 KiB pages. A future page directory must fit the same allowance |
| Encoded block buffer per active decoder | At most 65,535 bytes from the u16 length, rounded reservation at most 64 KiB; no whole compressed section buffer |
| Decoded block buffer per active decoder | At most 32 KiB; bounded decoder output, including checksum validation |
| Codec scratch per active decoder | Reserve a measured ceiling before codec construction; initial design ceiling 128 KiB. The codec task must measure/confirm actual state and allocations or return to design review before changing this ceiling. No unmeasured codec may be called “accounted.” |
| Cursor keys/bounds/widths/state | Fixed arrays for 20 dimensions and five fields; conservative 16 KiB reservation including persistent parser/alias/error state |
| Current output batch | At most `batch_bytes` requested payload plus separately charged bounded column headers; actual element sizes/capacities drive the checked row capacity |
| Diagnostic details | Included in fixed state, at most 1 KiB message bytes; no record-error history or unlimited path/message chain |

An active metadata stream may need its own decoder while a symbol cursor exists. Reserve that decoder too; do not assume these buffers are shared when they coexist. Sequential metadata navigation can reuse an existing scratch set safely. M1 has no workers, merge runs, postings lists, index or persistent reverse map; do not allocate speculative storage for them.

A reservation's measured implementation can be smaller than a conservative table allowance, but must never exceed its documented maximum unnoticed. The codec task measured 10,504 bytes on the tested target; see decision 0004. That measurement is not retroactively a measured M0 fact.

## Worked examples

With a 1 MiB cap, 16 KiB source state, 128 KiB metadata cache, one decoder at 64+32+128 KiB, 16 KiB cursor state, and a 64 KiB batch consume at most 448 KiB before charged small column-header/layout overhead. There is headroom for another bounded metadata decoder, not an arbitrary full metadata table. If a retained cache/second cursor would exceed the root cap, evict reclaimable cache pages or return `BudgetTooSmall`; never silently raise the cap.

That scan example occurs after open, when temporary path-conversion scratch has been released. Do not charge only the truncated diagnostic name while a platform open call converts a much longer input path. OS/runtime/allocator hidden overhead remains part of measured RSS headroom, not a claim of exact physical allocation control.

For a two-key parameter, each output row costs `2 * size_of::<UelId>() + size_of::<Value>()`. Compute rather than assume the enum size. For a 20-key/five-field row, the same formula is `20 * size_of::<UelId>() + 5 * size_of::<Value>()`. With a 64 KiB batch, fewer wide rows fit; do not reserve `record_count * row_size`. On an empty projection, enforce the 65,536-row ceiling despite zero payload bytes. A batch cap smaller than one selected row is `BatchTooSmall`.

At a 64 MiB root cap, increasing a file from one million to one billion records changes work/time, not cursor working-set reservations. Increasing labels can increase metadata scanning/page misses, not an obligatory offset-vector/hash-map allocation. That is the target invariant to execute in M2; it is not demonstrated by these examples.

## Verification design

M1 tests a counting positioned-read adapter with scripted short reads, EOF, selected-offset I/O errors and byte-range counters. It must prove open/metadata lookup does not read unrelated record sections and first-batch/zero-limit behavior does not scan a whole symbol. A fault while filling a batch must expose no partial batch, become terminal, and release storage on drop. Test actual read ranges; an elapsed-time assertion is not a substitute.

Budget-helper tests exercise rejected reservations before allocation, checked multiplication/rounding, allocation failure, capacity accounting, cache eviction, cursor cloning/retention, and exactly-once release. A test-only safe accounting hook reports current/peak library-requested bytes. Document all allocation sites; include codec state/buffers and metadata. A counter covering only the output vectors is insufficient.

M2 adds independently oversized record and metadata sources under a fixed cap, streamed into a validating sink. Initial scale target: 256 MiB library cap under a 512 MiB OS process cap where supported, file/decoded data at least 4× the OS cap, and separate metadata larger than the cap. Measure peak RSS and tracked reservations independently, record allocator/runtime/stacks/page-cache treatment and leave headroom. Growing row/label counts must not produce proportional retained memory. Unsupported OS-cap methods are unrun checks, not simulated passes.

No memory mapping, unlimited queue or implicit full-file index can substitute for this accounting. Any M5/M6 sidecar/worker design must extend the table with index pages, queues, reorder buffers and per-worker scratch within the same root cap before implementation.

## M2 selections, task 012

Each immutable selection reserves 16 KiB for fixed twenty-dimension state and standard Arc control/source references (checked against the ceiling). ID payloads reserve original input capacity × `size_of::<UelId>()`; range unions reserve input capacity × `size_of::<UelRange>()`. Both use the fallible exact buffer helper; in-place unstable sorting/deduplication/coalescing creates no auxiliary heap buffer. Capacities remain charged after normalization. Empty sets/unions allocate no payload. Clones share ownership, and scan cursors retain their selection until drop. The root cap never grows to accommodate an oversized set; task 012 returns `BudgetTooSmall` and task 015 will supply a separately budgeted spill path.

Scan fixed state includes its selection reference and decoded/returned counters, with the existing 16 KiB audited ceiling. No output payload is allocated for rejected rows; the existing byte-cap/row ceiling applies to accepted rows. Counters report work, not speedup or a process-memory guarantee. Task 012 tests quota exhaustion, retained clones/cursors, filtered failed-fill cleanup and final-owner release to zero; M2 scale/RSS containment remains pending task 016.

## M2 small label lookup and pages, task 013

Lookup owns one contiguous byte buffer and an ordered descriptor array; a temporary ordinal-index buffer is sorted in place. Their combined checked capacities have a 128 KiB small-request ceiling and all compete for the root quota, plus a 16 KiB fixed-state allowance. Indices are released before returning results. Descriptor lengths/offsets are checked before copying; no file count controls result allocation. Constructing a label selection temporarily owns a charged ID buffer alongside the selection's owned copy. Large requests explicitly fail until spill support exists.

Each ID-to-label cursor has a conservative 16 KiB fixed allowance and at most two pages, each 128 bounded Bytes values plus its complete optional page/control/checkpoint header. A secondary atomic source-wide budget limits all resident page charges to metadata_cache_bytes; root buffers remain charged too. On the tested target each page is 32,848 logical bytes. Header storage is conservatively included in the root cursor allowance; page headers are also counted in the cache allowance. No compressed/decoded block survives page fill: temporary Stream/codec scratch is separately root-charged and dropped before returning a view. No entry/offset vector grows with UEL count.

LRU eviction drops that slot's labels/checkpoint and releases its cache/root payload. Exhausted optional cache quota falls back to uncached operation; required scratch/root exhaustion returns BudgetTooSmall, and allocation/read failures remain visible. Reader clones share the same cache cap; independent cursors never mutate another cursor's pages or use a global decoder lock. Cursor/results keep their original source and accounting alive until the last owner drops. Fixed standard shared-owner allocator-OOM behavior remains as recorded in decision 0003. Tests execute zero/tiny caches, concurrent retained cursors, eviction/restarts, terminal failure cleanup and complete root release; independently oversized metadata and OS-cap evidence remain task 016.

## M2 temporary run sorting, task 014

`Reader::spill_sort` uses the same root memory budget as metadata, selections and scans. Record capacity is floor(run_bytes / actual size_of::<Record>()); the fixed record holds bounded bytes and an opaque u64. Every run uses explicit fallible 32 KiB I/O buffers. The input buffer is released before merging; at most merge_fan_in reader slots/heads and their buffers coexist with one output writer. No run/path list grows with input: contiguous numeric ID ranges describe passes, and cleanup scans known names without collecting a directory.

Each sorter/cursor has one transferred 16 KiB control reservation; its session has a separate 16 KiB control allowance and path scratch of 64 KiB + checked 8 × parent encoded length. Parent lengths are at most 4096, retained path capacities at most 16 KiB. Fixed standard Arc/Path controls retain the safe-stdlib OOM limitation; numeric/file data controls no path allocation. Root quota rejects oversized configurations and concurrent working sets without raising limits.

Temporary bytes and run handles are explicitly **per-session** quotas, not process-global/source-wide disk/descriptor limits. Root memory is shared source-wide. A multi-sort integration must budget the sum of its job limits before implementation. Reserved logical bytes include input runs, pending buffered writes and output runs simultaneously, before any write/growth; handle reservations precede opens and follow File lifetime. Filesystem block/inode overhead, the source's existing GDX handle and transient OS path internals are outside those logical quotas. This is neither physical disk preallocation nor RSS containment.

Run completion requires buffer flush, file sync/close, exclusive hard-link publication and removal of the partial name. Inputs close and are removed only after their replacement completes. Check header/version, bounded label lengths, tuple order, footer count/checksum/trailer/EOF and the final accepted-input/returned count. Stats complete remains false for cancellation or any error. On failure, drop readers/writers/buffers, attempt every known owned cleanup and preserve both primary and secondary cleanup errors. Explicit cancel/close permits cleanup retry; Drop can only attempt best effort. Private scratch parents/namespaces must not be concurrently mutated by others; same-directory hard links are required. No crash-recovery or cryptographic authentication promise is made.


## M2 large exact lookup and observed domains, task 015

[Decision 0009](decisions/0009-large-labels-domains.md) specifies the implemented
streaming APIs, ordering, ordinals, source/ownership checks, composition of three
scratch quotas, shared root memory, fixed-frame disk membership and unique-domain
extraction. Empty/unknown labels contribute no IDs; disk predicates intersect base
constraints on original dimensions before exact accepted-row limits. Lookup output
is label-sorted with original request ordinals; domain output is deduplicated in
original dimension/ID order. No eager dictionary, offset vector or universe bitmap
is retained. Explicit close reports cleanup, while Drop is best effort. All results
remain partial after a later error. At most one disk clause per selection is
supported; membership requires a fresh lookup cursor. Logical bounds are not
physical RSS/disk containment, and binary membership probes do not imply GDX data
pruning or throughput gains. OS-capped scale qualification remains task 016.


## M2 scale qualification, task 016

[Executed qualification](evidence/016.md) validates 20 million wide records and a
separate 36-million-label dictionary under a hard 512 MiB Linux RLIMIT_AS limit
with a 256 MiB shared root quota. Logical root/scratch/handle counters are reported
separately from wait4 RSS and sampled /proc data. The negative allocation control
and each consumer's own limit check confirm actual containment. Fixed-cap input
growth retains constant tracked scan/metadata memory. The qualification does not
turn library accounting into an RSS guarantee for arbitrary consumer collection,
other platforms, processes outside the capped worker or kernel filesystem cache.


## M3 initial output transaction, task 018

The [writer contract](decisions/0012-writer-contract.md) defines the standalone or
reader-shared root budget. A fixed fallible 32 KiB buffer, audited 16 KiB controls
and bounded path allowance are charged before allocation/filesystem mutation.
Logical output high-water extent is admitted before buffering, and index patches
do not consume another file-size allowance; physical writes are separately counted.
Only one owned output handle/temporary file exists per writer. There is no metadata
spool yet. Explicit finish/abort close handles before owned cleanup; Drop never
publishes. Output quota is logical extent, not physical filesystem-block usage,
and no writer OS-capped scale or throughput claim exists yet.


## M3 bounded section encoding, task 019

The [encoder audit](decisions/0013-bounded-writer-codec.md) records the exact
miniz_oxide feature and fixed allocation paths. Zlib adds a fallible 32768-byte
input buffer, a fallible 32771-byte framed-output buffer, a fallible one-element
visible codec-state buffer (65712 bytes on qualified native targets), and a
conservative 320 KiB reservation for five private fixed heap objects. All compete
for the existing root and are admitted before constructing the codec or creating
an output file. The additional tracked encoder reservation is 458931 bytes on
those targets. Private fixed boxes retain the documented allocator-OOM abort
exception; data buffers are fallible and never grow. Raw mode allocates no extra
encoder buffers/state. This changes no dependency versions/counts.

Every nonempty <=32 KiB input block resets the dictionary and checksum. A zlib
attempt can fill at most raw-length minus one output bytes; otherwise stored
fallback copies to the same fixed framed buffer. Admit the entire header+payload
to the output sink together; a sink error makes the encoder terminal. End flushes
only a pending final block; repeated empty flushes emit nothing. Section boundaries
are flushed before recording the next physical offset, and compressed data is
never patched. Compressed empty GDX output uses stored blocks because its tiny
sections do not benefit from zlib; the separate full/boundary block probes exercise
actual zlib output. No record-writer RSS or throughput result is claimed.
