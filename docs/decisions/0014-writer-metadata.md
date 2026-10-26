# 0014 — Bounded writer registration and metadata ownership

Task 020 follows completed task 019. Registration is optional: an untouched writer
retains its empty-file behavior. `begin_metadata(&MetadataOptions)` creates an
exclusive private scratch directory under an existing trusted parent. All buffers,
controls and bounded paths share the writer root. No dummy Reader is constructed.
The spill sorter gains an internal explicit-budget constructor; existing reader
sorts retain their source identity and membership validation.

Registration APIs return writer-bound IDs, rejecting IDs from another writer.
UEL equality uses exact bytes; IDs are dense one-based first-registration order.
Incremental registration scans the disk journal for duplicates (bounded memory,
linear lookup per insertion). For a fresh dictionary, `register_uels_fresh` accepts
a fallible iterator of borrowed bytes or bounded owned byte arrays and a mapping
callback. Producer errors propagate before mapping/publication. It externally sorts `(label, ordinal)`,
records each group's first ordinal in a fixed disk index, then walks requests in
original order to assign dense IDs. It reports every original ordinal and its ID
without a lexical remap or unchecked unique-input promise. The callback can stream
its own results; callback failure is terminal and cannot be replayed. This bulk
path is the intended large-dictionary path. Symbols use ASCII-folded names and
bounded disk scans; no eager dictionary, name vector or offset vector is retained.

`register_symbol`, `add_comment`, `add_set_text`, `add_acronym` and `set_domains`
spool complete metadata incrementally. Symbol descriptions/comments/text are
bounded byte strings; names follow native ASCII identifier rules. UELs follow
pinned native <=63-byte printable/quotable rules while preserving accepted raw
bytes, spaces and case exactly. No truncation or UTF-8 normalization. Set text ID
0 is empty; duplicate text returns its first ID. Acronym numeric IDs and folded
names are unique. References accept a writer-bound symbol or a forward name.
Alias chains serialize their resolved underlying set ID, matching native alias
creation. Freeze resolves forward aliases/strict domains, checks 1-dimensional set/alias
semantics and alias cycles with bounded disk traversal, then prohibits mutation.
Relaxed domains preserve their byte names; strict and relaxed modes are explicit
and cannot mix within a symbol. An empty domain list denotes implicit universe.

Metadata is held in checksummed, offset-bound frames with explicit tags, lengths
and high-water bounds. Scratch reads validate framing/checksum/IDs rather than
trusting internal files. One persistent journal and at most two bulk auxiliaries
are open. The metadata half of the total scratch-byte cap includes all three;
one external sorter receives the other half. Three handle slots are reserved for
metadata, the remainder for the sorter with fan-in reduced to fit (minimum six
scratch handles). These shares sum to at most the configured aggregate, even while
merging. The GDX output handle/extent have the separate existing output contract. Reported
peak scratch bytes/handles are conservative sums of share peaks, not a sampled
simultaneous OS measurement.
The writer retains the created sorter directory for streamed numeric-run cleanup
if a merge/cleanup failure outlives the sorter; unexpected entries are preserved.
Cleanup uses exclusive owned paths; explicit writer abort/failure retries and
preserves primary plus secondary errors. Drop cannot publish or imply success.

`freeze_metadata` validates registration. Finish may freeze automatically. Task
020 can publish complete metadata and empty symbols: a minimal zero-record data
header/terminator supports that concrete interoperability gate, without exposing a
record writer. Empty scalar defaults retain upstream semantics. Sorted nonempty
records remain task 021 and full record/value/finalization qualification is 022;
no future record-writing stubs are added. Metadata traversal is bounded but symbol
name/reference scans may be quadratic; performance/RSS acceptance remains 023.


Wire/validation provenance: pinned GDX `9e2a8c8932412010cd7d2f94727d1740143bf3da`
`src/gxfile.cpp` (`CanBeQuoted`, `GoodUELString`, `IsGoodNewSymbol`, `InitDoWrite`,
`gdxClose`, acronym serialization), and `generated/gclgms.h` (63-byte identifiers).
The implementation independently encodes this layout; no production native link
or new dependency is introduced. Symbol/acronym names share a folded namespace.
