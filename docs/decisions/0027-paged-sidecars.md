# 0027 — Source-verified, paged checkpoint sidecars

Task: [033](../../tasks/033-paged-checkpoint-sidecars.md), after merged PR #27.
This is an optional `index` feature; default/compression-only builds retain their
dependency graph. The new digest dependency audit is [0028](0028-index-digest-audit.md).
Task 034 still owns query planning and task 035 owns M5 scale/performance acceptance.

## Policies and trust

`Reader::index_builder(symbol, path, options)` returns an explicit owner before any
files are created. `finish`/`finish_with` performs one bounded serial record pass,
plus an independently timed full physical-source SHA-256 pass. No build occurs in
ordinary scans or `open_index`. `IndexPolicy::Off` does no sidecar work;
`UseIfValid` returns `IndexOpen` with Off/Missing/Rejected/Ready status. Source I/O,
source mutation, admission and access failures remain errors rather than silent
cache misses. Callers can choose serial scans after rejection before any output.

A source digest plus unkeyed index checksums does not authenticate attacker-authored
checkpoint tuples or pruning summaries. Strict open therefore hashes the entire
source once, then decodes the indexed symbol once to regenerate and compare every
canonical page and the header. This is deliberately more expensive than a raw
source digest alone. All schema/bounds/widths, positions, ordinals and extrema are
source-derived before a session becomes Ready. Metadata identity is included in the
full source hash and the canonical initial checkpoint. No trusted-cache shortcut
is implemented. This explicit open/validation cost must remain separate in task 035.

A validated session retains the regenerated root hash. Every later directory/data
page is verified through that root, so rewriting pages and recomputing their hashes
and on-disk header after validation cannot forge a new session root. A one-page
cache may retain old, valid bytes. Source immutability checks still apply; arbitrary
concurrent source edits with indistinguishable length/mtime are outside the existing
immutable-reader contract. This is not a lock, signature or competing-writer CAS.

No full-source digest or full directory pass is repeated by interval lookup.
Integrity/source/I/O errors invalidate the session and remain sticky; caller range
errors or root denial while materializing an interval can be retried. An error
never causes an implicit fallback after rows have escaped. Task 034 will integrate
exact residual filtering and before-output fallback into query planning.

## Format version 1

Sidecars are disposable, canonical little-endian files, independent of GDX byte
order. The GDX source is never edited. Pages are fixed at 1024 bytes, with the last
32 bytes holding SHA-256 of the preceding 992 bytes. All reserved bytes are zero.

- Header: `GDXSIDX1`, version 1, page size, full source length/SHA-256, interval
  count, root offset/hash/depth, dimensions, record and decoded-byte strides,
  canonical initial checkpoint, exact file extent/directory-page count and v7 ID.
- Data page: `GDXPAGE1`, kind/depth/count and half-open interval-index range;
  half-open raw public-row ordinals; conservative min/max for all 20 axes; a
  canonical 512-byte full decoder checkpoint. Empty intervals have zero extrema.
- Directory page: the same prefix and up to 16 fixed 56-byte children containing
  first/end interval indices, physical offset and child digest. Child pages are
  written before parents. Root descent is bounded by 16 levels and checks page
  bounds/alignment, depth, ranges and digest links. No in-memory root vector exists.

The complete checkpoint contains requested/resolved/schema/codec/order identity,
record count, data extent, preceding 20 keys, bounds/widths, original ordinal,
started/done state, and physical/decoded/logical stream position. Only the open
source's ephemeral ID is rebound after source verification. Raw serialized bytes
are not a public import API; `IndexInterval` exposes an owned charged checkpoint
only after the root proof succeeds.

An interval closes when either configured record or consumed decoded-byte stride
is reached; long first-key groups cannot block partitioning. Each accepted row
updates per-axis extrema. EOF is validated even when the last interval ended
exactly at a stride; an empty symbol has one empty interval. Alias/requested IDs,
universe and synthesized scalar defaults retain task 032 semantics.

## Resources and publication

Build uses one data file and at most 16 bounded pending directory buffers, each
holding 16 children. Full buffers stream to disk and propagate a single parent;
final partial levels flush the same way. Directory pages are interleaved with data
pages in the same file. File extent includes header, data and all directory levels
and is checked against `maximum_index_bytes` before writing. No second copy or
unaccounted directory spool exists. Source plus sidecar require two admitted handles.
The page cache/workspace is one charged page; larger configured ceilings do not
allocate additional pages in this version. Controls, path conversions, pending
levels, digest I/O and returned interval/checkpoint objects share the Reader root.

The builder creates an exclusive temporary directory in the canonical destination
parent. It retains ownership immediately after each successful create. Finish
backpatches the header, flushes/syncs/closes the file, checks source integrity, then
publishes with no-clobber hard link or explicitly selected atomic rename. Replace
refuses an existing target without sidecar magic, including GDX files; exclusive
ownership of the destination pathname is required against concurrent replacement.
There is no unlink-then-rename, in-place rewrite or fallback publication.

Primary/cleanup failures remain visible, finish is terminal on failure, and abort
retains/retries owned cleanup. After publication, cleanup failure reports published
but incomplete and never removes the published result. Drop is best-effort; explicit
abort reports failures. File sync/atomic visibility do not imply directory fsync,
power-loss recovery or filesystem crash durability. Directory/byte quotas describe
logical extents and admitted handles, not underlying filesystem block overhead.
