# 0009 — Large exact labels and unique domains (task 015)

## Contract before implementation

`Reader::large_label_lookup(&SpillOptions)` returns a `LargeLabelLookup` builder.
`push(&[u8])` assigns zero-based checked u64 request ordinals; labels are exact
bytes of at most 255 bytes. `finish()` externally sorts requests and the streamed
UEL table, then returns `LargeLabelCursor`. Its lending `next()` yields
`LargeResolvedLabel { ordinal, label, id: Option<UelId> }` in `(label, ordinal)`
order. Ordinals preserve original request identity; output is deliberately not
reordered to request order. Duplicates remain distinct, unknowns yield None,
and duplicate stored labels resolve to the first original one-based ID.
Empty requests avoid the UEL table. Full completion validates both sorted streams.
An error is terminal; previous output is partial. Explicit close reports cleanup;
Drop is best effort. Cursors retain the source.

`Reader::selection_from_large_labels(symbol, dimension, base, cursor)` consumes
matches and externally sorts known IDs with empty labels. It intersects disk
membership with an existing source/symbol-bound base selection; the base must
have no disk clause yet. The disk clause may target any original dimension.
The finalized ID run is fully verified before use, then binary-searched through
positioned reads of fixed ten-byte frames. No universe bitmap or ID offset vector.
IDs are never remapped. Cloned selections share the immutable run and one handle.
`Selection::close_spill(&mut self)` explicitly cleans scratch only with exclusive
selection ownership; retained scans/clones cause InvalidOptions. Drop is best effort.
Scratch parents remain trusted and immutable as specified by decision 0008.

`Reader::unique_domains(symbol, selection, dimensions, scan_options, spill_options)`
validates distinct original dimension indices, scans accepted rows with the exact
limit, externally sorts `(dimension, original UEL ID)` and returns `UniqueDomainCursor`.
Its next returns unique `DomainId { dimension, id }` in dimension/ID order.
Projection in scan_options is validated but extraction explicitly requests retained
keys; values are not retained. No per-dimension whole-universe bitmap. It enumerates
observed IDs, not declared domain metadata. Empty dimensions still validate inputs
and avoid record reads. Full enumeration is blocking before first domain result.

## Resource composition

All buffers and fixed controls use the existing source root quota. A lookup divides
the supplied temporary-byte and handle limits into three equal per-session shares,
rounded down; each share must support fan-in plus an output. At most three sessions
coexist (request sort, UEL sort, membership output). Thus their combined reservations
never exceed supplied limits. Each run buffer uses run_bytes; concurrent working
sets may fail the shared root budget. This conservative split reserves merge
headroom without silently raising quotas. Unique domains uses a single full-limit
session. Statistics expose each run's counters; logical quotas are not RSS/physical
disk guarantees. Scale qualification and throughput remain task 016.
