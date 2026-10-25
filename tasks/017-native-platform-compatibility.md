# 017 — Native macOS and Windows reader compatibility

Milestone: M2 portability follow-up, before M3 at the user's request.
Prerequisite: 016 complete and merged. Status: complete.

Run the existing reader, metadata, slicing and spill suites natively on Linux x64,
macOS Apple Silicon and Intel, and Windows x64/MSVC using the pinned Rust/MSRV.
Run default-feature tests in the optimized release profile as well.
Compile-only/cross-compilation is not evidence of native correctness. Add a
repeatable pull-request/main CI matrix with immutable action pins, read-only
permissions, explicit target checks, exact feature-suite counts and retained
machine reports. No skipped feature suite or weakened fixture/hash expectation.

Make the standard reader-check command runnable without importing Unix-only
oracle/scale tooling on Windows. Keep unsupported OS-level scale/oracle commands
explicitly separate; do not imply Linux's RSS/cap evidence applies elsewhere.
Ensure Cargo tests work on a clean checkout without a pre-existing .work directory,
and preserve fixture bytes/text line endings across Git platforms.

Exercise Unicode file/scratch paths, interleaved and concurrent positioned reads,
source change/truncation, retained owners, exclusive hard-link publication, no
clobber and explicit cleanup with closed handles. Existing failure-injection and
bounded-budget tests must remain enabled. Native frozen-corpus comparison must
validate complete metadata and all 141,358 public records in ten fixtures.

Record native workflow run/job IDs, exact tested revisions, runner/host triples,
compiler/features, counts/results and unavailable cases in docs/evidence/017.md.
Mark complete only after actual native jobs pass. If hosted runners/credentials
are unavailable, preserve the implementation and identify the blocker without
claiming platform compatibility. No new production dependency, unsafe code, writer,
index or parallel decoder; concurrency here tests independent existing readers.

Completion: [native CI evidence](../docs/evidence/017.md) and [machine results](../docs/evidence/017-results.json). Functional/lifecycle qualification covers both Mac architectures and Windows x64; Linux-only OS-cap scope remains unchanged.
