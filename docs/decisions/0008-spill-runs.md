# 0008 — Bounded temporary runs and merge, task 014

Task 014 follows merged task 013 (`4e0e790`). It implements scratch sorting only, not GDX writing, label join/domain extraction, persistent indexes or the M2 scale release.

## API and ownership

`Reader::spill_sort(&SpillOptions) -> Result<SpillSorter>` anchors memory accounting to the existing source/root budget. Caller supplies a trusted existing scratch parent and numeric run-memory, temporary-byte, open-run-handle and merge-fan-in limits. `SpillOptions::new(parent)` defaults to 256 KiB run memory, 512 MiB temporary logical bytes, nine run handles and fan-in eight. A run must hold one fixed bounded record; fan-in is 2–64 and needs input fan-in plus one output handle. Invalid options fail before creating scratch storage. Root quota failure never raises a cap. The source's already-open GDX handle and transient OS path internals are not spill run handles.

`push(label_bytes, opaque_u64_id)` accepts labels of at most 255 bytes, including empty/non-UTF-8/NUL. Sort order is exact `(label bytes, id)`; duplicates remain duplicates. Run buffers use fallible quota-charged exact-capacity storage and in-place unstable sorting. `finish(self)` flushes, merges, validates and returns a lending `SpillCursor`; it never publishes a successful output after a failed merge. `next()` returns a complete entry or EOF/error; previous entries are a partial result if a later integrity/read/cleanup check fails. Errors are terminal, with no implicit retry. `cancel(&mut self)` and cursor `close(&mut self)` report cleanup errors and allow explicit cleanup retry. Inspect/record `directory()` before consuming finish if manual recovery is needed.

Each job creates an exclusive private directory beneath the parent (0700 on Unix), and run files use create_new (0600 on Unix). Existing parent contents are never used as work files. Parent/private namespace must not be concurrently replaced or mutated by other actors. Fixed controls follow the recorded safe-stdlib allocator-OOM exception. Before path operations reserve 64 KiB plus eight times the bounded (at most 4096-byte) input parent length for retained/temporary path storage; canonical paths exceeding 32 KiB are rejected. No file count controls a path allocation.

## Temporary format and completion

Private version-one runs start with eight bytes `GDXRUN01`. Each record is `u16 LE label_length`, `u64 LE id`, and exact label bytes. Lengths 256–65534 are invalid. Sentinel length 65535 begins a footer: `u64 LE record_count`, `u64 LE FNV-1a-64` over every serialized record frame, and `GDXEND01`. Require exact trailer/EOF, count, checksum and nondecreasing tuple order. These noncryptographic integrity checks detect accidental corruption; they do not authenticate externally modified files. Disk input never controls allocation or recursion. Run reads/writes use explicit fallible 32 KiB buffers.

Write `.part`, flush the explicit buffer, sync the file and close it. Publish `.run` by an exclusive hard link, then unlink `.part`; no rename-overwrite fallback. Scratch filesystems must support same-directory hard links. No output consumer sees a new run before successful finalization. This is not a crash-recovery/durable spool promise: directory entries are not synced and process-kill recovery is not implemented.

## Resources and manifests

All run payloads, reader/merge headers, I/O buffers, retained state and path scratch compete for the root quota. A per-session resource counter reserves temporary logical bytes before buffering/writing and run handles before opening. Statistics expose current/peak reserved temporary bytes and open run handles, physical bytes read/written, input/returned records, published runs and merge passes. Temporary quota includes live inputs plus outputs during merge and buffered bytes pending writes. It excludes filesystem block/inode overhead; this is not a physical disk reservation or RSS cap.

Initial runs use monotonically checked numeric IDs. Each merge pass maps one contiguous input ID range to another contiguous output range, rewriting even a one-run remainder. Keep only start/count/next-ID integers and at most fan-in reader slots; no run/path vector or unbounded merge heap. Delete an input group only after the replacement run is finalized and its readers close. Cleanup enumerates known numeric names without collecting a directory. Track the one pending output so a publication collision is never treated as an owned completed file.

On any operation failure, close local readers/writers, release buffers and attempt every known owned cleanup. Preserve the primary error and a bounded secondary cleanup cause (`Error::cleanup_error()`) when cleanup also fails. Explicit cancellation/close surface cleanup failure; Drop cannot return errors and performs a final best-effort attempt. Never claim successful EOF if final cleanup fails. Do not delete unrelated scratch-parent files. TemporarySpaceExceeded, FileLimitExceeded and InvalidSpill distinguish quota/corruption failures; I/O causes remain inspectable.

## Required evidence

Force many initial runs/multiple merge passes and verify full independently specified order/multiplicity. Test byte lengths/IDs, empty input, short reads/writes, read/write/flush/publish faults, disk-full-style error, exact quota boundaries, allocation failure, invalid run header/length/order/count/checksum/trailer, truncation, cancellation and cleanup retry. Check high-water limits, no overwrite, no successful partial entry/finalization, full owner-drop resource release and unchanged M1/M2 readers. No dependency, throughput, OS-capped scale or task-015 implementation claim.
