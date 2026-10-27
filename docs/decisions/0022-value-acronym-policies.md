# 0022 — Explicit numeric projection and acronym sessions

Task 028 adds opt-in numeric adapters while leaving typed file decoding and typed
writes unchanged. No dependency, unsafe code, wire format or frozen fixture
generator changes are needed. The native inventory and exact differences are in
[value/acronym API status](../value-acronym-api.md).

## Special values and IEEE identity

`SpecialValueMap` holds exactly five u64 bit patterns in UNDEF, NA, +INF, -INF,
EPS order. Defaults are native's 1e300 through 5e300. Construction rejects duplicate
bits, matching native write validation; +0/-0 and different NaN payloads are
distinct. Custom sentinels take priority over all other classification, including
IEEE nonfinite values and the reserved acronym range, as native write does.

A Writer owns a fixed `WriteValuePolicy`. Its new `write_f64` adapter methods use
that session policy on sorted, unsorted, mapped, string and deferred records.
Deferred raw/mapped/string float variants share it. Typed `write(Value)` never
consults the map. The default float adapter preserves unmatched IEEE bits;
`IeeeWritePolicy::NormalizeNative` explicitly selects native's NaN-to-NA,
infinity-to-special and negative-zero-to-positive-zero normalization. Changing
or resetting a policy affects future float inputs only. Invalid sentinel settings,
conversion errors and invalid value references terminate the file transaction.
External sorters cancel before parent cleanup even on conversion failure.

A Reader creates an explicit `ValueSession`, separate from its scans and from other
sessions. `set_special_values` changes both internal and read sentinels;
`set_read_special_values` changes read projections only. Duplicate read sentinels
require `ReadCollisions::AllowLoss`, matching native's allowed lossy read map.
Reset restores default special maps without changing acronym mappings or next ID.

`ProjectedValue` always retains the original `Value`, including IEEE bits, tagged
special class, acronym ID and encoded bits. `to_f64(Exact)` permits unchanged
ordinary numbers only; typed identity cannot fit in one floating-point number.
`to_f64(AllowLoss)` explicitly selects the numeric projection. Ordinary values
that share sentinel bits remain ordinary in typed scans and in the original half
of a projection. Maps never change filtering, counts, defaults, or the raw decoder.

## Acronyms

`AcronymId::value()` creates checked canonical encoding. GDX's positive finite
range at or above 1e301 is reserved for acronym IDs, as already classified by the
core. Multiplication can overflow f64 well below i32::MAX: canonical ID 17976931
is finite, 17976932 is not. Rust reports an error; native AcronymValue can return
infinity. No overflow becomes an implicit infinity/special value.

Read-session mappings are immutable bijections within an explicit epoch.
`set_acronym_mapping` installs an original-to-caller pair; repeats are idempotent.
Absent pairs preserve identity when next ID is None, or lazily take/increment the
configured next ID. Encountered identity mappings are recorded too so later
assignments cannot alias their outputs. Every assignment validates finite encoding
and inverse uniqueness before append. Colliding automatic IDs fail instead of
silently assigning native's ambiguous duplicate target. The counter does not skip
occupied IDs. Repeated occurrences reuse a mapping and do not advance the counter.

`AcronymReadPolicy::ToNotAvailable` explicitly discards numeric acronym identity
in the projected half only, without allocating an automatic mapping or advancing
the counter. It uses the **internal** NA sentinel, not a read-only NA override;
this matches pinned AcronymRemap/MapAcronymsToNaN. The original acronym stays intact.
`reset_acronyms` begins a fresh mapping epoch, resets next ID and restores Preserve;
prior results remain unchanged and are never replayed implicitly.

`acronym_information` scans source metadata with bounded cursors and returns a
fixed-size name/text record or None for an unnamed/absent source entry. It does
not synthesize entries into the source catalog. Duplicate matching IDs are errors.
Writer `update_acronym` changes a registered name/text before metadata freeze,
retaining numeric ID and registration order. It checks name/symbol collisions,
streams a replacement journal under the existing live-copy quota, syncs it,
closes both handles before rename and reopens the owned journal. Original and
replacement extents are charged simultaneously until replacement succeeds.
Failure is terminal, prevents publication and retains retryable cleanup ownership.
Final names must be declared before data writing; native late call-state mutation
and implicit unnamed writer entries are not emulated.

## Bounds, failure and cost

A read session reuses the existing Metadata directory/journal owner. The owner is
returned before fallible journal allocation; failed initialization retains retry
ownership and secondary cleanup errors. A fixed 16 KiB session/control allowance,
32 KiB journal buffer, metadata controls/paths and temporary metadata cursor buffers
all share the Reader root. The mapping journal stores fixed checked frames, not a
RAM map or a full acronym table. Only encountered/explicit mappings are retained.
Information lookup streams source metadata. Session errors are sticky; explicit
close/abort retries cleanup. Drop is best effort through the existing owner.

Each explicitly created ValueSession is a separate scratch job with its own
MetadataOptions; concurrent jobs share memory, not an implicit aggregate disk cap.
The reused allocator requires six configured handles and reserves the existing
journal/sort quota halves; this policy uses one live journal handle and only the
journal half. Writer metadata updates use the Writer's existing root, journal
share and reserved auxiliary slot. There is no extra writer scratch allowance.

Epoch reset does not reclaim old journal bytes; they remain charged until close.
Callers can close and create a new session to reclaim them. Journal mapping searches
are linear and many assignments may be quadratic in encountered acronyms; metadata
information lookup is a streamed scan, and each writer rename rewrites the journal.
These are bounded, explicit operations, not throughput parity claims. Task 031
retains general-input OS/RSS/performance acceptance. Caller-retained projected
values and metadata results use caller memory; no library collection is implicit.
