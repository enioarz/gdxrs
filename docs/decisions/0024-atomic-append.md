# 0024 — Atomic checked rewrite append

Task: [030](../../tasks/030-atomic-append.md). This is a new-symbol transaction,
implemented in the existing Writer. Original UEL/symbol IDs never change.

## Ownership and phases

`Writer::open_append(source, destination, read_options, write_options, append_options)`
opens one immutable v7 Reader and creates an exclusive temporary output in the
canonical destination parent. Reader and Writer use the same root Budget, whose
configured sizes must match. Opening returns a pending owner before any fallible
journal initialization/import. `begin_append(metadata_options)` admits resources,
creates and retains the metadata owner, and explicitly streams the original file.
Pending writes are rejected; pending finish fails without publication. Abort and
drop work in either phase; explicit abort reports failures and retains retry ownership.

Import appends original UEL and text slots directly to the private journal, including
duplicate labels/text and empty text slots after slot zero. Slot zero must be the
native empty string. It preserves catalog order, numeric acronym IDs/order, direct
alias references, descriptions, comments, strict IDs plus relaxed display names,
prior error counts and set-text flags. The latter two use private journal tag 13;
this is not a GDX or public spill-format extension. Audit/producer header bytes are
preserved, including full-length strings. No nodes or caller maps are serialized
in GDX; new maps and text-node state remain explicit per-session state.

Every non-alias physical data stream is checked and re-encoded in bounded batches
under `min(reader.maximum_batch_bytes, 4096)`. A cap too small for one full row
returns BatchTooSmall; it is never inflated. Reader-synthesized defaults from
physical empty scalars are consumed/validated without authoring a physical row.
Writer keys/values, ordering and counts are checked. Import initially uses implicit
domains; once all inherited sets are complete, it installs the original strict
references and display names, validates alias chains and streams strict records
again for membership validation. Forward domain/alias references and completed
cyclic set dependencies require no in-memory dependency table. Direct alias IDs
remain direct alias IDs. Metadata validation of a membership run performs one
symbol lookup per run, not per member.

After import, registration opens for new UELs/texts/acronyms/symbols. Numeric binding
helpers return owner-bound tokens for existing raw UELs and symbols. Existing symbol
records cannot be written again. UEL rename, symbol comment/domain changes and
acronym metadata changes reject inherited targets. New metadata follows normal
phase/freeze rules. Sorted, unsorted, string, mapped and deferred adapters can write
new symbols. Empty newly registered symbols finalize normally.

Import/encoding failure terminates the transaction. It never retries escaped work.
`AppendStatistics.rewritten_records` acknowledges completed inherited physical-symbol
imports; it is not publication or an RSS measurement. `initialized` becomes true
only after the complete inherited catalog/data/domain validation succeeds.

## Publication policies and source contract

The caller explicitly selects one policy:

- CreateNew: destination must differ from source and be absent. Final publication
  uses the existing same-directory hard-link no-clobber operation. Existing and
  late collisions fail without replacing the destination.
- ReplaceSource: canonical destination must be exactly the source pathname; the
  source must be a regular non-symlink file. The caller must exclusively own this
  pathname through finish/abort and keep source bytes immutable during import.
  The final operation is a same-directory rename replacing that pathname.

Both policies validate new metadata/data, backpatch the major index, flush,
`sync_all` and close the output before publication. Source checks run before import,
after import, before finalization and after output close immediately before
publication. Positioned reads and batch scans also check the opened source.
The pathname is checked independently for regular-file type, length and modification
time; replacing the pathname while an old handle stays valid is therefore detected
when these fields differ. Source handles are closed before rename for Windows.

These checks inherit the Reader immutable-source contract. They are not a lock,
cryptographic identity proof or race-free compare-and-swap against a competing
writer; same-size edits with restored timestamps or an indistinguishable pathname
swap are outside that contract. Exclusive ownership is a requirement for ReplaceSource,
not a guarantee that the library acquires an OS lock. A concurrent writer between
the last check and rename is not supported. CreateNew always retains its final
no-clobber guarantee. Hard-link or replacement failure is reported without an
in-place fallback, unlink-then-rename sequence or backup choreography.

After rename the new file is published even if subsequent scratch cleanup fails;
`WriteStatistics.published` records that fact, the error remains visible and abort
retries only owned cleanup. It never deletes a published result or rolls back a
replacement. Before publication every failure/cancellation leaves original bytes
untouched by this library. External source mutations remain external mutations.
Rust File drop closes handles; no extra OS close-error reporting is claimed. The
fault hook checks the close/publication boundary, not a portable fallible-close API.

File syncing and atomic visibility do not imply directory fsync, power-loss
recovery or durability across a filesystem/OS crash. New output permissions follow
the writer temporary-file policy; source filesystem ACLs/ownership are not copied.

## Resources and cost

The aggregate admission ceiling covers maximum_output_bytes plus the entire
configured metadata/sort maximum_temporary_bytes, with checked addition. The
existing source is excluded; one new output is written and no second output copy
or backup is made. Admission is deliberately conservative and may reject a job
whose eventual actual size would fit. The handle ceiling admits two base handles
(source + output) plus the whole metadata/sort configured maximum_open_files.
No independent uncharged sorter is introduced. Initialization checks these sums
before journal allocation/import. All existing scratch shares and retained-cleanup
ownership rules apply. Controls/path conversions, decoder/encoder buffers and
batches share the same root; no original dictionary/catalog/record table is collected.

At least O(file-size) record I/O and O(output-size) output space are required.
Strict symbols receive a second read for membership validation. Metadata lookups,
reference validation, per-symbol finalization and membership binary searches add
cost; repeated journal/name scans can be quadratic or worse. This implementation
makes no general linear-runtime, throughput parity, OS/RSS or out-of-core append
qualification claim. Task 031 remains the general-input scale/performance gate.
There is no raw-block passthrough optimization in task 030: output is canonical
little-endian v7 with the explicitly selected raw/zlib codec. All inherited physical
records and semantic metadata are preserved, not original compression bytes,
record bounds, domain-name-table internal indices or unused physical padding.

Invalid names/labels/subtypes or metadata outside the established writer contract
fail explicitly before publication. In particular, native empty set-text slot zero
is required. This does not silently coerce names, remap IDs or drop metadata.
