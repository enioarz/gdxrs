# Session-local sparse UEL maps (026)

`MappedUelId` is a distinct caller-ID type accepting 0..=i32::MAX. On-disk raw
`UelId` remains one-based. Caller maps are session state; GDX files retain only
original raw registration IDs and labels. Never persist a map in GDX metadata or
silently substitute raw IDs when a caller mapping is missing.

## Ownership and registration

`Writer::register_mapped_uels` accepts a fallible iterator of `(MappedUelId,label)`.
It preserves preexisting raw IDs and assigns new exact-byte labels in encounter
order using task 025's bulk dictionary/remap. One immutable map phase is allowed
before metadata freezes; other metadata registration can continue afterward.
`raw_id_for_mapped` and `mapped_id_for_raw` expose explicit translations without
changing the raw APIs. `begin_mapped_symbol` borrows the writer; its rows translate
caller keys and use task 024's external sort back to raw-ID order. Shape, values,
strict domains, duplicates, abandonment and publication retain M3/024 rules.

`Reader::mapped_uels(options)` first returns an empty source-bound owner.
`MappedUels::register` then consumes explicit `(raw ID,caller ID)` pairs. This
two-step construction retains retryable cleanup ownership even if journal/index
allocation or registration fails. No input-sized map is required in caller RAM.
`mapped(raw)` and `raw(mapped)` return `Option`s for absent assignments. One map
cannot cross reader sources, even when they share a budget or equivalent bytes.
Its source Arc and root remain pinned. Explicit `close`/`abort` reports cleanup
failures and can retry removal; a closed/failed map cannot register or reopen.

Assignments are sparse immutable bijections. Repeating an identical pair within
the single registration phase is idempotent. Conflicting raw assignments or two
raw IDs sharing one caller ID are terminal errors, never inverse overwrites.
Negative/out-of-range caller IDs fail before any registration. A sparse ID near
i32::MAX costs one entry, not a dense array sized by that number. Raw IDs must
exist in the source/writer dictionary. Labels follow the core's exact-byte policy,
including its documented difference from native case-insensitive registration.

## Mapped reads and partial results

`Reader::scan_mapped` returns `MappedCursor`; each borrowed `MappedRecord` exposes
mapped keys, original raw keys and all typed value fields. The operation explicitly
prescans the raw symbol, externally sorts complete rows by caller keys, and then
streams one bounded row per `next`. It currently sorts even monotone/identity maps.
Raw scans retain their existing streaming behavior; no mapped fast-path or native
throughput parity is claimed. Public projection/filter extensions are not added
here. `batch_bytes` bounds the raw prescan batch under the reader's existing cap.
`limit` counts returned records **after** mapped ordering; limit zero validates
source/handle/options and does not decode the symbol. A positive short limit still
requires the prescan. Close cancels an unfinished result and permits another scan
with the map if cleanup succeeded.

The default `MissingMapping::Error` is terminal before output escapes from the
prescan. Explicit `SkipRecord` discards a row if any key is unmapped and counts
it in `MappedScanStatistics::skipped`; it never drops an axis or renumbers keys.
Decoded, accepted, skipped, returned and spill counters are separate. Merge,
scratch, lookup or source-change failures during output may follow returned rows;
subsequent `next` calls repeat the error and never replay those rows. A failed
scan invalidates the map, preserving ownership for abort retry. Close/cancel is
explicit and secondary cursor cleanup errors survive even when parent cleanup
succeeds. Drop is best effort, as with existing cursors.

## Disk, memory and handles

Both directions share one fixed-frame index in the existing checksummed Store.
Sort `(direction,key)` with an opaque target through the existing spill format;
collapse identical pairs and reject conflicts before marking the index ready.
Raw-to-caller pairs precede caller-to-raw pairs, each as an existing 19-byte ordinal
frame. Lookup binary-searches checked frames with O(log N) bounded reads and one
32 KiB page, independent of the largest caller ID. It reuses Store::integer,
including physical verified reads on Windows to avoid the known NTFS timestamp
cache issue. No new index/run format, unsafe code or dependency.

Writer registration stages caller labels and raw-ID remaps, then replaces that
spool with the final sparse table after its handles close. Its 1024-ID input batch
is 4 KiB and root-charged. Retained index bytes, registration requests/ordinals,
journal and any string spool share the existing metadata quota half. Live sorter
runs share the other half; no phase creates an additional implicit quota. All
buffers/control allowances use the same Reader/Writer root. Controls are fixed
and admitted; existing std Arc/path control allocations retain their documented
infallible allocator limitation. Statistics conservatively add share peaks.

Reserve one index handle from the existing sort allowance. At least seven
configured scratch handles are required when the map is the only additional
retained store; combining it with a live string spool needs another available
slot. Existing six-handle raw APIs keep their contract. Sorted read rows contain
at most 80 mapped-key bytes, 80 raw-key bytes and 65 typed-value bytes: 225 bytes
fits the existing 255-byte spill key. Values reuse task 024's typed packet codec.
Quotas include all live merge inputs/outputs. Metadata owns nested sort paths
immediately; cancel unconsumed sorts and explicitly close cursors before parent
cleanup. Table replacement closes its input handle before deletion on Windows.

Reader registration supports at most i32::MAX distinct raw entries; writer label
occurrences plus preexisting UELs retain task 025's ordinal limit. Raw row count
and dimensional limits remain v7's. General-input OS/RSS/throughput acceptance
is task 031; these bounded implementations and small qualification measurements
do not qualify mapped larger-than-RAM performance.

## Pinned native behavior and deliberate differences

GDX `9e2a8c8932412010cd7d2f94727d1740143bf3da`, gxfile.cpp
`gdxUELRegisterMap` / `TUELTable::AddUsrIndxNew`, accepts zero and identical
assignments. A duplicate caller ID for a different raw label overwrites the
inverse mapping while retaining both forward mappings. Rust rejects this
ambiguity so original raw identity remains meaningful. Native `gdxDataReadMapStart`
uses DOMC_STRICT, filters missing mappings into its data-error list, and sorts
nonmonotone mappings. Rust's explicit SkipRecord matches that filtering/order;
its default Error is stricter. General error streams remain task 027.

The isolated negative-ID native probe reproducibly aborts on Linux with
"double free or corruption (out)". gxfile.cpp `TIntegerMapping::SetMapping` writes
`PMap[F]` without checking F >= 0, unlike GetMapping. This is a reference-library
limitation, not Rust behavior to reproduce and not a native negative-case parity
pass. The development probe runs in a separate process with 256 MiB AS, 2-second
CPU, 1 MiB output, zero core and a 5-second wall timeout. Its observed result and
stderr are retained; a clean native rejection is also accepted on platforms where
the invalid write does not trigger an allocator abort. Rust unit tests require
safe rejection before registration for negatives and IDs beyond i32::MAX.
Huge sparse IDs are not passed into the native dense allocator. Source-backed
native collision and zero/idempotence probes run separately and validate the
resulting raw record's identity. Native registration also retains the case-folding
behavior documented by task 025; mapped qualification uses unambiguous labels.
