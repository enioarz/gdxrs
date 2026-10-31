# M4 API and acceptance status

Task 031 has passed its API/correctness/scale checks for the measured scope;
review is pending. General adapter throughput remains below the provisional
target; see [031 evidence](evidence/031.md). The [current inventory](m4-api.csv) maps every function
and property accessor in pinned native `gdxapi.yaml` to concrete Rust methods,
exact semantics, existing evidence and explicit exclusions. It supplements the
frozen M0 planning inventory, which is deliberately unchanged. The audit checks
98 entries (92 functions and six accessors), including all 74 M1–M4 required
entries. Required entries cannot be marked excluded or quietly moved out of M4.

The core implements typed streaming equivalents. Native-equivalent means the
specified data/metadata semantics, not source or ABI compatibility. Adapter-equivalent
means an explicit Rust policy or streaming operation substitutes for legacy
call-state, callback or error-list behavior. Caller maps and text nodes remain
session state, never new on-disk metadata. Error callbacks preserve every rejected
row and original ordinal; native duplicate coalescing is not substituted for this
contract. See each inventory row's linked evidence for differences in ordering,
identity, empty rows, values, aliases, domains and mutation phases.

Legacy slice dimension reduction/remapping, C ABI/global last-error state, tracing,
native DLL version and external converter execution remain explicit M8 adapter
work. The Rust core exposes raw-ID selection and explicit label/mapped views;
it does not claim those excluded native interfaces. Completing task 031 is not
full C API parity or release of the remaining roadmap.

## Acceptance recipe

`python tools/dev.py m4-api-check` verifies the native roster and implementation
references. `python tools/dev.py m4-check --profile smoke` calibrates all six
workload families against independently enumerated Python expectations and the
pinned C++ reference, executes eight benchmark configurations with three alternating
Rust/native trials, and validates growing small outputs with both implementations.
Smoke explicitly reports `m4_scale_qualified: false`. The separate fixed-size
`tools/m4_boundary.py NATIVE_SNAPSHOT` check independently enumerates 131,073
inputs per family to cover key wrap and diagnostic ordering across multiple
65,536-row blocks. It is an additional calibration gate, never an oversized case.

The scale profile requires committed measured sources. It snapshots binaries and
source hashes, enforces a 256 MiB shared library root and 512 MiB Linux address-space
cap, measures wait4 RSS and process I/O, and admits bounded scratch/handle quotas.
All generated outputs receive complete ordered key/value-bit and original UEL-ID
validation, plus SHA-256, before retirement. Dictionaries are streamed without a
caller token per label. Append imports through one reader/writer root and checks
source hashes before/after. The eager native reference and dictionary validator
use a separately declared 16 GiB cap; they do not demonstrate Rust containment.

| Family | Growing counts | Independently oversized quantity at largest size |
| --- | --- | --- |
| Unsorted / mapped / append | 4.5M / 18M | 20 keys + five values, 2.16 GB logical; raw output >2 GiB |
| Strings | 13M / 52M | Two labels + five values, 2.496 GB logical; raw output >2 GiB |
| Sparse mapped dictionary | 9M / 34M labels, five rows | 63-byte labels + framing, 2.176 GB independently of records |
| Deferred errors | 4.5M / 18M accepted data rows | 15M diagnostic callbacks, 2.385 GB, plus independently oversized accepted output |

The largest physical **and** logical workload in each family must exceed four
times the process cap. RSS growth is limited to the existing fixed 64 MiB allowance.
Error-stream size, dictionary size, all six families and both sizes are separately
mandatory. Small or compressed artifacts cannot substitute for these thresholds.

## Performance interpretation

The native timing reference is a development-only composite: native raw
registration/encoding, explicit eager C++ `vector`/`std::sort`, string lookup and
sparse `unordered_map` translation. It stores every staged row and therefore is
not itself an out-of-core implementation. It avoids mislabeling hidden native
legacy adapter buffering as separately timed phases. Native deferred diagnostics
are explicitly filtered and hashed in the composite so the compared work preserves
the Rust lossless contract. Both producers' outputs receive both validators.

Timers surround operations, never individual rows. Ingestion includes initial run
creation; merge preparation includes the external sort's complete merge passes;
replay includes final-run decoding and encoding. String remapping includes its
registration sort and spool-to-raw staging. Append copy includes full input
validation and rewrite/copy. Zero phases mean the operation is absent for that
family. Finalization includes metadata serialization and publication/close.
The total measured wall time remains available alongside the phase times.

The benchmark uses warm OS caches and fresh outputs. Rust uses miniz level 1,
native zlib's default level 6. Rust finalization includes `sync_all`; native
`gdxClose` gives no equivalent durability guarantee. Results must retain these
option/cost differences. Registration and per-row sparse lookups may remain
slower than eager native tables. No general native parity or macOS/Windows memory
claim follows from Linux smoke timing. Reader/M3 gates and four-platform native
Rust CI remain separate required checks.
