# Sorted raw records (task 021)

Registration freezes before the first symbol begins. `Writer::begin_symbol` borrows
one output transaction and returns a `SymbolWriter`; explicit `finish` closes that
section. Dropping it, failing a row, or forgetting it prevents publication (the
parent retains an active flag). Each non-alias symbol may be written once, in any
order. Untouched symbols retain the empty/default behavior from task 020.

`KeyBounds` are inclusive, nonnegative i32 bounds per dimension. Omitting them
uses 0..i32::MAX and four-byte keys. No prescan or compressed backpatch is needed:
the data header count is -1; the final catalog count is authoritative. Keys are
writer-owned registered IDs, strictly increasing; scalars accept at most one row.
Values are typed, with exactly one or five fields. Ordinary bits are preserved,
including negative zero and IEEE payloads; reserved acronym encodings must use
the typed acronym variant and reference registered metadata.

Strict domains resolve aliases to sets for membership checks while retaining the
original alias ID in serialized metadata. Non-universe domain sets must be
explicitly completed before dependent records start. This makes forward metadata
references possible without buffering dependent data or speculative validation.
One-dimensional set members are appended to the existing checksummed journal as
fixed 19-byte frames, in sorted order. Completion frames retain their extent;
membership uses binary search over that extent. Storage shares task 020's scratch
quota and handle, rather than an unbounded in-memory domain table. Only domain
set keys are duplicated, not values or other symbol records. Empty domain sets
can be completed explicitly. Scratch failures remain terminal and retryable.

Symbol controls reserve 16 KiB from the shared root. Fixed arrays hold at most 20
keys/bounds and five values. The existing encoder handles block boundaries and
partial writes. Counts stop at i32::MAX before encoding another row. Set values
must be finite integral set-text IDs in the registered table. Variable subtypes
0..9 and equation subtypes 53..59 retain the existing metadata contract. No new dependencies.

Task 022 still owns the complete finalization/interoperability inventory; task
023 owns OS-capped pipeline qualification and throughput comparisons. Task 021
must independently compare nonempty output to native GDX and authored streams.
