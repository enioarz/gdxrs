# 0028 — Optional SHA-256 dependency audit

The architecture plan permits one audited digest dependency for persistent source
identity. Task 033 uses pinned RustCrypto `sha2 = 0.10.9`, optional behind `index`,
with default features disabled and **force-soft** enabled. No assembly, SIMD codec,
OS hashing, subprocess, custom cryptography or default index feature is introduced.
Default builds still use only miniz_oxide/adler2; minimal builds retain zero runtime
dependencies. The optional graph is intentionally visible rather than hidden.

| Additional locked package | Version | License |
| --- | --- | --- |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |

`version_check` is build tooling; `libc` is a target-specific cpufeatures dependency.
The lock graph contains ten additional packages; it does not mean all are linked
on every platform. RustCrypto's generic-array/digest stack is larger than a custom
single-file hash, but avoids implementing cryptography in this project. Exact
registry checksums are retained in Cargo.lock. Metadata/feature audits in
`tools/index_dependencies.py` reject unplanned versions/features and retain the
existing default/minimal dependency audit unchanged.

Source audit: Sha256 owns fixed inline state and a fixed block buffer, with no
input-sized allocation or growing collection in update/finalize. The force-soft
selection in sha2's sha256/sha512 dispatch chooses the portable implementation;
cpufeatures remains a declared transitive dependency but hashing does not use its
runtime hardware dispatcher. Generic-array contains low-level layout code; this
is reviewed dependency implementation, not permission for unsafe code in gdxrs,
which still forbids it. Software SHA-256 is checked with the standard `abc` vector;
full source digests are independently compared with Python hashlib during scale
validation. No unkeyed digest is described as authentication of arbitrary authors.

Digest context/control state is covered by the index's fixed root reservations;
the source read buffer is a charged 32 KiB Buffer. Page hashing consumes fixed
1024-byte pages. Allocation/feature changes require a refreshed audit; std/alloc,
assembly and hardware feature expansions are not silently enabled.
