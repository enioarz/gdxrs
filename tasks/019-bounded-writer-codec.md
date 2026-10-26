# 019 — Bounded raw/stored/zlib output streams

Milestone: M3. Prerequisite: 018 complete. Status: pending.

Add section encoder byte/scalar/string primitives and independent <=32 KiB decoded
blocks, with stored fallback when zlib is not shorter. Extend the existing audited
miniz_oxide no-alloc use only after examining actual compressor state/API/features
and recording size/transitive/license effects. No new dependency without rationale.
Compression-disabled builds preserve uncompressed writing and explicitly reject
requested zlib. Check encoded bounds, backpressure, final blocks, checksums and
error propagation without hidden Vec growth or compressed in-place patching.

Test independent fixed vectors and pinned-native/Rust decoding for empty/one-byte/
full/final blocks, boundary-spanning fields/strings, incompressible data, both byte
orders if offered, quotas, allocation/I/O/finalization faults and feature/platform
matrix. Keep temporary-file publication and root accounting from 018. Record 019
runtime evidence; full symbols/registration remain later tasks.
