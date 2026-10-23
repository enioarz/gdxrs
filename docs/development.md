# Development

The bootstrap is a single dependency-free Rust library. No GDX functionality is implemented yet. Cargo does not invoke Python, CMake or a native library.

Use Rust **1.99.1**, including Cargo, Clippy and rustfmt. `rust-toolchain.toml` selects it when using rustup. The initial MSRV is also 1.99.1; lower releases are not claimed. Edition 2024 is supported by this release.

Install through official rustup, preserving TLS and checksum verification:

```sh
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy
```

Alternatively install the official component archives into a local prefix. Verify each against its `xz_hash` in the official 2026-10-15 stable distribution manifest. The current cloud instance uses `.work/toolchains/1.99.1`; activate it for each shell:

```sh
export PATH="$PWD/.work/toolchains/1.99.1/bin:$PATH"
```

Python 3.11+ is needed only for development commands (`tomllib` is standard-library). Run from the repository root:

```sh
cargo generate-lockfile
python3 tools/dev.py check-bootstrap
python3 -m unittest discover -s tools/tests -v
```

Fresh JSON reports are saved under ignored `.work/reports/`. Unknown commands fail; future oracle/fixture commands are not implemented until their tasks are completed. The runner checks exact tool versions, formatting, compilation, Clippy and absence of dependencies. It does not interpret a zero-test crate as GDX functionality.

Current validation is on Debian 13, Linux x86_64. Other platforms are untested. Later oracle tasks require a C++17 compiler and CMake as development dependencies only. Large generated datasets and native sources/builds belong under `.work/`, never in production Cargo dependencies.
