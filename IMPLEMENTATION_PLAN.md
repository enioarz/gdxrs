# gdxrs implementation plan

Status: M0 and M1 are merged; M2 read acceptance has passed on Linux x86_64. Runtime evidence and performance limitations are recorded in [task 016](docs/evidence/016.md). Native macOS ARM64/Intel and Windows x64 reader compatibility is qualified in [task 017](docs/evidence/017.md). M3 has begun with bounded empty-v7 creation and atomic publication ([task 018](docs/evidence/018.md)); symbol/record writing and subsequent milestones remain pending.

Execution guidance: read [AGENTS.md](AGENTS.md) and the [numbered task queue](tasks/README.md) before implementation. Tasks 001–018 record the completed deliverables and evidence; tasks 019–023 specify the remaining M3 writer gates.

## 1. Goal and priorities

Build an independent Rust implementation of the GAMS GDX library, with interoperable GDX reads and writes, fast sequential throughput, and symbol slicing that works when the records **and metadata** exceed RAM. The eventual consumer is `enioarz/polars-gdx`; the core must remain usable without Polars, Arrow, Python, a GAMS installation, C++, or a native GDX library.

Priority order:

1. Correct current-format interoperability and preservation of GAMS semantics.
2. Enforceable memory limits throughout reading, slicing, indexing, and writing.
3. Fast sequential reads/writes and low time to first batch.
4. Fast repeated selective queries through optional persistent indexes.
5. A small, auditable dependency graph and a simple Rust API.
6. Full modern library functionality, followed by adapters and companion tools.

This document began as the implementation plan. The bounded serial reader and M2 read qualification are now implemented; the remaining roadmap and separate `polars-gdx` integration retain the gates below.

## 2. Research baseline

Inspected on 2026-10-23. Pin oracle builds and fixtures to these revisions before implementation starts; do not use an unrecorded moving branch as the compatibility reference.

| Reference | Revision | Relevant evidence |
| --- | --- | --- |
| [GAMS-dev/gdx](https://github.com/GAMS-dev/gdx/tree/9e2a8c8932412010cd7d2f94727d1740143bf3da) | `9e2a8c8932412010cd7d2f94727d1740143bf3da` | `src/gxfile.cpp`, `src/gxfile.hpp`, `src/gdlib/gmsstrm.{cpp,hpp}`, `src/gdxapi.yaml`, `generated/gclgms.h`, `src/tests/` |
| [enioarz/polars-gdx](https://github.com/enioarz/polars-gdx/tree/23beb2429d4a362c3c078c239d9136ae2f463e27) | `23beb2429d4a362c3c078c239d9136ae2f463e27` | `crates/gdx/src/reader.rs`, `writer.rs`, `types.rs`, `crates/polars-gdx/src/lib.rs`, `python/polars_gdx/lazy.py`, fixtures, tests, and performance notes |

Observed constraints that shape this design:

- Upstream writes **file format version 7**. Versions 5 and 6 have separate older layout/decoding paths. File format versions are distinct from library release numbers such as 7.12.1 and GAMS release numbers.
- The header has section offsets, and the symbol table has symbol data positions. Opening a symbol need not scan other symbols' records. Symbol positions do not provide arbitrary record positions.
- Records use variable-width keys, delta encoding, and tagged values. A file byte offset alone is generally insufficient to restart decoding.
- Compressed sections use independent blocks with a three-byte header and up to 32 KiB of decoded data. A block can contain a zlib stream or stored, uncompressed bytes. A record can span blocks, and a block boundary is not necessarily a record restart.
- Upstream's `gdxDataReadSliceStart` scans records and builds dimension mappings; `gdxDataReadSlice` scans again. Those semantics do not imply a random-access slice index.
- UELs (unique element labels), symbol descriptions, set text, domains, and acronyms have their own tables. Eagerly loading these can exhaust RAM even if record decoding streams.
- Current offsets can be 64-bit while several counts and identifiers are serialized as signed 32-bit integers. Larger-than-RAM operation does not remove format limits.
- The Polars repository vendors additional checkpoint/range functionality, including compressed-block position plus decoded offset. Treat this as a useful design reference and regression source, not an upstream GDX format extension.
- The Polars wrapper currently accumulates vectors, builds a full-file UEL dictionary for `Enum` columns, and its Python source ignores `batch_size` before yielding one result. Replacing the decoder alone will not make that interface out of core.
- Upstream's introductory README says appending is unavailable, but `gdxapi.yaml` and `gdxOpenAppend` implement append of new symbols in v7. Resolve documentation conflicts against code and oracle tests.

The upstream library is MIT licensed. Preserve required notices for any adapted code, tests, or fixtures and record their provenance. Design verification uses upstream as an oracle; production code must not link or delegate to it.

## 3. Compatibility policy

**Default target: read and write GDX v7. Do not implement v5/v6 support in the initial library.** The user explicitly permits dropping very old compatibility when it helps performance. Reject unsupported versions early with an error identifying the version; never guess a modern layout for an old file.

Removing old format support simplifies validation, testing, and decoding specialization. Actual throughput improvement must be measured; merely removing a version branch does not establish a speedup. Older files can be converted externally with official tools. If a concrete future requirement justifies a legacy reader, isolate it in a separate adapter/feature and normalize into the modern API without adding branches to the v7 record loop.

Do not confuse old GAMS releases producing valid v7 files with old file formats. Preserve supported v7 layout variants, including optional domain metadata, after fixture verification. Unknown future formats fail explicitly. Read byte-order markers correctly; choose byte order once per decoder and write a consistent, documented byte order. Endian support need not impose per-field dynamic version dispatch.

“Complete reimplementation” means modern GDX library functionality, not an early declaration of parity after parameter reads work. Create a function-by-function inventory from `gdxapi.yaml` with statuses: native equivalent, adapter equivalent, deferred, or deliberately unsupported with rationale. Track all deferred items to closure before claiming full modern parity.

| Capability | Planned treatment |
| --- | --- |
| Sets, parameters, variables, equations, aliases, universe | Native support; scalar and empty-symbol behavior included |
| Dimensions and identifiers | Validate upstream limits, including up to 20 dimensions; typed UEL/symbol IDs |
| Five variable/equation fields and subtypes | Preserve level, marginal, lower, upper, scale and subtype-specific defaults |
| Special numbers and acronyms | Preserve UNDEF, NA, EPS, infinities, acronym identities and configurable mappings; do not silently collapse them into NaN or zero |
| Metadata | Names, explanatory text, comments, producer/system information, domains, relaxed domains, set text and associations |
| Raw, string, mapped reads/writes | Raw streaming core; explicit label/mapping adapters, including ordering semantics |
| Filtering, slices, domain enumeration | Bounded-memory native APIs; legacy slice dimension reduction/remapping via a compatibility adapter |
| UEL registration, lookup, renaming, mapping | Implement exact documented behavior, collision rules and error cases; disk-backed variants where size demands them |
| Compression | Interoperable zlib and stored-block reads/writes; uncompressed path always available |
| Append | Add new symbols to v7 files; duplicate symbol replacement remains an error |
| Error records, domain errors, tracing, diagnostics | Typed Rust equivalents; bounded or streamed diagnostics, never an unlimited error-record vector |
| C API, callbacks, legacy call-state/error-number surface | Later separate compatibility crate; no C ABI burden in the native hot path |
| Auto-conversion and external GAMS utilities | Explicit external-tool adapter only if needed; no hidden subprocess/runtime dependency in the core |
| `gdxdump`, `gdxdiff`, `gdxmerge` | Later companion CLI work with its own parity checklist; reuse streaming/spill primitives |
| Other generated language bindings | Subsequent adapters, after Rust and C contracts stabilize; not a prerequisite for the Polars adapter |

Semantic interoperability, rather than identical compressed bytes, is the compatibility criterion. Preserve ordinary finite floating-point bits where the format allows; define oracle-confirmed behavior for signed zero, NaNs, defaults, and special-value remapping.

## 4. Crate and dependency design

Start with one `gdxrs` library crate, organized internally rather than a large workspace:

| Module | Responsibility |
| --- | --- |
| `format` | Headers, sections, primitive codecs, limits, endian handling, v7 tags |
| `io` | Buffered positioned reads, compressed-block decoding/encoding, checkpoints |
| `model` | Symbol types, typed IDs, value fields and semantic metadata |
| `metadata` | Lazy table iteration, paged dictionaries, bounded lookup caches |
| `read` | Symbol cursors, selection planning, row/batch decoding |
| `write` | Symbol builders, streaming encoding, finalization and append |
| `spill` | Temporary files, sorted runs, bounded merge and dictionary storage |
| `index` | Optional persistent checkpoints, summaries and query planning |
| `error` | Structured failures with symbol, section and byte-position context |

Dependency policy:

- Use `std` for files, buffering, collections, errors, threads and bounded channels. Avoid async runtimes, Rayon, Serde, database engines, Arrow, Polars and PyO3 in the core.
- Tentatively use **`miniz_oxide` as the single direct default runtime dependency** for a pure-Rust zlib codec. Validate its encoder/decoder APIs, checksum behavior, transitive dependencies and performance in the codec milestone. Do not reimplement DEFLATE to chase a zero-dependency label.
- Make compression a default-enabled feature; a no-default-features build supports uncompressed files and reports `CompressionUnavailable` when necessary. Do not advertise this reduced build as full GDX support.
- Persistent sidecars may add one audited digest dependency (proposed `sha2`, enabled only with the index feature) for source identity. Benchmark the validation cost explicitly. Do not implement custom cryptography or use a process-randomized `DefaultHasher` for a persistent identity.
- Prefer buffered I/O initially. Consider `memmap2` only after measurements, behind a feature and a documented immutability/safety contract. Mapping an entire file is not a bounded-memory guarantee.
- Prefer safe Rust. Introduce unsafe/SIMD or alternative native compression only with demonstrated gains, narrow invariants and dedicated tests. A native codec, if justified, stays optional; a pure-Rust supported build remains mandatory.
- Add test/fuzz/benchmark dependencies only to development tooling. Record both direct and transitive counts, licenses, compile time and binary size. Choose and pin the MSRV at implementation bootstrap; do not invent it from the empty repository.

## 5. Reader and memory contract

The primary API is pull-based: `Reader` opens a seekable source, exposes lazy metadata, and creates independent symbol cursors. A cursor fills a caller-owned batch or reusable row buffer. A caller must explicitly request collection into an unbounded result.

Proposed API shape (illustrative, not yet a compile-tested contract):

```text
Reader::open(path, OpenOptions { memory_budget, cache_dir, index_policy, ... })
reader.symbols() -> fallible metadata iterator
reader.symbol(name_or_id) -> SymbolHandle
reader.scan(symbol, ScanOptions { selection, projection, batch_bytes, limit, ... })
cursor.read_batch(&mut reusable_batch) -> Result<BatchStatus>
reader.resolve_labels(label_iterator, lookup_options) -> bounded lookup stream
Writer::create(path, WriteOptions { memory_budget, temp_dir, compression, ... })
writer.start_symbol(metadata, key_bounds, input_order) -> SymbolWriter
symbol_writer.write_batch(batch)
symbol_writer.finish()
writer.finish() -> Result<WriteSummary>
```

### Decoding and output

- Validate the header and section locations first. Read only the needed metadata and selected symbol data. Do not pre-scan a whole symbol just to open a cursor or honor `head`.
- Decode raw keys into a fixed-size array and values into fixed storage. Use integer UEL IDs in the hot loop; resolve labels only when requested.
- Keep canonical UEL IDs one-based, matching GDX. Make the Arrow adapter's zero-based conversion explicit and checked; never clamp invalid IDs to zero.
- Expose columnar batches of primitive key/value arrays for consumers that want them, plus a low-overhead row/callback path. Preserve special-value identity with a compact representation or explicit conversion policy, chosen by profiling; do not make Polars' lossy numeric mapping the storage model.
- Evaluate exact native predicates before appending output. Projection can avoid allocation/conversion for unused fields, but record framing still requires consuming their encoded bytes. Never claim columnar disk pruning for this row-oriented format.
- A row limit counts accepted records. A zero limit or empty resolved selection avoids decoding. Stop exactly at the limit and support cancellation/drop without completing a scan.
- Keep file order by default. A read error is terminal and contextual; previously emitted batches are a partial result, never silently a complete one.
- Use independent handles/cursors for concurrent reads; avoid shared seek offsets and a global decoder mutex. An independent open or positioned-read abstraction is safer than assuming cloned file handles have separate offsets on every platform.

### Enforceable resource bounds

The budget includes library-owned metadata pages, label caches, filters, decoder buffers, output batch capacity, worker queues, reorder buffers, index pages, sort buffers and diagnostics. Reserve memory before allocation and use checked sizes/fallible allocations. Report `BudgetTooSmall` when even the minimum working set will not fit. Validate attacker-controlled counts before allocating.

For a scan with budget `B`, workers `T`, decoded block allowance `C`, batch capacity `Q` and metadata/index allowance `M`, enforce a plan such as `M + T*C + queued_batches*Q + scratch <= B`. Concurrency is reduced to fit the budget. Never allocate one whole result per worker and concatenate it.

An accounting budget is not an exact RSS cap: allocator overhead, stacks, runtime state and OS page cache differ. Test under an OS memory limit with explicit headroom, measure RSS separately, and publish both. Memory retained by a caller keeping old batches is outside the scanner's ownership; a bounded-memory consumer must process/release batches or use a backpressured sink.

Metadata must not be the hidden exception:

- Iterate symbol, UEL, set-text, acronym and domain tables in pages. Scanning a metadata section is acceptable when no index exists; loading it all is not.
- For compressed metadata, store page checkpoints as physical block position plus decoded offset and parser state where required.
- Small label lookups can scan the UEL table once against a bounded requested-label set. Large lookup sets spill to sorted runs and use a merge join. Preserve original file IDs and exact label bytes.
- Provide optional on-disk ID-to-label pages and a sorted label-to-ID lookup structure; cache only a budgeted working set. Do not require an in-memory offset vector or hash map proportional to all UELs.
- Large symbol-name catalogs, unique-domain extraction, mapped-index reordering, and legacy slice-local dictionaries use the same spill primitives.
- Treat labels as length-delimited bytes internally until upstream encoding/validation behavior is verified. UTF-8 conversion must be fallible or explicitly lossy. Symbol-name case rules and UEL exact-match rules are distinct.

## 6. Slice semantics and indexes

### Correct scans without an index

A selection is a conjunction of per-dimension constraints: all keys, one UEL, an allowed set, or a raw-ID range. Union within a dimension is supported; more complex predicates can remain a residual consumer predicate initially. Raw-ID order is registration/file order, **not lexical label order**.

Validate dimension numbers and IDs. Unknown labels and explicitly empty sets produce an empty result. By default retain all key dimensions and original IDs. Offer explicit projection of fixed dimensions and a separate legacy slice remapping adapter; do not silently change tuple meaning.

Without an index, scan and filter with bounded memory. Lexicographic key order can allow stopping after a leading-key interval, but reaching that interval may still require decoding the prefix. Non-leading filters may require decoding the entire symbol. This is the correct baseline for arbitrary existing GDX files.

### Optional persistent acceleration

Store acceleration data in a disposable, versioned sidecar/cache, not inside the standard GDX file. Queries remain correct without it. Offer `Off`, `UseIfValid`, and explicit `Build` policies; do not hide a full-file index build inside a short first query.

1. Build coarse checkpoints during one bounded-memory scan and write index pages incrementally. For newly written files, optionally record checkpoints while encoding.
2. A checkpoint contains source/symbol identity, format and codec information, record ordinal, physical block position and decoded offset for compression (or byte position without it), and sufficient decoder state immediately before the next record: prior keys, dimension widths/bounds and any other state identified by codec tests. Natural restart records can reduce stored state but must be proven independent.
3. Checkpoints at a configurable record/byte stride prevent a huge first-key group from becoming unsplittable. Persist full key state when a natural restart is unavailable. Cover cross-block records and multiple checkpoints within one compressed block.
4. Store leading-key ranges and conservative per-dimension min/max summaries per interval. Skip only intervals proven disjoint. Summaries on non-leading dimensions can be ineffective; return to sequential scanning when pruning is poor.
5. Keep the index itself paged on disk, with a bounded page cache and a disk-backed root/directory if needed. Index building and querying must not collect all checkpoints into a `Vec`.
6. For workloads that need repeated highly selective non-leading slices, evaluate a second, explicit index: externally sorted `(dimension, UEL, interval/checkpoint)` postings with deduplication. Intersect/merge postings as streams. Bound index disk usage and build time; never build every dimension by default.
7. Filter candidate records exactly after seeking. Record counts and half-open checkpoint ranges must prevent missing or duplicated boundary rows. Compression blocks alone are not valid partition boundaries.

Sidecars need a format version, source byte length and content identity, metadata identity, checksums, validated offsets and atomic publication. A path/mtime/size tuple is only a quick change detector, not proof of unchanged content. Default strict reuse validates a full source digest once per open/session, which can require an O(file-size) pass; benchmark this cost separately. A faster trusted-cache mode requires an explicit immutable-snapshot/content-identity contract and must not become the silent default. Do not reread the whole file for each slice within the same validated immutable session.

Source files are required to remain immutable while a reader is open. Detect observable replacement/truncation and fail; metadata checks alone cannot prevent arbitrary concurrent same-size modification. Corrupt or stale indexes fall back to a correct scan when detected before output. After output begins, an unexpected checkpoint inconsistency returns an error rather than restarting and duplicating emitted rows. Indexes are caches of a supported source, never a replacement source of truth.

## 7. Writer and external-memory operations

Implement the fast sorted-raw path first, then reuse it for more convenient inputs.

### Sorted raw writer

- Accept registered UEL IDs in the upstream raw ordering, validate lexicographic monotonicity, domains and duplicates, and stream encoded records through bounded buffers. Decide duplicate behavior explicitly; default to an error rather than silent last-wins.
- Preserve v7 key widths, delta codes, value tags, end markers, metadata sections and counts. Verify each element against upstream-generated fixtures.
- Key bounds determine encoded widths and precede data. Accept caller-supplied validated bounds for the fastest path. If exact bounds are unavailable, choose format-valid conservative bounds where possible, or spool/replay the symbol to determine them. Do not silently buffer the whole symbol, and do not patch a compressed header in place assuming fixed encoded length.
- Stream independent 32 KiB compression blocks, writing stored blocks when compression is not beneficial. Offer uncompressed mode and documented compression settings. Benchmark size/CPU tradeoffs; byte-identical zlib output is unnecessary.
- Write/spool metadata incrementally and patch the uncompressed section-offset table at finalization. Bound metadata registration storage as well as record storage.
- Require `Write + Seek` for the base writer. A nonseekable destination needs an explicit temporary seekable file and final copy; do not promise direct pipe output for a format requiring backpatches.
- Use checked 64-bit internal positions/counts, reject values beyond v7's on-disk field limits and report the offending field. Never truncate a count to fit. Larger logical datasets may require multiple symbols/files rather than an incompatible extension.

### String, mapped and unsorted input

- Expose an explicit `Unsorted` input mode with a memory budget, temporary directory and spill quota. Form sorted runs, then perform a bounded fan-in merge into the sorted writer. Limit open file descriptors and report temporary disk consumption.
- For incoming strings, preserve first-registration UEL order where compatibility requires it. Use a bounded map backed by sorted immutable disk runs with bounded compaction/lookup buffers, or a two-pass external deduplication/remapping algorithm. Do not assign lexicographic IDs accidentally. Select the simpler verified approach before implementing both.
- Bulk callers can pre-register a streamed dictionary and supply raw IDs for best performance. Keep lookup/deduplication costs separate from raw encoding benchmarks.
- Apply duplicate handling and domain validation during merge. Strict domain membership may itself need disk-backed lookup; relaxed metadata must not trigger an implicit in-memory set of every domain element.
- Use the same external merge primitive for mapped-output sorting and large unique-domain queries. Avoid implementing a general-purpose database.

### Finalization, append and failure handling

An explicit `finish()` returns all errors; `Drop` performs cleanup and never implies a successful write. For path-based atomic creation, write to a unique same-directory temporary file, flush/finalize, optionally sync for durability, then publish with documented replace/no-clobber semantics. Portable atomicity and crash durability need platform-specific tests.

Implement append of new symbols using an atomic copy/rewrite workflow first: preserve prior UEL IDs, metadata and existing data blocks where possible, append data, rewrite trailing tables and offsets, and publish only after success. Account for its O(file-size) copy cost and temporary space. A faster in-place mode is later work with an explicit recovery/durability contract. Inject failures at writes, seeks, compression, sync and rename; incomplete work must not masquerade as a valid finished file. Temp files use exclusive creation and cleanup on cancellation; disk-full errors are actionable.

## 8. Parallelism and performance work

First establish a fast serial implementation. Profile parsing, decompression, copies, allocations and syscalls before optimizing.

- Specialize once per symbol for dimensions, value arity, byte order and compression where it measurably helps. Use reusable buffers and checked decoding rather than per-record allocations.
- Start with buffered sequential I/O. Benchmark buffer sizes and compression backends on representative files; include opening and metadata lookup, not just an already-positioned hot loop.
- Parallel reads require validated checkpoints and independent cursors. Partition into half-open record ranges, use bounded channels and ordered delivery, and apply backpressure when an early partition is slow. A row limit cancels unnecessary work and counts globally accepted rows.
- Avoid cold index construction for small or short reads. Add a cost-based choice among serial scan, indexed scan and parallel scan using measured bytes, selectivity and available budget.
- Independent compression blocks can be compressed concurrently after serial record encoding; output order and the in-flight queue remain bounded. Do not parallelize writer state or UEL assignment before their ordering contract is established.
- Expose scan statistics: source bytes read, blocks decompressed/skipped, records decoded/emitted, checkpoint/index build time, cache hits, peak tracked allocation, spill bytes and elapsed time. This distinguishes true I/O pruning from output filtering.

## 9. Future Polars integration

Keep integration work in `polars-gdx` after the core streaming milestones pass. Initially support backend selection there for differential tests and gradual migration; remove `gdx-sys` only after the required behavior is verified.

- Map current symbol metadata, domains, UEL lookup, `value_field`, `key_filter`, `n_rows` and `threads` behavior onto the native API. Preserve case-sensitive labels, empty-filter behavior and serial ordering.
- Replace full `RawSymbolData` collection with a cursor-backed batch iterator; honor `batch_size` and projection. Keep Arrow/PyO3 types and dependencies in the adapter. Transfer owned primitive buffers where possible; do not promise zero-copy decompression or universal zero-copy conversion.
- Retain `Enum(full UEL table)` as an explicit fast path only when its dictionary fits the configured budget. For huge dictionaries, evaluate ID columns plus a paged resolver, or bounded string batches. Batch-local dictionary IDs cannot be silently treated as global Enum IDs. Choose schema and dictionary-unification behavior before execution, with Polars-version tests.
- Native predicates may prune only when sound. Keep residual predicates and their required columns, then project. Push a limit below a residual predicate only when equivalence is proven; otherwise count accepted rows after residual filtering across batches. Never implement residual `head(n)` as a per-batch limit.
- Ensure cursor ownership, cancellation, repeated scans, Windows handle closure, and thread safety without borrowing the old wrapper's global-lock/unsafe `Send` assumptions.
- Demonstrate a streaming sink under a memory cap. `.collect()` deliberately materializes a final DataFrame and is not proof of an out-of-core end-to-end workflow. Polars joins, sorts and dictionary unification have their own memory costs beyond the GDX scanner.

## 10. Verification and benchmark gates

### Interoperability and correctness

Build the pinned upstream library in isolated development tooling as a test oracle. Generate small deterministic fixtures and keep their generating source and license/provenance. A GAMS installation is not required for ordinary core tests; optional GAMS/gamsapi integration is a separate check.

- **Upstream writes → Rust reads** and **Rust writes → upstream reads**, compressed and uncompressed. Compare metadata and complete record streams using typed special-value semantics. Rust-to-Rust round trips alone are insufficient.
- Cover every symbol kind, dimensions 0/1/2/20, empty/default scalars, all subtypes and value fields, aliases/universe, sparse domains, comments, set text, acronyms, mapping, append and duplicate/error behavior.
- Cover key width transitions, delta boundaries, signed zero/special values, non-UTF-8 label behavior, endianness, maximum valid lengths/count representations and files crossing 4 GiB offsets. Test integer overflow with synthetic headers without requiring enormous allocations.
- Exhaustively compare serial, filtered, indexed, compressed, parallel and spill-based paths on small fixtures. Check order, exact limits, empty selections, late-leading filters, non-leading filters and wide accepted results.
- Force tiny batches and compressed-block boundary crossings, multiple checkpoints in one block, long first-key groups, partial reads/writes, stale indexes, cancellation and disk-full errors.
- Maintain a list of v7 layout variants verified by fixtures. Keep negative fixtures proving v5/v6 and unknown future versions fail cleanly.
- Fuzz headers, section tables, compressed payloads, key/value tags, aliases, domains and sidecars. Require no panic, undefined behavior, unchecked allocation or unbounded loop. Enforce decompressed block limits and verify zlib checksums. Run sanitizers/Miri for any unsafe code.
- CI: formatting, Clippy, unit/integration tests, dependency-feature combinations, Linux/macOS/Windows, and targeted architecture/endian coverage. Test the declared MSRV. Keep oracle toolchain dependencies out of production builds.

### Performance and larger-than-RAM proof

Use deterministic generators that stream their output, with separately generated metadata-heavy cases. Store environment/compiler/CPU/storage details, fixture seed and hashes, compression settings, budgets and result checksums with every benchmark run.

| Scenario | Required evidence |
| --- | --- |
| Full raw read/write | Records/s, encoded and decoded MiB/s, CPU time, allocations, peak RSS, output size |
| Open, symbol lookup, first batch and `head` | Latency and bytes read, including huge UEL/symbol catalogs |
| Leading-key slice | Cold no-index scan, index construction, validated index reopen, and warm reused-index query measured separately |
| Non-leading slice | Full scan versus summaries/postings; show actual skipped bytes/blocks rather than only fewer output rows |
| Compression | Both codec directions, low/high entropy, stored-block fallback, single/parallel workers |
| Unsorted/string write | Registration, sorting, spill I/O, merge passes and final encoding measured separately |
| Metadata-heavy workloads | Millions of labels, long labels, many symbols, large set-text tables and large filter sets |
| Out-of-core workloads | File and logical decoded data larger than memory cap, wide result streamed to a sink, metadata larger than cap, unsorted write and index build under the same cap |

Provisional gates, to be calibrated from the first reproducible baseline:

1. Run scale tests with files at least 4× the process memory cap, plus independently oversized metadata. With a fixed budget, increasing rows/labels must not create a proportional RSS increase. Use an initial 256 MiB library budget under a 512 MiB OS limit where supported and leave explicit runtime headroom.
2. Verify counts and rolling result digests against a separately established reference, not merely process exit status. Exercise outputs larger than RAM without collecting them. Large jobs can run periodically rather than on every PR.
3. Target at least upstream-equivalent serial raw read/write throughput on the principal corpus, with an initial investigation threshold of >10% median regression on a controlled runner. This is a target, not an established result; correctness and the memory ceiling remain mandatory.
4. On an indexed leading-key slice selecting a narrow interval, require materially fewer bytes/blocks decoded and lower warm latency than a full scan. Report index build and strict identity-validation break-even; do not advertise warm-cache latency as cold-query performance.
5. Measure medians and dispersion over repeated runs, with cold/warm cache treatment documented. Compare the same decoded work and special-value policy. Use current Polars/gamsapi numbers only as context, never as a portable speed guarantee or a substitute for raw codec comparisons.

## 11. Delivery milestones

Each milestone ends with committed tests, documented limitations, and reproducible measurements where applicable. Do not claim full parity before the inventory is closed.

| Milestone | Deliverables | Exit criteria / dependencies |
| --- | --- | --- |
| M0 — Format and oracle | Written v7 byte-layout specification, API parity inventory, pinned oracle build, fixture generators, license records, Rust bootstrap and benchmark harness | Layout questions resolved with fixtures; explicit legacy rejection policy; dependency/MSRV choices recorded |
| M1 — Serial reader | Checked primitives, compression blocks, lazy metadata access, raw record cursor and bounded batches for all symbol kinds | Differential fixture reads pass; first batch needs no full-symbol scan; malformed input tests pass |
| M2 — Bounded slicing and metadata | Exact filters, projection, limits, paged metadata, large label lookup and domain enumeration with spill | Index-free slices match reference results; record-heavy and metadata-heavy workloads pass a memory cap; **first usable out-of-core read release** |
| M3 — Sorted writer | Raw registration, supplied/conservative bounds, compressed/uncompressed encoding, metadata spooling, robust finish | Rust-written files read correctly upstream; full metadata/value cases covered; bounded raw read→write pipeline |
| M4 — General write and modern semantic parity | External sort, string/mapped adapters, domain checks, error streams, UEL operations, append, remaining native API inventory | Unsorted/string data and dictionaries exceed RAM safely; cross-reader/writer parity; failure injection passes |
| M5 — Persistent slice acceleration | Paged checkpoint index, identity validation, interval summaries, explicit index policy, optional postings after measurement | Cross-block resume is exact; stale/corrupt sidecars safe; index creation/query memory caps pass; cold/warm costs published |
| M6 — Performance and parallel execution | Profile-driven hot-path improvements, bounded workers/queues, planner and optional parallel compression | Ordered/cancelable parallel paths equal serial results; measured gains justify complexity and dependencies |
| M7 — Polars integration (separate repo) | Backend adapter, real batch source, schema/dictionary modes, predicate/limit correctness | Existing Polars tests plus large streaming-sink tests pass; no full-result or mandatory full-dictionary allocation |
| M8 — Compatibility surface and companion tools | Separate C ABI adapter, remaining modern API coverage, streaming dump/diff/merge, follow-on binding plan | Function/tool parity matrices close; explicit exclusions documented; only then describe the modern replacement as complete |

Main order: M0 → M1 → M2 → M3 → M4. M5 builds on M2 and can reuse writer checkpoints after M3. M6 follows validated indexing. Polars experiments may begin after M2/M3, but remain outside this repository's core dependency graph. A serial library can ship before parallel acceleration; bounded-memory support cannot be deferred behind an eager MVP.

## 12. Decisions to validate during implementation

No further user decision is needed to begin M0. Defaults are v7-only, pure Rust, local seekable files, bounded-memory APIs, synchronous I/O, optional sidecars, and no Polars dependency in the core.

Resolve these with source analysis, fixtures and measurements:

- Exact v7 variant, encoding, scalar/default, acronym and numeric mapping semantics; distinguish intended format behavior from upstream bugs.
- Whether direct `miniz_oxide` meets throughput goals without an extra wrapper; whether alternate backends justify optional dependencies.
- The minimum complete checkpoint state and practical checkpoint stride under real data skew.
- Whether summaries suffice for target non-leading queries before paying for postings indexes.
- The tradeoff between strict full-digest sidecar reopening and a supported immutable-source identity contract; expose both costs honestly.
- The simplest bounded dictionary registration/lookup design that preserves UEL ordering and remains fast.
- Concrete on-disk count limits and graceful diagnostics for datasets that cannot fit one standard v7 symbol/file.
- A Polars schema mode that handles dictionaries larger than RAM without changing label semantics or secretly materializing all categories.

Keep these decisions in short design notes as evidence appears. Avoid committing to a new GDX dialect, a homegrown general database, or speculative performance targets as established facts.
