# 020 — Review fixes for strict alias domains and initialization cleanup

PR #14 is held in draft for re-review. This follow-up supersedes the initial
[task-020 qualification](020.md) for the two findings reported at
`d57573ec6082ce3399215944b59e2973032b88f6`. Tasks 021–023 remain pending.

Fix revision: `659e8564d1fb40b077c2550bc8a22be80a159d46`.
[Structured evidence](020-review-fixes-results.json) retains source-bound reports,
regression results and native-platform results. The user review identified the
findings; no review approval or independent signoff is inferred from the fixes.

## P1 — Preserve strict-domain alias identity

The old catalog writer serialized `set_target(reference)`, replacing the domain's
alias with its underlying set. The added regression, run before changing the
production code at d57573e, failed: a forward domain naming alias K expected
symbol ID 4 but read ID 1 (the underlying set I).

Catalog serialization now resolves the original reference ID, validates its chain
through a one-dimensional set or universe, then emits the original ID. Alias
*definitions* still normalize their own targets as before. Strict-domain alias
identity is preserved independently of that validation/alias-definition behavior.

The regression checks registered and forward-named references, chained aliases,
a universe alias and a direct universe domain, in raw and compressed output where
available. It asserts both stored strict symbol IDs and domain names. Existing
cycle/missing-target/type checks remain enabled.

The independent native interoperability recipe now additionally writes PAlias
with domain Alias. Native GDX and Rust dumps must both contain domain symbol ID 1
and name `Alias`, preserving the referenced alias rather than underlying set I
(ID 2). The original direct-set domain P(I,*) remains checked too. All five
metadata cases (raw default/minimal/all, compressed default/all) pass. The recipe
now has eight registered symbols, so its output sizes/hashes intentionally differ
from the earlier seven-symbol probe; frozen corpus/oracle/baseline files are
unchanged. This is an added semantic assertion, not an updated expected result
for the original domain-loss defect.

## P2 — Retain ownership before journal initialization

The constructor previously created a directory and attempted journal allocation
before returning ownership to Writer. Allocation error used `?`; the local
metadata owner's Drop could fail cleanup without returning that failure or
retaining an owner for abort. The added regression failed before the fix because
AllocationFailed had no secondary PermissionDenied cleanup cause.

Directory ownership now transfers to Writer before a separate fallible journal
initialization step. Initialization errors use the existing terminal writer
failure path: preserve the primary error, report cleanup failure as secondary,
and retain the metadata owner when cleanup needs retry. No additional allocation,
quota, temporary file or public API is introduced.

The regression injects journal allocation failure after directory creation and
persistent removal failure. It requires AllocationFailed plus PermissionDenied,
Failed/unpublished state and the retained directory owner. Abort fails while
removal is denied; once the fault is cleared, abort succeeds and the directory is
gone. Finish still returns the original failure, and final owner drop releases
all root charges. The ordinary allocation-failure and cleanup-retry regressions
remain enabled.

## Executed checks

At fix revision 659e856 on Linux x86_64 / Rust 1.99.1:

- Both new regressions fail with the held production code and pass with the fix;
  all eleven metadata unit tests pass together.
- `python3 tools/dev.py reader-check`: default/all each pass 70 unit tests,
  36 integration tests and seven doctests; minimal passes 62, 35 and seven.
  Ten frozen fixtures compare 141358 records. Formatting, Clippy and dependency
  audits pass without changed thresholds or ignored tests.
- `python3 tools/dev.py writer-check`: all prior block/empty-file cases and the
  five expanded independent metadata/native cases pass. Updated raw metadata
  outputs are 748 bytes, compressed outputs 581 bytes, with hashes in JSON.
- `python3 tools/dev.py m0-check`: passes 44 tooling tests and the original
  fixture, oracle, baseline and released-review integrity checks.

[Native run 40300835549](https://github.com/enioarz/gdxrs/actions/runs/40300835549)
passes Linux x86_64, macOS ARM64/Intel and Windows x64/MSVC. Each host executes
both regressions in applicable feature suites, optimized default tests, warning-
free checks and the complete differential corpus. Checked merge `e524c115d76d1825628aa8070cad87fee80798ee`
has the same tree as fix revision 659e856; host/toolchain details and job URLs are
retained in JSON. The separately compiled native GDX interoperability probe ran
locally on Linux. PR #14 remains held in draft for re-review. The original 100000-request
registration experiment remains bound to its recorded ff3840b revision; no new
large-scale RSS or throughput measurement is claimed here. Incremental scan
costs, fixed-state allocator-OOM and trusted namespace/durability limitations
remain as documented in the task-020 contract.
