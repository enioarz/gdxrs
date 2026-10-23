# 002 — Pinned upstream oracle

**Milestone:** M0. **Prerequisites:** task 001 complete. **Goal:** build and exercise an independent upstream reference used only by development tooling.

## Deliverables

- `tools/oracle/sources.json`: exact upstream, compression dependency and any required submodule revisions/checksums, repository URLs, build options and licenses.
- `tools/oracle/CMakeLists.txt` and a small C++ oracle driver using the upstream API; source stays in `tools/oracle/` and artifacts under ignored `.work/oracle/`.
- `tools/dev.py oracle-build` and `oracle-self-test`.
- `docs/oracle.md`: setup, driver commands, build isolation, semantic dump grammar and a licensing/provenance record with required notices.
- `docs/evidence/002.md` and the updated queue status.

## Implementation steps

1. Fetch `GAMS-dev/gdx` at `9e2a8c8932412010cd7d2f94727d1740143bf3da`. Verify the full checked-out SHA before building. Inspect pinned build instructions and dependency declarations; pin zlib and any required submodule separately. Do not silently use a moving branch or a different local library.
2. Build only the upstream library and the driver needed for this task. Determine build options from that source revision. Record compiler, CMake, platform, build type and codec revision. Bound parallelism using available memory/CPU; no production Cargo build hook may invoke this build.
3. Define a driver that can create one deterministic smoke file, inspect a supplied file, and stream a semantic dump to stdout or a requested output file. Surface upstream errors with failing exit codes. Specify the driver CLI in `docs/oracle.md`; later tooling must invoke that same CLI.
4. Use a versioned, line-oriented dump with explicit symbol/metadata records and a streamed sequence of data records. Define escaping, integer widths, byte-string representation and special-value encoding. Hex-encode label bytes and ordinary IEEE-754 bits; represent GDX specials/acronyms with explicit tags and IDs. Do not serialize ordinary numbers as rounded decimal text or collapse special classes to NaN.
5. Cover metadata via the upstream public API: file information, UEL order, symbol type/dimension/subtype, domains, comments, set text, aliases, acronyms and raw record ordering. If the API normalizes a value, record that behavior; do not call the dump a lossless view of inaccessible on-disk bits.
6. Make builds repeatable and isolated from arbitrary search paths. A changed pinned dependency must invalidate the build identity. Keep required upstream notices with retained source-derived material.

## Smoke experiment

Create a small uncompressed fixture and a compressed fixture through upstream. Each contains at least a set, a two-dimensional parameter and a variable with distinct values in all five fields. Include one nontrivial UEL registration order and selected special values. This smoke fixture is development validation; task 003 creates the reviewed permanent corpus.

Before running, author the expected symbol properties and ordered records directly from the generator inputs. Compare the driver dump to those expectations. Successful upstream write→upstream read alone is not enough if the driver accidentally omits fields. Check a known malformed/truncated input produces a failure; document upstream behavior if its error checking is permissive.

## Acceptance checks

```sh
python3 tools/dev.py oracle-build
python3 tools/dev.py oracle-self-test
python3 tools/dev.py check-bootstrap
git diff --check
```

- Build succeeds at the pinned revisions; repeat `oracle-build` to verify safe reuse of matching outputs.
- Both smoke compression modes execute with nonzero record counts and complete expected fields. Record exact counts and expected-versus-observed comparison results.
- Missing library, bad input path and driver errors are propagated as nonzero results. A fresh failed run must not reuse the previous run's successful report.
- Cargo still has zero runtime dependencies and no native build/link requirements. Demonstrate `cargo check --locked` with oracle artifacts unavailable using an isolated Cargo target directory or equivalent supported setup, without deleting user files.

## Completion gate

A source-pinned reference actually builds and runs, semantic output has a specified grammar, and successful/failing operations are distinguishable. Document unsupported oracle observations; they must not become guessed expected results. Unlocks task 003.
