# 0012 — M3 sorted writer contract and first transaction

M3 starts on merged task 017 (`34577c3`). Task 018 implements valid empty v7 creation
and failure-safe publication only. Tasks 019–023 add bounded encoding, metadata
registration, sorted records, full finalization/interop and scale acceptance. This
note specifies ownership/resource rules before code; no panicking future stubs.

## Public task-018 surface

`WriteOptions { memory_budget_bytes: usize, maximum_output_bytes: u64 }` defaults
to 64 MiB and i64::MAX; the root minimum is 1 MiB. `Writer::create(path, options)`
creates a standalone budget. `Reader::write_options()` derives options with the
reader's existing root cap. `Reader::create_writer(path, options)` requires matching
memory caps and shares that root; it does not open another GDX source. Writer
retains the budget after Reader drop without retaining the input file itself.

`Writer::{finish(&mut self), abort(&mut self)} -> Result<WriteStatistics>` permits
explicit cleanup retry. `statistics()`, `budget_statistics()` and `temporary_path()`
are inspectable, including after failure. `WriteStatus::{Open, Failed, Aborted,
Complete}` and `WriteStatistics { status, output_bytes, bytes_written, published }`
distinguish terminal state and publication. `output_bytes` is the highest admitted
logical extent, including buffered bytes (not current retained disk space).
`bytes_written` counts actual writes, including index backpatches/short writes.
The output cap bounds final logical extent; metadata scratch in later tasks gets
its own aggregate quota and handle limit. Never relabel quota accounting as RSS.

Initial output is canonical little-endian, uncompressed v7 with fixed audit and
producer bytes `gdxrs`, zero symbols/UELs/acronyms, one empty set-text entry and no
optional domain section. It is a useful valid file, not a claim of record-writing
support. Future methods are absent until implemented. Duplicate/out-of-order raw
rows will be rejected; registration keeps original one-based IDs and byte/case
semantics. Unknown bounds use format-valid conservative widths. Phase transitions
and borrowing symbol ownership prevent concurrent mutation or publication of an
abandoned/failed symbol. Future strict-domain checking remains bounded.

## Resource and destination ownership

One fallible exact 32 KiB output buffer, a 16 KiB fixed control allowance and
64 KiB + 8 × bounded input-path encoded length compete for the root budget.
Input/canonical paths are at most 4096 encoded bytes and retained path capacities
at most 16 KiB; validate before unbounded path cloning. Fixed standard owner/path
allocator-OOM limitations are the existing documented safe-stdlib exception.
No unbounded buffer, file-sized mapping, output collection or hidden BufWriter.

The destination's parent must already exist and be trusted/not concurrently
replaced. An exclusive same-directory temporary regular file uses mode 0600 on
Unix; Windows inherits parent ACLs. Never follow/replace an existing destination,
including a symlink or an entry created after open. All output admission checks
precede buffering/writes, with checked offsets/counts. The writer buffer is flushed
before checked seeking to the uncompressed major index. Loop short writes and
retry Interrupted; zero writes/error/seek/sync failures are terminal.

`finish` appends valid metadata, patches the index, flushes, syncs and closes the
file before publishing by exclusive hard link. Same-filesystem hard-link support
is required. Existing paths are never overwritten, and no overwrite/copy fallback
is hidden. Directory durability/crash recovery is not promised; directory entries
are not synced in this first transaction. Future durability policy must be explicit.

Successful publication sets published=true immediately. Removal of the owned temp
name follows; if that fails, finish reports Failed while published remains true
and the valid destination remains intact. Complete requires successful temp cleanup.
A later abort retries only owned temporary cleanup, never deletes the destination.
Pre-publication failure keeps published=false and attempts owned cleanup after
closing handles. Preserve primary error and bounded secondary cleanup cause.
Repeated finish on Complete returns the saved statistics; failed finish never
retries data operations. Abort on Open is terminal Aborted; abort after Failed
allows cleanup retry while retaining the original terminal error. Drop closes and
attempts cleanup only, never syncs/publishes or implies successful finish.
