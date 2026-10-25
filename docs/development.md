# Development

The crate implements the M1 bounded serial GDX v7 reader. Cargo invokes no Python, CMake or native library. Default compression uses the audited pure-Rust `miniz_oxide`/`adler2` pair; `--no-default-features` has no third-party dependencies and explicitly rejects compressed files.

Use Rust **1.99.1**, Cargo, Clippy and rustfmt. `rust-toolchain.toml` and the MSRV select that version; lower releases are untested. Install through official rustup with TLS/checksum verification:

```sh
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy
```

Alternatively use official component archives verified against their `xz_hash` in the official 2026-10-15 distribution manifest. This cloud instance has a local prefix/cache:

```sh
export PATH="$PWD/.work/toolchains/1.99.1/bin:$PATH"
export CARGO_HOME="$PWD/.work/cargo-home"
```

Run from the repository root. Python 3.11+ is development tooling only.

```sh
python3 tools/dev.py reader-check
python3 tools/dev.py m0-check
```

`reader-check` checks formatting, locked compilation, Clippy with warnings denied for default/no-default features, the audited dependency graphs, and default/no-default/all-feature Rust suites. It executes the streaming dump example against all ten frozen expectations, including full metadata and 141,358 public records. Unit tests include I/O fault injection and budget accounting; integration tests include exact projections/limits, malformed input, a deterministic byte-mutation smoke test and real truncation. A compile-fail doctest checks batch borrowing. This is not a fuzz campaign or larger-than-RAM performance certification.

`m0-check` rebuilds/checks the pinned C++ oracle, fixtures, format coverage, initial contract blueprint, development tests and retained baseline identity. The user confirmed M0 is reviewed/released; the stale local pending review is historical. M0 blueprint checks do not execute Rust reader behavior. The legacy `check-bootstrap` command now enforces the audited M1 dependency graph rather than its historical zero-dependency bootstrap policy.

Each command creates a fresh report in ignored `.work/reports/` and propagates failures. Do not regenerate Cargo.lock or fixture expectations as a routine workaround. See [task evidence](evidence/011.md) for exact executed counts, commands and limitations. [Oracle setup](oracle.md) requires C++17 and CMake only for development. Native sources/builds and generated datasets stay under `.work/`.

Current validation is Linux x86_64, Debian 13. Windows positioned I/O is implemented but untested here; native big-endian hardware, >4 GiB inputs and OS-capped oversized workloads remain unrun. Exact raw-ID and small exact-label selections plus bounded label pages are implemented in M2; spill-backed exact-label requests and observed-domain enumeration are implemented; the OS-capped scale gate remains pending; writing belongs to M3/M4.

`python3 tools/dev.py label-pages-check` builds a bounded development-only 8,192-label probe in both stored and zlib block forms and verifies its full output with both the pinned native oracle and the Rust dump. A label crosses the 32 KiB decoded boundary. Run oracle-build first if the development oracle is unavailable. Core Rust tests exercise the same declared boundary input without requiring native/Python tooling; the normal frozen corpus is unchanged. See task 013 evidence for request/cursor/cache checks and exact executed counts.

Task 014 adds private scratch-run tests to the normal reader-check suites. Focused `cargo test --locked spill:: -- --nocapture` exercises small run buffers, multiple merge passes, quota peaks, partial/interrupted I/O, disk-full-style faults, corruption, cancellation and cleanup retry; it needs no native library. OS-capped large input qualification is still task 016. Scratch-run checks are not Rust GDX-writer interoperability tests.


Task 015 is included in `reader-check`. Focused checks:

```sh
cargo test --locked --test reader large:: -- --nocapture
cargo test --locked --lib spill::tests:: -- --nocapture
```

These execute frozen label/record comparisons, non-leading membership/limits,
20-dimensional rows, small-versus-spill equivalence, a request payload exceeding
the logical root quota, multi-pass merges, root/disk denials and injected join,
membership, domain-output and cleanup errors. Logical peak reservations are
recorded in task 015 evidence. They do not establish OS-capped RSS, independently
oversized metadata or a throughput improvement; those remain task 016.


Task 016 adds development-only Linux scale tooling:

```sh
python3 tools/dev.py m2-scale-check --profile smoke
python3 tools/dev.py m2-scale-check --profile scale
```

Smoke validates the generator against independent Python recipes and the pinned
native reader, then exercises capped sinks on small files. It never qualifies M2.
Scale requires committed measured sources and at least 18 GiB free workspace,
runs all regression gates, generates separate record/metadata files exceeding
2 GiB and measures both increasing input sizes under fixed 256 MiB root / 512 MiB
hard address-space limits. It can take several minutes and retains inputs/reports
under `.work/scale/`. Do not run two scale invocations concurrently. No Python/C++
production dependency is added. Read [the protocol](decisions/0010-m2-scale-qualification.md)
for exact containment, digests, cache treatment and unmeasured-platform limits.
`Reader::io_statistics()` now includes successful stream-fill decoded bytes and
buffer counts, including read-ahead/repeated metadata traversals. These counts do
not equate selected rows with skipped source I/O.
