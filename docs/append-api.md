# Atomic new-symbol append

Task 030 adds `Writer::open_append` and explicit `begin_append`. The source remains
unchanged until finish publishes a complete checked rewrite. Choose CreateNew for
an absent destination or ReplaceSource to replace exactly the source pathname.
ReplaceSource requires exclusive caller ownership of that pathname through finish.

```rust,no_run
use gdxrs::*;
# fn example() -> gdxrs::Result<()> {
let mut writer = Writer::open_append(
    "original.gdx", "extended.gdx",
    OpenOptions::default(), WriteOptions::default(),
    AppendOptions::new(AppendPolicy::CreateNew),
)?;
writer.begin_append(&MetadataOptions::new("scratch"))?;
let original_key = writer.append_uel_id(UelId::new(1)?)?;
let symbol = writer.register_symbol(&WriteSymbol::new(b"Added", SymbolType::Parameter, 1))?;
let mut rows = writer.begin_symbol(symbol, None)?;
rows.write(&[original_key], &[Value::NumberBits(42.0f64.to_bits())])?;
rows.finish()?;
writer.finish()?;
# Ok(()) }
```

Open read/write memory caps must match. AppendOptions sets aggregate admitted
output-plus-scratch bytes and source/output/scratch handles. Begin imports IDs,
metadata and records in bounded batches under the shared root. Initialization is
explicit so the returned owner can retry abort after any journal/import cleanup
failure. Register all new schemas before beginning new records, as for normal writes.
All existing write adapters remain available for new symbols. `append_symbol_id`
binds inherited domains/aliases; `append_uel_id` binds original raw IDs without label
lookup, including duplicate-label IDs. Inherited records/schema/UEL names and
acronym metadata cannot be replaced. Numeric/acronym policies apply only to new
writes; imported typed values preserve identity. Nodes and caller maps are session
state, not file metadata.

AppendStatistics reports source bytes, inherited counts, completed physical-record
rewrites and conservative resource admissions. WriteStatistics.published can be
true on a post-publication cleanup failure. Such a failure needs abort cleanup,
not an append retry or rollback. Any failed import/writing stream is terminal.

| Native inventory | Disposition |
| --- | --- |
| gdxOpenAppend | Modern replacement: explicit checked rewrite transaction with caller-selected new-output or source-replacement policy. Native in-place append/call-state ABI is unsupported. |
| Add new symbols and UELs | Implemented; original IDs and metadata retained; new IDs follow existing counts. Duplicate folded symbol names are errors. |
| Raw/zlib and byte order | Reader supports v7 LE/BE; append re-encodes to LE using explicit raw/zlib output. A minimal build cannot read/write zlib. |
| Strict/relaxed domains and aliases | Implemented; strict IDs retain direct alias identity, display names coexist and inherited sets validate new domains. |
| Atomic source replacement | Implemented for an exclusively owned regular source pathname; no concurrent-writer compare-and-swap or locking guarantee. |
| In-place append, replacement of existing symbols, crash recovery | Unsupported; no fallback that mutates the original before publication. |

See [transaction, ownership, source integrity and resource policy](decisions/0024-atomic-append.md)
for costs and limits. Each append rewrites the file; metadata scans may be quadratic.
No performance/RSS claim follows. Task 031 remains the M4 acceptance gate.

[Task 030 evidence](evidence/030.md) records fresh local and four-platform qualification.
