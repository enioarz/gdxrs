# Value and acronym API status (task 028)

Reference: GAMS-dev/gdx `9e2a8c8932412010cd7d2f94727d1740143bf3da`,
`src/gxfile.cpp`, `src/gdxapi.yaml`. `tools/values` is a separately hashed sidecar;
it reuses pinned helpers without modifying frozen generator sources.

| Native operation | Rust equivalent / explicit policy |
| --- | --- |
| GetSpecialValues / SetSpecialValues (gxfile.cpp 2370) | Fixed `SpecialValueMap`, per-Writer float adapter policy and per-reader ValueSession internal/read maps. Exact bits; duplicate write bits rejected atomically. |
| SetReadSpecialValues (3528) | Per-session read-only map. Duplicate sentinels require explicit AllowLoss. Original Value always retained. |
| ResetSpecialValues (683) | Reset internal/read special maps; preserve acronym mappings/counter. Writer reset affects future float inputs only. |
| MapValue | `WriteValuePolicy::decode_bits` returns typed classification; no global mutable float classification. Sentinel priority matches native. |
| Native IEEE write normalization (DoWrite 1323) | Explicit NormalizeNative option. Default preserves unmatched IEEE bits. Typed core writes bypass sentinels and preserve supported file bits. |
| AcronymValue / AcronymIndex (3305/3325) | `AcronymId::value`, `Value::Acronym` and existing checked file classification. Reject nonfinite canonical products instead of native overflow-to-infinity. |
| AcronymAdd / AcronymCount / AcronymGetInfo | Existing bounded writer registration and streamed reader catalog. New fixed-size `acronym_information(id)` performs explicit lookup without retaining a table. |
| AcronymName | Typed ID plus `acronym_information`; absent source entry is None, not a fabricated UnknownAcronymNNN string. |
| AcronymSetInfo, write (3042) | `Writer::update_acronym` changes pre-registered metadata by numeric ID before freeze. Streamed journal rewrite; collision checks, live-copy quota and cleanup ownership. |
| AcronymSetInfo, read / AcronymGetMapping (3086) | `set_acronym_mapping`, `acronym_mapping`, and per-row original/mapped/automatic provenance. Numeric IDs replace mutable ordinal call-state. |
| AcronymNextNr (3079), AcronymRemap (1468) | Explicit optional next ID; stable lazy allocation, checked finite encoding, original IDs retained. Bijections reject explicit/automatic target collisions. |
| MapAcronymsToNaN | Explicit ToNotAvailable; projected numeric value uses internal NA sentinel, preserving the original acronym. No automatic-ID allocation on this path. |

Native write maps compare five patterns as int64 bits; common numeric shortcuts
and IEEE normalization happen later. Read-only duplicates are allowed. Native
AcronymRemap chooses internal NA when the acronym-to-NA flag is set even if the
read-only NA projection differs. The sidecar tests these details against independent
complete expected streams, not only record counts or Rust round trips.

The native explicit mapping setter rejects a duplicate target, while lazy automatic
allocation can create one. Rust rejects both. Native integer-bound checks in
AcronymSetInfo/GetMapping use `N >= 1 || N <= size`, which does not validate the
range; the helper never invokes invalid native ordinal indexing. Rust uses checked
IDs and Option metadata queries. No unsafe native behavior is claimed as parity.

Exact exclusions: no C ABI/error-number/call-state emulation; no implicit unnamed
writer registration; names/text must be finalized before Writer metadata freeze;
no remapping an already assigned ID within one epoch (reset is explicit); no
synthetic source metadata mutation for discovered unknown IDs; no native ambiguous
automatic mappings or overflow-to-infinity IDs. Core typed scans, dictionary IDs,
wire format, native special tags and acronym file metadata remain unchanged.
General-input scale/RSS/throughput qualification remains task 031.

See [policy and ownership](decisions/0022-value-acronym-policies.md),
`examples/create_values.rs` and `examples/project_values.rs` for executable recipes.
