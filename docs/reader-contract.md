# M1 reader contract

Status: implemented for M1; see [runtime evidence](evidence/011.md) and [runtime details](decisions/0005-m1-runtime-details.md). Rule references point to [the v7 specification](format-v7.md). The immutable M0 [acceptance blueprint](reader-acceptance.json) remains documentation coverage; `reader-check` executes runtime tests. M2 scenarios remain future work.

## Surface and ownership

The reader exposes these types/methods, with concrete names and return semantics. Supporting structs may have private fields; do not expose unchecked constructors. This task intentionally adds no panicking stubs.

```rust,ignore
Reader::open(path: impl AsRef<Path>, options: OpenOptions) -> Result<Reader>
Reader::symbols(&self) -> Result<SymbolCursor>
SymbolCursor::next(&mut self) -> Result<Option<SymbolMetadata<'_>>>
Reader::symbol(&self, id: SymbolId) -> Result<SymbolHandle>
Reader::find_symbol(&self, name: &[u8]) -> Result<Option<SymbolHandle>>
Reader::uels(&self) -> Result<UelCursor>
UelCursor::next(&mut self) -> Result<Option<UelEntry<'_>>>
Reader::comments(&self, symbol: &SymbolHandle) -> Result<CommentCursor>
CommentCursor::next(&mut self) -> Result<Option<CommentView<'_>>>
Reader::set_texts(&self) -> Result<SetTextCursor>
SetTextCursor::next(&mut self) -> Result<Option<SetTextView<'_>>>
Reader::acronyms(&self) -> Result<AcronymCursor>
AcronymCursor::next(&mut self) -> Result<Option<AcronymView<'_>>>
Reader::domains(&self, symbol: &SymbolHandle) -> Result<DomainCursor>
DomainCursor::next(&mut self) -> Result<Option<DomainView<'_>>>
Reader::scan(&self, symbol: &SymbolHandle, options: ScanOptions) -> Result<ScanCursor>
ScanCursor::next_batch(&mut self) -> Result<Option<BatchView<'_>>>
BatchView::len(&self) -> usize
BatchView::key_dimensions(&self) -> &[u8]
BatchView::value_fields(&self) -> &[ValueField]
BatchView::key_column(&self, dimension: u8) -> Option<&[UelId]>
BatchView::value_column(&self, field: ValueField) -> Option<&[Value]>
BatchView::row(&self, index: usize) -> Option<RecordView<'_>>
```

These are **lending** APIs: views borrow their cursor's current buffers. The caller cannot request the next batch/entry while retaining a borrowed view. They are not standard `Iterator` implementations returning references to an iterator's own mutable buffer. There is no unbounded `read_all`/`collect` convenience method in M1. Consumers wanting owned data explicitly copy it and own that memory; a future budgeted ownership-transfer API is separate M7 work.

`SymbolMetadata` exposes `id`, borrowed `name`/`description` bytes, dimension/type/user information, physical/projected counts, compressed/text flags and comment count; it does not contain an eagerly collected comment list. `UelEntry` has ID and borrowed label bytes. Comment views have one-based ordinal and bytes; set-text views have zero-based ID and bytes; acronym views have ordinal, typed ID, name/text bytes. Domain views have dimension position, optional strict symbol ID and optional resolved relaxed-name bytes; universe references are explicit. Variable-sized metadata content stays borrowed and each entry is fallible. Domain state has at most 20 dimensions, not an array sized by the full symbol catalog.

`Reader` owns an immutable source/header/options and a shared budget via `Arc`. Cursors and handles retain the source while the reader value is dropped. A `SymbolHandle` contains source identity and an ordinal plus only bounded fixed metadata; using a handle from another reader returns `SourceMismatch`, even if the path/ordinal matches. No `Send`/`Sync` unsafe wrapper is added. A global decoder lock is forbidden.

M1 supports path-based local files only. It does not promise generic `Read + Seek`, pipes, remote storage or asynchronous I/O. A file adapter exposes `read_at(offset, destination)` without shared seek state (Unix `FileExt::read_at`, Windows `FileExt::seek_read`, with short-read looping). All cursors maintain independent logical positions on the original opened handle. Do not rely on `File::try_clone` having an independent offset. A future generic seekable adapter must be exclusively owned or provide a proven positioned-read contract.

The source must remain immutable for its lifetime. Capture length and available platform file identity at open, detect observable truncation/change and return `SourceChanged`. Sharing the original handle avoids path replacement changing which file a later cursor reads. No claim is made to detect every concurrent same-size write. Retained path diagnostics can be explicitly truncated to 1024 encoded bytes with a truncation flag; this does not alter the opened path.

## Opening and metadata

`OpenOptions` has `memory_budget_bytes: usize` (default 64 MiB), `metadata_cache_bytes: usize` (default 128 KiB, maximum the root budget), and `maximum_batch_bytes: usize` (default 64 KiB, nonzero and within the budget). Budgets below 1 MiB return `BudgetTooSmall`; invalid contradictory options return `InvalidOptions`. Compression availability is decided by the feature at compile time; a compressed file requiring an unavailable codec returns `CompressionUnavailable` without treating bytes as plain input.

Open reads the F01–F03 header, validates primitive profile/version/required pointer bounds, and stores counts/metadata lazily. It must not scan symbol record streams or eagerly build the UEL/symbol catalog. Reading symbol counts may inspect the start of the symbol table, including its first compressed block; that block may contain a whole small table naturally. Distinguish necessary metadata bytes from unrelated record reads in tests.

`SymbolCursor`/`UelCursor` preserve file order and expose fallible byte views. Name/ID lookup may scan metadata on a cold source. Only budgeted metadata pages/cache entries persist; no hidden ordinal offset vector or reverse hash map. Labels/descriptions/comments/domains/text/acronyms are resolved lazily through views or dedicated bounded queries, not nested eagerly allocated vectors in every handle. Name lookup is ASCII-folded; UEL lookup policy is in decision 0002.

Alias resolution performs at most `symbol_count` target hops using bounded state; exceeding it signals `AliasCycle`. Missing/non-set targets are `InvalidReference`. No visited set proportional to all symbols is required. Universe ID 0 has dimension 1 and synthetic UEL-count rows. Scalar defaults follow F14. Metadata must expose both physical stored count and projected raw-view count where they differ, using `u64` internally and checked conversions at format boundaries.

## Scanning and batches

`ScanOptions` contains `key_projection: KeyProjection` (`All` or `Mask(u32)`, default `All`), `value_projection: ValueProjection` (`All` or `Mask(u8)`, default `All`), `limit: Option<u64>` (default none), and `batch_bytes: usize` (default 64 KiB; `Reader::scan_options()` uses the reader's maximum; nonzero and no larger). `All` is expanded after the symbol is known, rather than using an invalid all-bits-set numeric mask. Explicit mask bits outside the symbol dimension or value arity are `InvalidProjection`. `ValueField` is `Level`, `Marginal`, `Lower`, `Upper`, `Scale`; sets/parameters/universe/alias-set rows expose level only. Requested columns are always in original dimension/field order.

The cursor keeps all previous keys in fixed storage even when some key columns are not projected. The decoder consumes all encoded value tags/payloads; projection avoids allocation/conversion/output work, not necessary framing reads. A batch is columnar primitive-ID arrays plus typed-value arrays, all with a common row count, and bounded metadata identifying the projected fields/dimensions. Empty projection is valid: row counts/limits still work, with no key/value output columns. Column access by an absent dimension/field and row access out of range return `None`.

`next_batch` returns a nonempty batch or `None` for EOF; it never emits endless empty batches. Batch row capacity is calculated from the actual selected element layouts with checked arithmetic and a bounded row-count ceiling (65,536), not total symbol count. Zero projected payload still uses that ceiling. A requested byte capacity unable to hold one projected row returns `BatchTooSmall` before output allocation. The metadata/decoder working set may independently cause `BudgetTooSmall`.

Limits count accepted/public rows, including scalar/universe projection. `Some(0)` returns `None` without record decoding; normal metadata/reference validation can still occur. M1 has no selection predicates yet, so every decoded/public row is accepted. M2 adds an explicit validated `Selection` before the limit, with empty resolved selections returning no data reads. Do not add an eager filter engine or stabilize a complex predicate AST during M1.

Dropping the cursor cancels immediately and releases its owned budget/buffers. EOF remains EOF; an error makes the cursor terminal. After an error, subsequent calls return the same terminal error category/context and emit no further rows. A fill that fails exposes no partially filled batch; previous returned batches remain a partial result and are never relabeled complete. No silent serial retry or duplicated output.

## Error contract

`Error` includes a stable `ErrorKind`, optional source/symbol ID, optional section and physical/decoded offset, and bounded diagnostic details; I/O errors retain their cause. Required kinds are `Io`, `UnexpectedEof`, `UnsupportedVersion`, `UnsupportedByteOrder`, `CompressionUnavailable`, `InvalidHeader`, `InvalidOffset`, `InvalidCount`, `InvalidDimension`, `InvalidType`, `InvalidKeyTag`, `InvalidValueTag`, `InvalidCompression`, `InvalidReference`, `InvalidAcronymId`, `AliasCycle`, `SourceMismatch`, `SourceChanged`, `InvalidOptions`, `InvalidProjection`, `BatchTooSmall`, `BudgetTooSmall`, `AllocationFailed`.

Untrusted bytes cannot cause unchecked indexing, integer casts/overflow, `unwrap`/`expect`, recursive alias walks or indefinite loops. Native `Result` errors do not consume a global last-error slot. Runtime panic/fuzz validation is an implementation task; documentation coverage cannot prove those properties.

## M1 acceptance and later extensions

Implement the named scenarios from `reader-acceptance.json` using actual typed results, counting/fault-injection source adapters and allocation accounting. No scenario is considered executed by `contracts-check`. Cross-implementation fixture checks remain mandatory. M2 handles exact filters, large label-request sets, metadata spilling/unique domains and scale tests. M3/M4 add writers and registration; M5 adds sidecars; M6 parallel ranges; M7 owned Arrow batches. Those extensions must preserve the source/limit/error/ownership contracts here.
