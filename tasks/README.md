# Numbered implementation tasks

These specifications break **M0–M3** into bounded units of work. Task evidence distinguishes completed implementation from future work. Read the root [AGENTS.md](../AGENTS.md) and [architecture plan](../IMPLEMENTATION_PLAN.md) first.

## Execution order

| Task | Scope | Prerequisites | Current status |
| --- | --- | --- | --- |
| [001 — Development bootstrap](001-development-bootstrap.md) | Minimal Rust crate, reproducible tools and command entry point | None | Complete; [evidence](../docs/evidence/001.md) |
| [002 — Pinned upstream oracle](002-pinned-oracle.md) | Reproducible C++ reference build and semantic dump format | 001 complete | Complete; [evidence](../docs/evidence/002.md) |
| [003 — Oracle fixture corpus](003-oracle-fixtures.md) | Independently specified fixtures, manifests and negative cases | 002 complete | Complete; [evidence](../docs/evidence/003.md) |
| [004 — Verified v7 specification](004-v7-format-specification.md) | Byte layout, semantic rules and complete API inventory | 003 complete | Complete; [evidence](../docs/evidence/004.md) |
| [005 — Reader and resource contracts](005-reader-resource-contracts.md) | Concrete types, ownership, error and memory contracts for M1 | 004 complete | Complete; [evidence](../docs/evidence/005.md) |
| [006 — Baseline and M0 review](006-baseline-and-m0-review.md) | Measured oracle baseline, repeatability and independent review | 005 complete | Complete; [independent review accepted](../docs/reviews/m0.md) at `914c6fc`; released on `main` at `2e5980a` |

Dependencies mean the task's acceptance checks passed and its evidence record is present, not merely that its files exist. Task 006 required independent review; the user confirmed on 2026-10-24 that M0 is reviewed and released. The older pending snapshot is historical. Source research may reveal missing fixtures; add them in the current task, record the change, and rerun the affected prior gates instead of treating this table as a reason to ignore new evidence.

The released M0 handoff drafts are promoted into the numbered M1 tasks below. M1 tasks 007–011 follow the verified format and contracts under explicit user authorization. Later milestone tickets must retain the same bounded scopes and exact fixture references.

## Required completion record

Each implementation task creates `docs/evidence/NNN.md` containing:

- Task ID, implementation revision, relevant upstream/tool versions, and actual working directory.
- Deliverables and acceptance criteria, each mapped to a command/result or a named review artifact.
- Exact commands, exit status, executed fixture/test counts, failures, skips and unrun checks; references to retained machine-readable reports.
- Relevant resource measurements and how they were obtained. Say “not measured” when appropriate.
- Deviations, unresolved issues and scope limitations. Required unresolved checks keep the task incomplete.
- Review findings, their disposition, reviewer identity/role, and reviewed revision where review is required.

Use statuses `pending`, `in progress`, `blocked`, or `complete`. Keep verification and review distinct in the record. A successful self-review can finish an ordinary task; it cannot satisfy task 006's independent-review requirement.

## Command convention

The command surface below is implemented by `tools/dev.py`. Unknown commands fail clearly.

```sh
python3 tools/dev.py check-bootstrap
python3 tools/dev.py oracle-build
python3 tools/dev.py oracle-self-test
python3 tools/dev.py fixtures-check
python3 tools/dev.py spec-check
python3 tools/dev.py contracts-check
python3 tools/dev.py benchmark-oracle --profile small
python3 tools/dev.py m0-check
python3 tools/dev.py reader-check
python3 tools/dev.py label-pages-check
```

Run from the repository root. Use Python's standard library for the development runner; it is not a runtime dependency. Subcommands must preserve child exit failures, avoid shell-string interpolation, validate their input/output paths, and emit a fresh report under ignored `.work/reports/`. Reports record revision and run identity; an old artifact cannot make a failed new command pass. Store compact reviewed results/provenance in version control where useful, not build trees or large generated files.

## Current implementation scope

Tasks 001–011 are complete; M1 is merged at `5cc766d`. M1 provides the bounded serial GDX v7 reader. M2 begins with task 012; its out-of-core read gate passed in task 016 on Linux x86_64. Runtime evidence is separate from documentation checks. The crate now also has a qualified M3 sorted raw writer; see tasks 018–023 below.

## M1 — user-directed advancement (2026-10-24)

The user explicitly confirmed M0 is reviewed/released and instructed proceeding with M1. [Decision 0003](../docs/decisions/0003-m1-advancement.md) records that confirmation and resolution of the stale local gate.

| Task | Scope | Status |
| --- | --- | --- |
| [007](007-checked-foundation.md) | Checked source I/O, quota, errors and v7 headers | Complete; [evidence](../docs/evidence/007.md) |
| [008](008-bounded-compression.md) | Bounded raw/zlib section streams | Complete; [evidence](../docs/evidence/008.md) |
| [009](009-lazy-metadata.md) | Lazy symbols, labels, text, comments, acronyms and domains | Complete; [evidence](../docs/evidence/009.md) |
| [010](010-record-decoder.md) | Stateful serial raw record decoding | Complete; [evidence](../docs/evidence/010.md) |
| [011](011-lending-batches.md) | Bounded lending batches and M1 acceptance | Complete; [evidence](../docs/evidence/011.md) |

## M2 — bounded slicing and metadata

M1 is merged in `5cc766d`; the user requested beginning M2. [Decision 0006](../docs/decisions/0006-m2-selection.md) defines the first selection API. All six M2 task gates have passed, including OS-capped scale qualification in task 016. This qualifies the bounded read scope on Linux x86_64; native throughput parity, writing and subsequent milestones are not claimed.

| Task | Scope | Status |
| --- | --- | --- |
| [012](012-exact-id-slices.md) | Exact bounded raw-ID slices, projection/limits and work counters | Complete; [evidence](../docs/evidence/012.md) |
| [013](013-small-exact-label-lookup.md) | Small exact label lookup and paged metadata/cache lifecycle | Complete; [evidence](../docs/evidence/013.md) |
| [014](014-bounded-spill-primitives.md) | Quota-controlled sorted runs, merge and failure cleanup | Complete; [evidence](../docs/evidence/014.md) |
| [015](015-large-lookup-domain-enumeration.md) | Spill-backed large label requests and unique domains | Complete; [evidence](../docs/evidence/015.md) |
| [016](016-m2-scale-acceptance.md) | Independently oversized inputs, OS memory cap and M2 acceptance | Complete; [evidence](../docs/evidence/016.md) |

M2 is complete for the qualified read scope. M3 sorted-writer work is tracked below.


## User-directed native platform follow-up

Before M3, the user requested macOS/Windows compatibility qualification.

| Task | Scope | Status |
| --- | --- | --- |
| [017](017-native-platform-compatibility.md) | Native macOS ARM64/Intel and Windows x64 reader/metadata/spill compatibility | Complete; [evidence](../docs/evidence/017.md) |

The native platform follow-up is complete; M3 has begun below.


## M3 — user-directed sorted writer

The user confirmed #11 merged and requested M3. [Writer contract](../docs/decisions/0012-writer-contract.md)
defines the first API/resource/ownership contract. Complete one bounded task gate
before starting its dependent; task 023 is the M3 release gate.

| Task | Scope | Status |
| --- | --- | --- |
| [018](018-writer-foundation.md) | Writer contract, bounded empty-v7 creation and atomic publication | Complete; [evidence](../docs/evidence/018.md) |
| [019](019-bounded-writer-codec.md) | Bounded byte/block encoder and stored/zlib compression | Complete; [evidence](../docs/evidence/019.md) |
| [020](020-writer-metadata-spooling.md) | Original-ID raw registration and metadata spools | Complete; [review fixes](../docs/evidence/020-review-fixes.md); merged in PR #14 at `8997385` |
| [021](021-sorted-record-writer.md) | Checked sorted raw symbol/record encoding | Complete; [evidence](../docs/evidence/021.md) |
| [022](022-writer-finalization-interop.md) | Complete metadata/finalization and both interoperability directions | Complete; [evidence](../docs/evidence/022.md) |
| [023](023-m3-writer-acceptance.md) | OS-capped record/metadata/pipeline scale and M3 acceptance | Complete; [evidence](../docs/evidence/023.md) |


M3's sorted-writer exit gate passes at `cc972bf`; see [M3 API status](../docs/m3-api.md).
Linux OS/RSS qualification is separate from native Rust functionality on all four
platforms. Bulk metadata registration remains slower than eager native registration;
compressed timings use different compression levels. No M4 append/general-input
adapter or full native C API parity is implied. M4 is authorized and separately specified below. PRs #16/#17 retain task boundaries for review.

## M4 — general write and modern semantic parity

The user authorized beginning M4 after the M3 gate passed. Tasks 018–023 are
qualified and the final native CI passed. PR #18 integrates M3 onto main because
PR #17 merged into the already-merged feature branch; M4 builds on that validated
M3 tree. Complete one gate at a time; task 031 is the M4 exit gate.

| Task | Scope | Prerequisites | Status |
| --- | --- | --- | --- |
| [024](024-unsorted-raw-records.md) | Bounded unsorted raw records using the existing merge engine | 023 | Complete; [evidence](../docs/evidence/024.md) |
| [025](025-string-record-registration.md) | Streamed string records and first-registration dictionary/remap | 024 | In progress |
| [026](026-mapped-id-adapters.md) | Bounded mapped registration/read/write | 025 | Pending |
| [027](027-domain-error-streams.md) | Explicit domain validation and bounded error streams | 026 | Pending |
| [028](028-value-acronym-policies.md) | Special-value/acronym policies and semantic parity | 027 | Pending |
| [029](029-uel-metadata-operations.md) | Remaining modern UEL/metadata operations | 028 | Pending |
| [030](030-atomic-append.md) | Atomic copy/rewrite append | 029 | Pending |
| [031](031-m4-acceptance.md) | API inventory, general-input scale and M4 acceptance | 024–030 | Pending |
