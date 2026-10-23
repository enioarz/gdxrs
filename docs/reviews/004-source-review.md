# Task 004 source-versus-fixture review

Reviewer: implementation author, separate second pass. This is **not independent M0 review** and cannot satisfy task 006. Source revision: `9e2a8c8932412010cd7d2f94727d1740143bf3da`; file digests and exact anchors are in the evidence ledger.

| Area | Review and disposition |
| --- | --- |
| Primitive order | Confirmed three size/pattern pairs, separate byte-order selectors, and eight-byte reversal for i64 through `order_integer`. Uniform little/big transformations match unchanged oracle semantics. Mixed primitive order is explicitly outside the initial profile. |
| Strings/header | Confirmed one-byte length and no on-disk NUL; only the C API uses terminated buffers. Header has magic, identifier, v7, compression, audit and producer before the index. No fixed audit length is assumed. |
| Major index | Confirmed **80 reserved bytes**, then final i32 marker + six i64 positions, not ten meaningful section pointers or an 88-byte structure. Next-write insertion point coincides with symbol-table start, not EOF. |
| Compression | Confirmed three-byte big-endian length header independent of primitive order; decoded 32 KiB blocks can split records. The damaged-checksum acceptance is upstream permissiveness; Rust rejection is normative. |
| Metadata | Compared every symbol field in save/read order, including error/text/compression/domain flags, comment counts, UEL order, zero-based text IDs and acronym triples. Node numbers are not serialized. Verified optional zero domain pointer and strict-versus-relaxed priority. |
| Records/counts | Confirmed raw header count -1, sorted-header counts and final stored count distinction. Empty scalar physical count 0 is projected to one public default. Checked all key-width cardinality thresholds and explicit/compact suffix transitions against annotated bytes. |
| Numeric behavior | Confirmed tags 0–10, default tables, five fields, acronym numeric representation, negative-zero/NaN/infinity writer normalization and preservation of ordinary returned finite bits. Invalid tags becoming zero upstream must not be copied. |
| Names and text | Resolved the case-folding contradiction through the `TXStrHashList` default and the raw-table transform. A second source pass also corrected the initial draft's description of overlong explanations: symbol overflow uses a diagnostic prefix, while comments truncate. Raw reader framing does not run writer normalization. |
| Writer/append | Confirmed ordered raw writes, rejected rows versus stored counts, mapped/string sorting, domain diagnostics, final offset backpatching and append semantics. No inference that old README prose disables implemented append. |
| Restart analysis | Traced `LastElem`, bounds, widths, field arity and scalar/universe/alias branches through `PrepareSymbolRead` and `DoRead`; traced decoded buffering separately. Natural record/block boundaries alone do not carry all state. Actual restart execution remains M5. |
| API inventory | Extracted 92 names only from `functions:`. Counted the six getter/setter entry points for three properties separately; callback declarations in later YAML sections are not additional unique functions. Every row has a milestone, native/adapter/external disposition and verification requirement. No implementation status is implied by a planned equivalent. |

All issues found in this pass were resolved in the specification. No M1-required field layout/tag/default question remains open for the documented supported profile. Runtime parser checks, >4 GiB execution, resource contracts and independent M0 review are distinct subsequent gates.
