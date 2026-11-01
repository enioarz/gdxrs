# 0032 — Exact serial ordinal ranges

Task 037 follows merged PR #31 (`58af12b`). It adds serial range cursors only;
no worker, automatic index operation, dependency or serialized format changes.

## API and ownership

`Reader::scan_range(symbol, checkpoint, end, options)` scans the half-open raw
public row interval `[checkpoint.row_ordinal(), end)`. The selected variant is
`Reader::scan_selected_range(symbol, selection, checkpoint, end, options)`.
`end` is an absolute ordinal, not a length or accepted-row limit. There is no
arbitrary start argument and no hidden prefix traversal to obtain a restart.

The checkpoint must belong to the same immutable Reader/requested symbol,
including alias identity. Constructors validate `start <= end <= raw_record_count`
using the existing resolved public count (universe UELs, scalar default row, aliases).
Invalid bounds return `InvalidOptions`; source/symbol mismatch is `SourceMismatch`.
This catalog bound does not independently prove a corrupt file's complete record
stream: in-session checkpoints retain serial error detection during consumption.
Persistent checkpoint state still comes only through a strictly source-verified
index and authenticated interval lookup. There is no untrusted state import.

The cursor copies the complete opaque restart tuple into its root-charged inline
state and retains the Reader/source. Callers can drop the checkpoint or index owner
after creating a cursor. A coordinator obtaining further interval starts retains
its index owner, as in decision 0031; no worker needs to reopen or revalidate it.
Mutating a sidecar later cannot change an already copied verified tuple. Source
immutability checks remain mandatory and retain the existing length/mtime assumption.

## Consumption

Only raw ordinals determine the end. Every decoded row advances the range,
including rejected rows. Exact selection precedes projection and the per-cursor
accepted limit. Limits/counters start fresh at the checkpoint. No row at or after
an interior end is decoded, even if filters reject the entire range. Read-ahead
and full compressed restart-block checksum verification remain bounded and may
read bytes beyond the range. No prefix records are decoded.

Empty ranges, including `[total,total)`, zero accepted limits and empty selections
do no record/codec work; structural options and source identity still validate.
A nonempty range fully consumed through EOF validates pending count/framing via
the existing decoder. An explicitly exhausted accepted limit may stop before EOF
framing, exactly as ordinary serial scans do. A full final batch can require a
subsequent `next_batch` call to observe EOF; dropping a cursor is cancellation.

Unselected ranges can export a checkpoint at the next raw ordinal, including
an interior end or an accepted-limit stop. Selected checkpoint export remains
unsupported, since accepted batches do not identify a simple raw suffix boundary.
Range boundaries do not follow a checkpoint into a fresh suffix/range cursor.

## Resources and failures

Range counters are inline in the existing `ScanCursor` reservation. The copied
restart tuple, selection references, decoder, batch and codec buffers share the
existing Reader root. No partition collection, full-symbol prescan or new cache
exists. Batch capacity is capped by possible remaining raw rows as well as the
payload/accepted limits, so a tiny range does not allocate a maximum-size batch.
At total, one loop iteration remains available to validate EOF without a batch.
Mid-range completion releases the active decoder while preserving its
position; cursor/checkpoint drops release their reservations independently.

Bounds/source/selection/options errors leave the borrowed checkpoint usable.
Resume I/O, malformed framing, source changes, filter failures and runtime budget
denials use the existing sticky cursor error path: unpublished fills are discarded,
acknowledged output/counters are preserved, and decoder/batch state is released.
Ranges have no index fallback and never replay acknowledged rows. EOF before an
expected range end is `InvalidCount`. No inferred CAS or crash-recovery guarantee.

Verify all ten frozen native fixtures (eight minimal), split/recombined ranges,
compressed/shared/crossing blocks, aliases/universe/default scalars, values,
selection/projection/limits, empty EOF ranges, malformed bounds/restore, source
changes, exact decoded counters and quota denial/release. Keep full reader/writer,
metadata-page, M0 and four-platform gates. Task 038 remains gated on this proof.
