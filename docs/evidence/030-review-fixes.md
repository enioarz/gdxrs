# Task 030 — PR #25 review follow-up

Baseline: `475e43b489cefb279510e3d8ab3346c603ae06d1`.
Implementation: `8fde59cd6ef2fa85b8fc2f90ffa23325f4941e6f`.
Working directory `/workspace/gdxrs`; pinned Rust 1.99.1 and native sources unchanged.
[Source-bound results](030-review-fixes-results.json) retain fresh reports and hashes.

## Inherited duplicate labels

Reproduced the InvalidSpill failure on the baseline: import preserves two identical
labels, but adding unrelated `cc` through string registration fails. The bulk
registration seed incorrectly required every existing label's canonical name
ordinal to equal its raw ID.

The seed now retains every inherited raw ID independently. New exact-byte label
references resolve to the lowest existing matching ID. New labels receive IDs
after the complete inherited dictionary, in first-request order. No source record
IDs are remapped, dictionary slots collapsed or dense table added. Existing bounds,
frame integrity, disk quotas, handles, spill sorting and callback order remain in use.

The new regression checks four string/mapped scenarios: unrelated labels only,
or references to the duplicated label plus a case-distinct new label. It verifies
new IDs, sparse caller IDs including zero/i32::MAX, repeated identical registration,
absence of an implicit mapping for the second inherited ID, retained exact-byte
labels and independent raw writes using that second ID. Source bytes remain
unchanged before successful publication. The regression fails before this fix.

## Terminal InvalidCount and cleanup diagnostics

This issue was already fixed in `54d65ee`, before the baseline/current reviewed
head: import catches errors while the borrowing SymbolWriter is alive and calls
fail_import before dropping it. No additional production change is needed for
this finding. The previous evidence includes in-flight I/O/source-change cases.

Added the exact reported InvalidCount case by making the source catalog declare
one physical row while its data stream contains two. The regression passes on
the baseline, before the duplicate-label production change. Both begin_append
and subsequent finish return the same InvalidCount/error display, with identical
cleanup diagnostics. It tests normal cleanup and injected cleanup denial, retained
abort ownership and retry, blocked publication and unchanged source bytes.

## Fresh checks

At the implementation revision, all commands exit 0:

- `cargo test --locked append_ -- --nocapture`: 19 append tests pass; the focused
  invocation intentionally filters other unit and integration tests.
- `python3 tools/dev.py reader-check`: default/all each 173 unit, 40 integration
  and eleven doctests; minimal 165, 39 and eleven. Ten frozen fixtures compare
  141358 records. Formatting, Clippy and dependency/feature checks pass.
- `python3 tools/dev.py writer-check`: all existing native/Rust interoperability
  checks pass, including 36 append cases, four native append references and ten
  frozen-corpus rewrites. Native development-oracle interoperability runs on Linux.
- `git diff --check`: passes. No dependencies, fixture hashes or generators change.

[Native run 41135248666](https://github.com/enioarz/gdxrs/actions/runs/41135248666) passes Linux x64, macOS ARM64/Intel
and Windows x64/MSVC. Each platform executes default debug/release and the full
default/minimal/all feature, formatting/Clippy/dependency and frozen-corpus gates.
The new regressions execute natively on all four. Actual tested merge revision
`3c2d5ed8fbd89e612fd81b6e3e70c45a08d6722f` has tree `cdc63dcf8c4ce1dd78e2023b645629507944cb93`,
which matches the implementation revision exactly; the workflow head SHA alone
is not treated as runtime evidence.

M0, label-page and M3 OS-capped smoke gates were not rerun for this focused identity
fix. The original task 030 evidence retains those historical results; none are
claimed at this revision. No new RSS/performance/scale qualification is implied.
Implementer self-review confirms that only new name references canonicalize, while
inherited raw slots and the existing terminal-error/cleanup path remain intact.
This is not independent review signoff. Task 031 remains pending.
