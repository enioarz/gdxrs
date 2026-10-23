# Reader resource contract

Status: chosen M1 design, not an executed resource test. Read together with [reader-contract.md](reader-contract.md) and rule F04.

## Accounting scope and minimum

One `Budget` belongs to the opened source. Its cap is the reader's `memory_budget_bytes` (default 64 MiB, minimum 1 MiB). All shared metadata, cursor allocations, output buffers, codec scratch and bounded diagnostics compete for this same cap. Cloning a handle does not create another unaccounted allowance. A budget uses safe atomic/locked quota bookkeeping; it is not a decoder mutex or global process quota. No unsafe allocation instrumentation is introduced into the production library.

Before allocation, a reservation acquires checked requested-layout bytes. Allocation is fallible. The exact-capacity buffer helper is restricted to nonzero-sized elements and charges every vector's capacity times actual `size_of::<T>()`, outer vector headers, owned byte strings, codec state and persistent fixed cursor/header storage. Verify capacity/layout assumptions on supported toolchains; reject/free an unexpected larger capacity before using/publishing a buffer, and record it as allocation failure. Never use `reserve` growth or `collect` to bypass the helper. Hidden allocator block overhead is outside the logical-capacity accounting and belongs in RSS headroom; the cap is not an exact physical allocator/RSS ceiling.

Reservation ownership follows storage, not a temporary function scope. On allocation failure, read error, eviction, EOF cleanup or drop, release exactly once. Do not release a reservation while a buffer/cursor/Arc still retains its storage. A rejected reservation performs no oversized allocation. User-owned copies of borrowed views are outside the budget and must be explicit; borrowing itself cannot retain a previous internal batch across the next fill.

## Initial bounded working sets

| Owner | Bound / reservation rule |
| --- | --- |
| Shared file/header/options/diagnostic context | 16 KiB conservative fixed reservation; longer diagnostic paths are explicitly truncated, not retained without accounting |
| Metadata page cache | At most `metadata_cache_bytes`, default 128 KiB; 32 KiB decoded pages, bounded page-directory entries included in the same allowance |
| Encoded block buffer per active decoder | At most 65,535 bytes from the u16 length, rounded reservation at most 64 KiB; no whole compressed section buffer |
| Decoded block buffer per active decoder | At most 32 KiB; bounded decoder output, including checksum validation |
| Codec scratch per active decoder | Reserve a measured ceiling before codec construction; initial design ceiling 128 KiB. The codec task must measure/confirm actual state and allocations or return to design review before changing this ceiling. No unmeasured codec may be called “accounted.” |
| Cursor keys/bounds/widths/state | Fixed arrays for 20 dimensions and five fields; conservative 16 KiB reservation including persistent parser/alias/error state |
| Current output batch | At most `batch_bytes` requested payload plus separately charged bounded column headers; actual element sizes/capacities drive the checked row capacity |
| Diagnostic details | Included in fixed state, at most 1 KiB message bytes; no record-error history or unlimited path/message chain |

An active metadata stream may need its own decoder while a symbol cursor exists. Reserve that decoder too; do not assume these buffers are shared when they coexist. Sequential metadata navigation can reuse an existing scratch set safely. M1 has no workers, merge runs, postings lists, index or persistent reverse map; do not allocate speculative storage for them.

A reservation's measured implementation can be smaller than a conservative table allowance, but must never exceed its documented maximum unnoticed. The codec ceiling is a design precondition to verify in the codec task, not a measured M0 fact.

## Worked examples

With a 1 MiB cap, 16 KiB source state, 128 KiB metadata cache, one decoder at 64+32+128 KiB, 16 KiB cursor state, and a 64 KiB batch consume at most 448 KiB before charged small column-header/layout overhead. There is headroom for another bounded metadata decoder, not an arbitrary full metadata table. If a retained cache/second cursor would exceed the root cap, evict reclaimable cache pages or return `BudgetTooSmall`; never silently raise the cap.

For a two-key parameter, each output row costs `2 * size_of::<UelId>() + size_of::<Value>()`. Compute rather than assume the enum size. For a 20-key/five-field row, the same formula is `20 * size_of::<UelId>() + 5 * size_of::<Value>()`. With a 64 KiB batch, fewer wide rows fit; do not reserve `record_count * row_size`. On an empty projection, enforce the 65,536-row ceiling despite zero payload bytes. A batch cap smaller than one selected row is `BatchTooSmall`.

At a 64 MiB root cap, increasing a file from one million to one billion records changes work/time, not cursor working-set reservations. Increasing labels can increase metadata scanning/page misses, not an obligatory offset-vector/hash-map allocation. That is the target invariant to execute in M2; it is not demonstrated by these examples.

## Verification design

M1 tests a counting positioned-read adapter with scripted short reads, EOF, selected-offset I/O errors and byte-range counters. It must prove open/metadata lookup does not read unrelated record sections and first-batch/zero-limit behavior does not scan a whole symbol. A fault while filling a batch must expose no partial batch, become terminal, and release storage on drop. Test actual read ranges; an elapsed-time assertion is not a substitute.

Budget-helper tests exercise rejected reservations before allocation, checked multiplication/rounding, allocation failure, capacity accounting, cache eviction, cursor cloning/retention, and exactly-once release. A test-only safe accounting hook reports current/peak library-requested bytes. Document all allocation sites; include codec state/buffers and metadata. A counter covering only the output vectors is insufficient.

M2 adds independently oversized record and metadata sources under a fixed cap, streamed into a validating sink. Initial scale target: 256 MiB library cap under a 512 MiB OS process cap where supported, file/decoded data at least 4× the OS cap, and separate metadata larger than the cap. Measure peak RSS and tracked reservations independently, record allocator/runtime/stacks/page-cache treatment and leave headroom. Growing row/label counts must not produce proportional retained memory. Unsupported OS-cap methods are unrun checks, not simulated passes.

No memory mapping, unlimited queue or implicit full-file index can substitute for this accounting. Any M5/M6 sidecar/worker design must extend the table with index pages, queues, reorder buffers and per-worker scratch within the same root cap before implementation.
