# Two-pass string records and first-registration IDs (025)

Use `Writer::begin_string_records` before any symbol data is written. Register
symbols, domains, comments, set texts and acronyms first. Existing UELs are allowed.
Within the borrowing phase, `begin_symbol(id)` closes the previous input group;
`write(&[&[u8]], &[Value])` spools one row without retaining caller slices.
`finish` closes the last group, constructs the dictionary/remap and replays each
group into task 024. Explicit `Writer::finish` is still required for publication.
Empty groups and scalar defaults retain M3 behavior. Each symbol may occur once;
complete domain-set groups before dependent groups. Input may be unsorted within
each group. Metadata freezes at replay. Further raw writes of previously unwritten
symbols are allowed, but no further registration or string phase.

## Choice and evidence

Reuse the task-020 two-pass bulk algorithm rather than add an immutable-run
dictionary with another on-disk format, lookup cache and compaction policy.
The existing sorter orders exact `(label, occurrence)` tuples; the smallest
occurrence becomes canonical. The ordinal file resolves each request to that
canonical occurrence, then requests replay in encounter order, assigning IDs
only at their first occurrence. This is bounded O(N log N) external work, avoiding
the existing incremental registrar's repeated whole-journal name scans. Existing
UELs are seeded in original ID order before new requests; internal extension does
not change the public fresh-only bulk API. No complete token array, map or record
offset index is retained.

The generated qualification workload has 8192 distinct 63-byte labels and a
16 KiB run buffer, independently checks every metadata/record item through both
implementations, and records wall time, charged root/scratch peaks and handle
peaks. Tiny-run tests use 2048 labels and 512-byte buffers. These are functional
and resource measurements for choosing a known bounded implementation, not a
throughput comparison with an unimplemented immutable dictionary. Native eager
registration and M3's slower bulk registration measurements remain documented in
task 023. General-input OS/RSS/throughput acceptance stays task 031.

## Storage, admission and ownership

One additional checksummed Store in the metadata directory contains group-start,
row-value, individual label and group-end frames. Splitting labels into separate
<=63-byte frames keeps dimensions 0–20 and up to five typed fields within the
existing 1024-byte frame bound. No format version or spill-run format changes.
After ingestion, the dictionary iterator scans label frames; the mapping callback
appends fixed 19-byte ordinal/UEL frames to the same Store. Replay streams original
frames and mapping frames with two offsets, validating checksums, shapes, counts,
tags, extents, ordinals and registered IDs before external record sorting.
Prefetch at most 1024 mapping IDs (4 KiB, root-charged) to avoid switching the
shared page cache between distant offsets for every key. This is a fixed batch,
not an input-sized token array or a second dictionary cache.

The Store has one existing 32 KiB root-charged buffer. Admission reserves 16 KiB
for fixed controls/frames, including its small shared owner, and a 20-token key
buffer and the 4 KiB mapping batch. The Arc/Mutex control allocation is fixed and admitted but uses std's
infallible Arc constructor, like existing shared budget/spill controls; it does
not provide an allocator-OOM recovery guarantee. There is one local spool mutex,
never a global decoder lock. Locks are released before dictionary callbacks and
writer calls. No user callback executes under that mutex.

The spool uses the existing metadata/journal half of the aggregate scratch quota.
Its entire mapping tail is admitted before dictionary work, so live requests,
ordinal index, journal and spool cannot spend the same bytes twice. This is a
conservative reservation: statistics include the admitted mapping tail before all
of it is materialized. Sort runs use the existing other half. At least seven
configured scratch handles are required for this adapter: the existing three
metadata slots, the spool, and three merge handles. Deduct the spool slot from
the existing sort allowance; never raise the caller's cap. Other APIs retain
their six-handle minimum. Historical peaks survive all phases. Fan-in remains
conservatively reduced after the phase.

Metadata owns the shared Store as soon as creation succeeds. Parent failure
cleanup closes its file even if the borrowing job still holds a control reference,
then removes owned paths; this is necessary on Windows. Failed removal keeps
ownership for abort retry. Close every replay/merge cursor explicitly, preserving
secondary cleanup errors, and cancel an unconsumed dictionary sorter on ingestion
failure. The string adapter explicitly rejects replay decoding failures before
dropping its raw writer, preserving the primary error rather than replacing it
with abandonment. Failure or abandonment blocks publication. Forgetting a borrower
leaves the parent's active flag set, so it cannot publish; normal Rust forget
resource-leak limitations still apply.

## Semantics and limits

Labels use the core's exact-byte, case-sensitive policy and native-compatible
quotable <=63-byte validation, including non-UTF-8 bytes and empty labels. No
trimming, folding or UTF-8 conversion. Preexisting registration IDs come first,
then first encounters across symbol groups. Arity/label/count errors fail during
ingestion; duplicate keys, strict domains and typed references are revalidated
during replay and may fail at finish. All are terminal. No last-wins recovery.
Keep the typed value packet codec shared with 024, including ordinary numeric
bits, special identities and acronym references. The phase's label-occurrence
count plus preexisting UEL count is limited to i32::MAX by the reused bulk ordinal
format, even if distinct UELs would fit. This is an explicit adapter limit, not
a claim about the maximum count of key occurrences representable in GDX v7.

The pinned native implementation behaves differently when *authoring*: its
`TUELTable : TXStrHashListImpl<int>` uses the case-insensitive hash-list default;
`gdxDataWriteStr` looks up/adds labels through that table. Its `DoWrite` explicitly
encodes negative zero as vm_zero. The independent string fixture reproduces both:
native folds A into a and writes +0; Rust retains distinct IDs and -0. Native reads
Rust's exact IDs/bits correctly. Both author directions have explicit expected
streams for these source-backed differences; do not fold the Rust dictionary or
weaken its exact-bit expectations to match native authoring. Sources: pinned GDX
`9e2a8c8932412010cd7d2f94727d1740143bf3da`, `src/gxfile.cpp` gdxDataWriteStr/DoWrite,
`src/gxfile.hpp` TUELTable and `src/gdlib/strhash.hpp` template default/IndexOf.
The initial authoring experiment rejected the incorrect identical-stream
assumption; no existing golden stream, frozen hash or quota was changed.
