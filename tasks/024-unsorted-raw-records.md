# 024 — Bounded unsorted raw records

Milestone: M4. Prerequisite: 023 complete. Status: in progress.

Expose `Writer::begin_unsorted_symbol` and a borrowing symbol writer accepting
registered writer-owned UEL IDs and typed values in arbitrary input order.
Reuse the M3 sorted encoder and existing spill primitive. Encode complete records
in fixed bounded scratch entries (20 big-endian u32 keys plus up to five typed
values fit the existing 255-byte sort key), then stream the validated merge into
the sorted writer. No full symbol, record-offset table, new spill format or new
dependency. Use the same root and explicitly configured MetadataOptions scratch
shares/handle limits; report per-job sorting statistics and aggregate metadata peaks.

Validate dimensions, bounds, registered IDs and typed references. Revalidate
merged records, reject duplicates by raw key regardless of values, preserve all
ordinary bits and special/acronym identity. Scalar multiplicity, empty/defaults,
alias/universe strict domains and completed-domain-set ordering retain M3 rules.
No last-wins or implicit data-error recovery. Any failure/abandonment prevents
publication; abort retains retryable ownership of nested sort storage, including
consumed-sorter merge/cleanup failures. Never lose cleanup errors or owned paths.

Force tiny runs, multiple merge passes and shared budget pressure; cover dimensions
0/1/2/20, all kinds/fields/subtypes, raw/zlib, equal keys/different values, foreign
IDs, corrupt scratch records, allocation/disk/handle/output quotas, failed
merge/replay and cancellation/cleanup retry. Compare complete native and Rust
streams to independent recipes, with native-write/Rust-read gates preserved.
Keep four-platform debug/optimized/feature CI and all M3 gates enabled. Record
source-bound 024 evidence. Larger-than-RAM general-input release stays task 031.
