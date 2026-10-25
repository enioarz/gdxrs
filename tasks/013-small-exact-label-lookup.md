# 013 — Exact small-label lookup and bounded metadata access

Milestone: M2. Prerequisite: 012 complete. Status: complete; [evidence](../docs/evidence/013.md).

Resolve a budgeted small request set of raw byte labels with one streamed UEL-table traversal. Distinguish a/A and non-UTF-8, preserve file IDs/request ordinals, handle duplicates and unknowns explicitly, and compose unknown/empty results into zero-data-read selections. No eager UEL vector/reverse map. Define bounded ID-to-label paging/checkpoints and cache ownership, counting encoded/decoded page bytes and directory entries inside metadata_cache_bytes. Zero-cache operation remains correct. Bound symbol/catalog lookup state too.

Before implementation record concrete API/options, allocation lifetimes and cache eviction policy. Tests must compare exact bytes/IDs to authored fixtures, exercise block crossings, tiny/zero caches, simultaneous cursors, cache eviction/drop/failure, foreign handles, and metadata traversal counters. Large requests exceeding the small-request allowance fail explicitly until task 015's spill path exists. Complete with revision-bound evidence in docs/evidence/013.md and full affected checks. No sidecar index or dependency additions.
