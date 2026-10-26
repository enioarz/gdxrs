# 0013 — Bounded v7 output sections and encoder allocation audit

Task 019 builds on merged task 018 (`3fa50b7`). Output remains little-endian;
big-endian writing is not offered. `WriteOptions::compression` selects
`WriteCompression::{None, Zlib}`, default None. Zlib uses level 1 for a fixed,
fast baseline; no throughput claim precedes task 023. Reduced builds reject Zlib
before filesystem changes. The raw bootstrap/index stay uncompressed. Each
metadata section ends its own independent block stream before recording the next
physical offset; only the uncompressed major index is backpatched.

A private reusable section encoder accepts borrowed byte/scalar/string slices.
Compressed input is buffered in fallible 32 KiB storage. Every nonempty block is
an independent checksum-bearing zlib stream, selected only if strictly shorter
than its input; otherwise use stored framing. Header length is big-endian u16,
regardless of field byte order. Empty sections emit no block. One fallible 32771-
byte encoded buffer holds a complete header+payload, admitted to the output sink
in one operation. There is no compressed patching, empty terminal block, growing
output Vec, or retry after an encoder/sink error. Flushing a section preserves
its allocated buffers/state for reuse, but resets the zlib dictionary/checksum
for every block. Callers must finish before switching sections or seeking.

## Pinned miniz_oxide 0.9.1 audit

Unlike its inflater, the deflate module is gated behind `with-alloc`. Enable that
single feature only with the existing `compression` feature. Versions/checksums,
licenses (miniz_oxide MIT OR Zlib OR Apache-2.0; adler2 0BSD OR MIT OR Apache-2.0)
and the two-dependency graph stay unchanged. No std/SIMD/Serde or native backend.
Dependency-audit tooling must require exactly this feature and reject others.
Decision 0004's no-alloc encoder assumption did not apply: that decision audited
only the inflater, which still uses the same fixed-slice API.

Audited files: miniz_oxide `src/lib.rs`, `deflate/{core,buffer,stored,zlib}.rs`.
`CompressorOxide` owns a fixed inline LZ code buffer and five fixed heap objects:
Huffman counts/codes/sizes (4320 bytes), LocalBuf (85196), dictionary (33026),
next-chain (65536), hash (65536): 253614 payload bytes on qualified native targets.
On wasm the inline 65536-byte code buffer is boxed too. Conservatively reserve
320 KiB for private heap state on all targets, plus a fallibly allocated one-
element `Buffer<CompressorOxide>` for the visible state. Enforce a 128 KiB ceiling
on visible state size before constructing it. The constructor allocates fixed
boxes (dictionary construction uses fixed-sized vec-to-box conversions); neither
compression nor native-target reset allocates or grows a container. No clone or
convenience Vec encoder API is used. The fixed state follows the already documented
safe-standard-owner allocator-OOM exception in decision 0003: quota denial is
recoverable and precedes construction, but actual allocator OOM in these private
boxes can abort. Recoverable failure for every allocation is not claimed.

This retains the existing audited codec instead of adding a dependency, forking
it or implementing DEFLATE. Fixed state admission, fallible data allocation,
block/framing bounds and sink backpressure receive dedicated tests. Platform CI
checks construction/reset on native Windows/macOS/Linux stacks. Record and full
metadata writer support remain tasks 020–022; RSS/throughput acceptance is 023.
