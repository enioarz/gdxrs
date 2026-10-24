# Upstream oracle small-profile baseline

Executed baseline for native upstream GDX, **not Rust performance**. Measured code revision: `2941d6641b77eea03fc408f521e91b8fc38c4646`. Full raw trials, file hashes, source/build hashes, expected digests, CPU timings, RSS, encoded/nominal-decoded throughput and dispersion are in [oracle-small.json](oracle-small.json). Its native driver is development-only and uses the same pinned GDX/zlib archive as the fixture oracle.

## Environment and method

Linux x86_64, GCC 14.2.0, CMake 3.31.10, Python 3.12.14; CPU model `AMD EPYC 9V74 80-Core Processor`. The host exposes 5 CPUs with a 4-CPU cgroup quota and a 32 GiB memory limit. These are a shared cloud runner's observations, not a controlled bare-metal hardware specification. Storage hardware/cold-cache behavior was not characterized.

Each native invocation is a fresh child process. One setup file and one read warmup precede **five measured reads and five measured sorted writes per workload**. Every one of the 20 measured written files is subsequently scanned and validated outside its recorded write interval. Total validated scan invocations: **44** (4 warmups + 20 measured reads + 20 write validations); total writes: **24** (4 setup + 20 measured). The OS cache is naturally warm; there is no cold-cache or indexed-query claim.

The scan sink processes keys and **every returned value field** into an ordered FNV-1a-64 digest, compared against a Python calculation from the input recipe before file generation. It does not format a semantic text dump or materialize result vectors. Counts alone are insufficient. Metadata timing additionally verifies every UEL byte string and every symbol's name/type/dimensions/count/subtype/explanation. Open time includes upstream's eager metadata load. Raw scan time below excludes the separately measured open/metadata intervals but includes digest work. Full write time includes opening, registration, encoding and finalization; subintervals and total CPU time are retained.

`getrusage(RUSAGE_SELF)` records process high-water RSS (in bytes after platform-unit conversion). This includes child-launch/runtime overhead and is not exclusive library allocation; the inherited launch floor can dominate small cases. No library quota, memory cap or out-of-core behavior is validated by this baseline. The native reference can eagerly retain metadata.

## Medians (milliseconds)

| Workload | Rows | Open | Metadata validation | Raw scan + digest | Full sorted write |
| --- | ---: | ---: | ---: | ---: | ---: |
| parameter-uncompressed | 1000000 | 0.085 | 0.032 | 34.686 | 31.953 |
| parameter-compressed | 1000000 | 0.127 | 0.046 | 44.481 | 268.620 |
| variable | 100000 | 0.083 | 0.032 | 9.815 | 9.490 |
| metadata | 2000 | 5.649 | 7.872 | 0.280 | 35.322 |

The JSON retains all five observations plus minimum/maximum/sample standard deviation for each interval. Input recipes and timing boundaries are specified in [tools/bench/README.md](../../tools/bench/README.md). Compression settings are 0/1, not a cross-codec quality-level equivalence. Decoded throughput uses nominal raw key/float payload bytes, not Rust enum allocation size or label materialization. These measurements provide a comparison method for later work, not a portable speed guarantee.

## Reproduce and validate

```sh
export PATH="$PWD/.work/toolchains/1.99.1/bin:$PATH"  # or activate the pinned rustup toolchain
python3 tools/dev.py oracle-build
python3 tools/dev.py benchmark-oracle --profile small
python3 tools/dev.py m0-check
```

The benchmark runner writes a fresh ignored `.work/bench/runs/<run-id>/baseline.json` and preserves the four input artifacts; reviewed results can then be deliberately retained here. It rejects missing trials, incorrect counts/ordered digests, stale measured-source identities, tampered input bytes and inconsistent summaries. An isolated native CMake target builds only the benchmark driver against the pinned archive; Cargo stays independent. A fresh checkout without the ignored input artifacts must rerun the benchmark rather than claim current-instance validation from this report.

The original pending review made `m0-check` exit 2 even when automated checks passed. The user subsequently confirmed M0 was reviewed and released; decision 0003 resolves that stale snapshot. Tooling does not fabricate an independent review.

## Large profile recipe (deferred and unrun)

A later dedicated scale run should parameterize the same streaming generator/sink for 250,000,000 two-key parameter records (at least 2.5 GB uncompressed), register at least 250,000 required UELs, and independently generate a metadata-heavy file with 12,000,000 labels padded to 63 bytes. Run the intended Rust reader under the planned 256 MiB library / 512 MiB OS cap, consume outputs as a stream and validate counts/digests; measure the native reference separately and record any eager-metadata limitation. Disk quota/free-space and representation limits must be checked before generation. Both record and metadata dimensions must exceed the cap independently.

That recipe is documented for later scale tooling; M0's CLI deliberately exposes only the implemented `small` profile. No large/memory-capped result, Rust comparison, sidecar cost or checkpoint/parallel speedup has been measured. Ordinary microbenchmarks are not a substitute for those later gates.
