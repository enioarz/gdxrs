# Native upstream baseline driver

Development only, C++17, linked statically to the already pinned upstream GDX/zlib archive. This directory is intentionally separate from fixture generation: adding a benchmark cannot silently change fixture expectations. `tools/benchmark.py` records hashes for this source, its CMake file, the runner and source pins.

CLI: `gdxrs-bench {write|scan} FILE WORKLOAD`. Supported workload names/parameters are fixed in `driver.cpp` and independently described in Python:

| Workload | Rows | UELs | Symbols | Dimensions | Fields | Compression |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `parameter-uncompressed` | 1,000,000 | 1,000 | 1 | 2 | 1 | 0 |
| `parameter-compressed` | 1,000,000 | 1,000 | 1 | 2 | 1 | 1 |
| `variable` | 100,000 | 1,000 | 1 | 2 | 5 | 0 |
| `metadata` | 2,000 | 100,000 | 2,000 | 0 | 1 | 0 |

For zero-based row ordinal `r`, indexed keys are `(r/1000+1, r%1000+1)` with integer division. Values are `r+0.25`; variable rows additionally contain `-r-0.5`, `-100.25`, `100.75`, `1.0`. Metadata rows are one scalar per symbol with the same ordinal-based value. Labels are `uN`; metadata labels use `uN_` padded to 63 bytes with `x`. Symbols are `data` for single-symbol workloads, otherwise `sN`; descriptions are `benchmark`.

The scan sink hashes every key and returned field: canonical little-endian u32 keys then binary64 bits, bytewise FNV-1a-64 with offset basis `14695981039346656037`, prime `1099511628211`, modulo 2^64. Python computes the expected ordered stream directly from these formulas before the native file is written. This is deterministic benchmark validation, not a cryptographic integrity mechanism. The writer reports no digest; its output is validated by a separate complete scan outside its recorded write interval.

Scanning verifies every UEL string and symbol's dimensions/type/user information/count/explanation separately in the metadata interval. Timed raw scans exclude that metadata loop but include digest work and raw API overhead. Write timing includes open, registration, encoding and finalization with their subintervals recorded. JSON contains CPU time and process peak RSS; RSS is not exclusive library allocation and may include child-launch/runtime overhead. Failures exit nonzero. Direct FILE output is overwritten; the Python runner uses only uniquely owned generated paths.

No row vector, Arrow object or semantic text dump is accumulated by the native sink. Upstream may eagerly retain metadata; measuring this reference does not meet the future Rust memory contract.
