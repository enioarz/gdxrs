# M4 entry and bounded unsorted raw symbols (task 024)

M3's task gates and final four-platform CI passed; the user authorized M4.
Begin with task 024, keeping string/mapped adapters, recovery/error streams,
value/UEL policies, append and the M4 exit gate pending in tasks 025–031.

`Writer::begin_unsorted_symbol(id, bounds)` explicitly selects external sorting.
It borrows the same sorted symbol encoder/output transaction. MetadataOptions
already supplies the root-bound run buffer, private scratch parent, temporary-byte
quota and merge handle limits. Its existing half for journal state and half for
sort runs sum to the configured aggregate, including live merge inputs/outputs;
three metadata handle slots remain reserved. No independent hidden quota, record
spool or new spill format is needed. Report per-job SpillStatistics at completion
and conservative aggregate metadata peaks; registration IDs never change.

For a fixed symbol shape, 20 big-endian u32 key IDs occupy at most 80 bytes. Up to
five typed values need at most 65 more bytes (tag + u32 acronym ID + u64 bits).
The complete 145-byte record fits the existing 255-byte exact-byte spill label.
Equal-length key prefixes sort in raw-ID lexicographic order, independent of the
value suffix. Input ordinals are the existing opaque u64 tie breaker. Duplicate
raw keys are rejected by the sorted encoder, regardless of differing value
suffixes; no last-wins/stability promise is introduced. Scalars still accept at
most one row. Preserve ordinary bits, including negative zero/NaN payloads, and
special/acronym types. Decode scratch entries with exact shape/tag/length/ID checks.

Push validates arity, bounds and writer-owned registered keys/value references;
merge revalidates through the existing sorted writer. Strict domain sets must
complete first, including alias resolution; universe/relaxed rules remain M3's.
No output rows are retained in memory; fixed packets plus the root-charged run
buffer and bounded fan-in are the entire record working set. Registration freezes
when the borrowing symbol begins. Invalid/duplicate rows, sort/cursor/replay
failure or abandonment terminates publication. No recovery/error stream yet.

Record the nested sorter directory in the metadata owner immediately after its
successful creation, before fallible push/finish/replay. Close/drop sort handles
before the parent failure cleanup, preserving the primary and secondary errors.
If sort cleanup fails after a consuming merge, the parent retains its owned path
for abort retry. Never lose ownership by merely dropping an unreturned local.
Successful symbol finish requires sorted EOF integrity and scratch cleanup before
closing the symbol section. Forgotten borrowers still leave the parent active
flag set; Rust mem::forget may leak resources, but cannot authorize publication.

Task 024 qualifies correctness, resources and native functionality. General-input
larger-than-RAM/RSS/throughput claims remain the separate task-031 gate.
