# 0025 — General writer acceptance and optional bounded lookup pages

Task: [031](../../tasks/031-m4-acceptance.md). M4 qualification remains gated on
its actual large runs, native platform checks and review.

Small OS-capped pilots exposed excessive buffered I/O in sparse mapped binary
searches: the original single page thrashed, reading about 66 GB for 16,384 wide
rows. String remapping also alternated distant ordinal pages. This measured need
justifies an optional multi-page cache; the production dependency graph is unchanged.

`MetadataOptions` (the existing `SpillOptions` alias) accepts
`lookup_cache_bytes`, default zero, maximum 64 MiB per retained journal/index.
The spill sorter ignores this writer-only option. Zero keeps the previous
single-page footprint, including existing 1 MiB tests. Buffers and control slots
are charged to the same Budget before allocation; all retained caches compete
with record runs and decoder/encoder buffers. No cache can exceed its requested
additional ceiling, and the root remains the aggregate ceiling. Partial allocation
releases charges; owners retain their paths for ordinary terminal cleanup/retry.

The cache uses fixed-size pages and 16-way hashed sets with bounded control state.
Eviction explicitly writes dirty pages with short/interrupted-write handling;
failures retain cache ownership. Reopen invalidates pages. Close releases cache
charges. Appends flush dirty pages and invalidate old snapshots. Existing file
length/mtime checks, position-bound checksums and frame validation remain enabled.
Binary searches validate source snapshots before and after the whole search;
no user callback occurs inside that interval. Windows still physically reads each
ordinal frame, including within snapshot searches, because NTFS timestamps can
lag across callbacks. No cryptographic or competing-writer mutation guarantee is
introduced.

Bulk registration preserves each inherited raw ID even for duplicate labels.
Resolving an existing canonical label can use its proven original ID directly;
new canonical IDs still use the verified ordinal index. This avoids needless
random index reads without changing first-exact-match resolution.

Public unsorted/string/deferred statistics expose aggregate `Duration` phase
measurements for acceptance tooling. No per-row clocks or instrumentation
allocation are introduced. Complete merges are timed separately from final-run
replay/encoding. Fallible duration sums happen after cursor and symbol cleanup,
so overflow cannot bypass the established cleanup contract.

Handle statistics record aggregate peaks at each sort observation. They do not
combine a previous sort's peak with a later retained spool; that would report
handles that were never live together. Admission ceilings are unchanged.
