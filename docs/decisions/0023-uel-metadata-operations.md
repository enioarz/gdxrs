# 0023 — ID-preserving UEL and metadata operations

Task 029 closes its scoped inventory through explicit typed operations. No new
dependency or unsafe code is needed. [API disposition](../uel-metadata-api.md)
lists native equivalents, adapter differences and unsupported call-state behavior.

## IDs, matching and mutable phases

`Writer::rename_uel` changes the exact label bytes of an existing writer token,
including after earlier symbols finish. Raw ID, registration order, caller-map
pairs, record keys, numeric values, alias targets and strict domain IDs remain
unchanged. New labels follow the existing <=63-byte printable/quotable writer
rules. Matching is exact bytes; case-only/trailing-space distinctions remain
intentional, unlike native case folding/right trimming. Rename to any existing
label, including the same ID's current label, is an error. Unknown/foreign IDs
fail explicitly. No collision overwrites, merges, truncation or ID reassignment.

| Operation | Before data freeze | Between finished symbols / before publication |
| --- | --- | --- |
| Register UELs, symbols, texts, acronyms and strict domains | Existing registration rules | Rejected; schema identity stays frozen |
| Rename existing UEL | Allowed | Allowed; bounded rewrite relocates membership anchors |
| Add symbol/alias comments | Allowed, <=255 exact bytes | Allowed, same checks |
| Set relaxed display-domain names | Allowed | Allowed; strict IDs/membership stay in force |
| Set/get text node numbers | Session bookkeeping | Session bookkeeping; no file serialization |
| Update acronym name/text | Existing task-028 rule | Rejected after freeze |
| Change symbol identity/type/dimension/user info, alias target, strict domains | Declare during registration | No mutation API / rejected |

Borrowing record writers prevent simultaneous metadata edits during an active
symbol. Failed, aborted or published Writers reject operations. Writer errors are
terminal: they block final publication rather than rolling back one edit and
continuing after a partial transaction. Existing no-clobber atomic finalization,
secondary diagnostics and retryable cleanup remain required.

## Bounded journal replacement

A shared private rewrite pass replaces variable-length frames while charging old
and new journals simultaneously to the existing journal quota. It reuses the
reserved auxiliary slot, fixed packets/frames and the same root buffers. Both
handles close before rename; the replacement reopens under the original owner.
Failures at allocation/copy/sync/rename/reopen retain owned paths for abort retry.
Task 028's pre-freeze acronym update uses this helper too.

Post-record edits require more than copying frames: a completed one-dimensional
set's membership start is a **journal byte offset**. The rewrite tracks one
contiguous membership group at a time, validates symbol/key order/count, and
relocates each completion anchor to the new journal position. Empty/non-set
anchors are relocated too. GDX data positions refer to the separate output file
and are never shifted. No table of offsets or memberships is retained in RAM.
Reader policy journals have separate frame meanings and never use this helper.

## Display domains and set-text flags

Strict domain registration and membership rules remain immutable after freeze.
`set_relaxed_domains` independently appends display names, as native SetDomainX
does. Repeated display edits preserve old registered name IDs while the latest
complete axis set supplies current display metadata. Strict alias IDs are still
serialized alongside a relaxed display table; changing display text cannot bypass
membership checks. Invalid names/arity fail rather than silently becoming `*`.
Empty display names explicitly mean `*`; zero-dimensional edits are no-ops.

Accepted set rows with nonzero set-text IDs mark a private summary, and final
symbol metadata now serializes the corresponding has-set-text flag. The native
SetHasText name is a query, not a setter. Alias/non-set flags stay false; empty
sets stay false. Unused display-domain work and impossible non-set flags do not
add full-journal scans to ordinary finalization.

The private checksummed Store gains tags 10 (text node), 11 (display domain) and
12 (set-text summary). These are ephemeral journals, not a GDX or public spill
format change. Registration validation checks new pre-freeze frames; node state
is omitted from GDX. The frozen fixture generator and golden hashes stay unchanged.

## Read views and callbacks

`Reader::metadata_session` returns its cleanup owner before lazy journal allocation.
It holds sparse rename overrides and nonzero text nodes on disk. It never changes
the source file, source UEL count, raw keys, symbol metadata or another view.
Lookup consults the latest override, without caching a full dictionary. Repeated
renames retain history under quota until close. Direct Reader lookups see the
immutable source; session lookups see the explicit view. Source changes are errors.

Text-node IDs refer to existing zero-based text slots, including slot 0. Any i32
node is allowed; it is not an allocation size or index. Assignment succeeds while
the current node is zero. Zero leaves it assignable; after a nonzero assignment,
another assignment returns false, including attempts to clear it. This matches
native. Native files serialize text strings only, so all nodes are zero after
reopen; reader/writer session nodes do not persist.

Fixed-size UEL/text information results include original raw IDs and optional
caller IDs. Mapped lookup/inverse lookup/highest-ID information reuses the sparse
index; caller zero is distinct from an absent mapping. An explicit checked join
with MetadataSession supplies renamed labels without changing caller-ID pairs.
Missing mappings remain Options for lookup and errors for mapped domain visits.
No dense allocation is made for large caller IDs or text-node numbers.

`visit_uels` streams a fallible fixed-size callback using view labels. `visit_domain`
explicitly scans/spills/deduplicates one observed axis and calls back in mapped
order with original raw IDs. It requires complete observed mappings; there is no
implicit expansion or error-list truncation. The existing unique-domain cursor
supports selected/raw-domain extraction. Callback statistics acknowledge only
successful calls; failure is terminal, preserves partial counts and never
implicitly replays output. Cursors close explicitly before parent cleanup,
including callback errors; secondary cleanup and retry ownership survive.
Domain prescans use the reader's configured batch cap, up to 4096 bytes. Replay
checks source integrity before each entry, after acknowledging each successful
callback and before completion. A callback that changes the source is still
acknowledged, but terminates the visit with `SourceChanged` and `complete: false`.

Max-length helpers stream catalogs or explicitly scan all symbol records with
fixed 20-axis state. Session variants use view labels. They never run implicitly
to satisfy a normal scan or short read. Lengths are bytes, not Unicode characters.

## Bounds and limitations

Each explicit read view is a separate scratch job using existing MetadataOptions
and the Reader's shared memory root. The reused owner needs six configured scratch
handles, uses the journal half and one live journal handle. A map/view pair uses
two explicit scratch quotas; it does not share an implicit aggregate disk cap.
Writer edits stay within the Writer's existing quota halves and handle allowance.
Fixed controls/results and all buffers compete for the same applicable root.
Caller-retained callback results use caller memory. Drop cleanup is best effort;
close/abort reports errors and retries owned cleanup.

Journal rewrites are linear per edit. Repeated overlay queries can cost
O(UELs × edit history); display/metadata scans and many mutations can be quadratic
or worse. Domain callbacks explicitly prescan and sort. No throughput, RSS or
larger-than-RAM qualification follows from these small functional tests. Task 031
retains general-input acceptance. Append remains task 030. Read-session catalog
expansion, mutable caller maps, implicit domain-map expansion, legacy error numbers
and other C-call-state behavior are explicitly unsupported here.
