# 001 — Development bootstrap

**Milestone:** M0. **Prerequisites:** none. **Goal:** provide a reproducible, dependency-minimal development shell for subsequent oracle and specification work.

## Scope

Create build/tooling structure only. Do not implement headers, a GDX reader/writer, placeholder public APIs, compression, indexing, or Polars integration. Do not add passing dummy tests to make the empty crate appear functional.

## Deliverables

- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and a minimal `src/lib.rs` with crate documentation and `#![forbid(unsafe_code)]`.
- `.gitignore` for `target/`, `.work/`, Python bytecode and generated benchmark data; never ignore tracked fixture expectations or task evidence.
- `tools/dev.py` implementing `check-bootstrap` and a clear nonzero error for unknown/unimplemented commands.
- `docs/development.md` with prerequisites, exact setup/check commands, selected toolchain/MSRV/edition, supported development platform scope, and the distinction between development and production dependencies.
- `docs/decisions/0001-toolchain-and-dependencies.md` with the version rationale and dependency policy.
- `docs/evidence/001.md` and the updated queue status.

## Implementation steps

1. Inspect existing tools before installing anything. Reuse supported installations. Keep downloads and research/build files under ignored `.work/` or outside the checkout. Use verified official distributions.
2. Select one exact stable Rust release available at execution time, pin it, and use that release as the initial MSRV unless evidence requires a different choice. Declare a compatible edition and `rust-version`. Do not pin a date string, floating `stable`, or an untested lower MSRV.
3. Create a single library crate named `gdxrs`, with **zero direct or transitive third-party runtime dependencies** at this stage. Do not add `build.rs`, CMake, FFI, proc macros, or unused optional dependencies. Leave compression feature design to its implementation task.
4. Give `tools/dev.py` a small standard-library CLI and subprocess wrapper. Use argument arrays, inherited trusted proxy configuration, explicit timeouts where relevant, and fresh report creation. This is a task runner, not a generic build framework.
5. Make `check-bootstrap` run formatting, Clippy, crate checking, and dependency inspection on the pinned toolchain. Include `--locked` where applicable. Treat missing tools and failed subprocesses as failures.
6. Document test absence honestly. Compilation and linting validate the scaffold only; there is no runtime GDX behavior to test yet.

## Acceptance checks

From the repository root:

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo tree --locked
python3 tools/dev.py check-bootstrap
python3 tools/dev.py not-a-command
git diff --check
```

The first five commands and `git diff --check` must succeed; `not-a-command` must fail with a clear message. `cargo tree` must contain only the root crate. Verify the toolchain used is the pinned release, the command report is fresh, and a failed child process cannot produce an overall success report. Add focused runner tests for failure propagation and report freshness if needed, without claiming these are library tests.

Run `check-bootstrap` a second time to demonstrate repeatability. Review the diff for generated files, user configuration or credential values. Record all results and the absence of GDX tests.

## Completion gate

The scaffold builds reproducibly, no runtime dependencies were introduced, and the evidence record is complete. This unlocks task 002; it does not establish GDX readiness or authorize decoder implementation.
