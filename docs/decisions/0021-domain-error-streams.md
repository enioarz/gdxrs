# 0021 — Deferred domains and bounded data-error callbacks

Task 027 retains terminal errors on every existing writer API and on
`DeferredRecordWriter::finish`. Recovery is explicit through
`finish_reporting(ErrorLimits, callback)`. Only domain-membership failures and
duplicate keys are recoverable. Invalid shape, foreign/unregistered IDs, invalid
typed values, I/O, budget exhaustion and dependency cycles terminate the transaction.
No dependency or wire/spill-format change is needed.

Register metadata and UELs before `begin_deferred_records`. Stage each symbol in
one contiguous group, in any dependency order and any row order. Raw, mapped and
exact registered-string inputs share the phase. String inputs resolve existing
labels; automatic first-encounter registration remains task 025's separate API.
Mapped inputs require a completed immutable map. No adapter silently invents IDs.

The phase owns one checksummed Store in the parent metadata directory. Its
32 KiB buffer, fixed controls/keys/error-label scratch and spill allocations share
the existing root. Its bytes consume the journal half of MetadataOptions and
one handle is deducted from the existing sort allowance: at least seven total,
or eight with a retained map. Parent cleanup force-closes the shared Store before
removal, including on Windows; failed removal retains ownership for abort retry.
There is no in-memory graph, domain set, row registry or error vector.

The phase makes bounded passes over groups until their strict-domain sets have
completed. Untouched one-dimensional sets participate as empty groups. Direct
I(I) is allowed, matching native; alias-to-self and multi-set cycles cannot
establish membership and fail explicitly. Universe and relaxed domains do not
restrict membership. Target alias chains are validated but direct domain IDs
remain unchanged in metadata. Physical data order may differ from symbol IDs.
Group uniqueness scans and dependency passes may cost O(symbols × staged rows)
or worse with journal lookups; no performance parity is claimed for this adapter.

Each ready group uses the existing external sorter. The exact key is raw IDs in
big endian, then the global input ordinal, then local ordinal/origin/optional
caller IDs/typed values. It is at most 238 bytes for 20 dimensions/five fields.
The ordinal precedes values: among valid duplicate keys, first input wins,
including scalars. Domain checks precede duplicate checks, so every out-of-domain
occurrence is a domain error. Accepted set memberships remain sorted disk frames
and dependent keys use checked binary searches. Complete row counts, checksums,
sort EOF and explicit cursor cleanup are checked before symbol completion.

Callbacks borrow fixed row state and retain no library-owned error list. Global
input ordinals and per-symbol ordinals are one-based. Callback order follows ready
groups, raw lexicographic keys, then input ordinal, rather than global input order.
Each diagnostic carries writer/source session ID, symbol ID, raw keys, typed
values, origin, all bad axes and original caller IDs or exact string bytes where
applicable. Native signed bad-axis keys are represented by a bit mask while raw
IDs stay one-based. Session maps/error lists are never serialized into GDX.

Record and logical payload-byte quotas are checked **before** invoking the sink.
The logical charge is 34 header bytes, four bytes per raw key and present
caller key, the typed value packet (one-byte tag plus eight-byte numeric bits,
or twelve-byte acronym identity/bits), and one length byte plus bytes per string
label. The library does not account for memory or serialization the caller chooses
to retain. `errors` counts detected rejections, `reported` and `bytes` count only
successfully acknowledged callbacks. A quota breach or callback error is terminal,
preserves earlier partial counts and blocks publication. Previously delivered
side effects cannot be rolled back or automatically replayed. Successful recovery
requires the caller to explicitly finish the Writer transaction to publish.
Secondary cursor cleanup errors are attached before parent cleanup. If parent
cleanup also fails, the first cleanup diagnostic and retryable parent ownership
are retained. Every public deferred operation checks terminal state before
accessing closed metadata, preserving the original error. Drop remains best effort.

Mapped reads gain `scan_mapped_reporting`, an explicit prescan callback for each
missing-map row, including all raw axes and each mapping that exists. Existing
Error/SkipRecord policies still apply. Zero limit decodes/reports nothing; read
limits still apply after the mapped sort. Sink/quota faults are terminal and the
map owner retains partial error statistics and cleanup ownership.

The pinned native source and qualification differences are inventoried in
[domain/error API status](../domain-error-api.md). Native data error lists coalesce
repeated domain errors and silently stop at a cutoff. Rust deliberately reports
all rejected rows, or fails the whole transaction. It does not emulate loss of
error records. Task 031 retains general-input OS/RSS/throughput qualification.
