# Numbered implementation tasks

These specifications break **M0** into bounded units of work. Tasks 001–006 are complete at the evidence revisions below. Post-review M1 drafts remain pending; adding them does not start implementation. Read the root [AGENTS.md](../AGENTS.md) and [architecture plan](../IMPLEMENTATION_PLAN.md) first.

## Execution order

| Task | Scope | Prerequisites | Current status |
| --- | --- | --- | --- |
| [001 — Development bootstrap](001-development-bootstrap.md) | Minimal Rust crate, reproducible tools and command entry point | None | Complete; [evidence](../docs/evidence/001.md) |
| [002 — Pinned upstream oracle](002-pinned-oracle.md) | Reproducible C++ reference build and semantic dump format | 001 complete | Complete; [evidence](../docs/evidence/002.md) |
| [003 — Oracle fixture corpus](003-oracle-fixtures.md) | Independently specified fixtures, manifests and negative cases | 002 complete | Complete; [evidence](../docs/evidence/003.md) |
| [004 — Verified v7 specification](004-v7-format-specification.md) | Byte layout, semantic rules and complete API inventory | 003 complete | Complete; [evidence](../docs/evidence/004.md) |
| [005 — Reader and resource contracts](005-reader-resource-contracts.md) | Concrete types, ownership, error and memory contracts for M1 | 004 complete | Complete; [evidence](../docs/evidence/005.md) |
| [006 — Baseline and M0 review](006-baseline-and-m0-review.md) | Measured oracle baseline, repeatability and independent review | 005 complete | Complete; [independent review accepted](../docs/reviews/m0.md) at `914c6fc`; [fresh evidence](../docs/evidence/006.md) |

Dependencies mean the task's acceptance checks passed and its evidence record is present, not merely that its files exist. Task 006 additionally requires the independent review described there before M1 implementation is eligible. Source research may reveal missing fixtures; add them in the current task, record the change, and rerun the affected prior gates instead of treating this table as a reason to ignore new evidence.

M1 drafts were written after the accepted M0 aggregate passed. They restate verified contracts and remain in the evidence handoff until a subsequent M1 change promotes them into numbered task files. Do not infer that implementation has started from this queue.

| Draft | Prerequisite | Status |
| --- | --- | --- |
| [007 — Checked primitives/header](../docs/evidence/m1-drafts/007-checked-primitives.md) | Accepted M0 | Pending; next eligible implementation scope after promotion |
| [008 — Bounded compression](../docs/evidence/m1-drafts/008-bounded-compression.md) | 007 accepted | Pending |
| [009 — Lazy metadata](../docs/evidence/m1-drafts/009-lazy-metadata.md) | 007–008 accepted | Pending |
| [010 — Record decoding](../docs/evidence/m1-drafts/010-record-decoding.md) | 007–009 accepted | Pending |
| [011 — Serial batch cursors](../docs/evidence/m1-drafts/011-serial-batches.md) | 007–010 accepted | Pending |

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

The command surface below is a **future contract**. Task 001 creates `tools/dev.py`; later tasks add the named subcommands. Until implemented, commands must fail clearly rather than return success or generate placeholder reports.

```sh
python3 tools/dev.py check-bootstrap
python3 tools/dev.py oracle-build
python3 tools/dev.py oracle-self-test
python3 tools/dev.py fixtures-check
python3 tools/dev.py spec-check
python3 tools/dev.py contracts-check
python3 tools/dev.py benchmark-oracle --profile small
python3 tools/dev.py m0-check
```

Run from the repository root. Use Python's standard library for the development runner; it is not a runtime dependency. Subcommands must preserve child exit failures, avoid shell-string interpolation, validate their input/output paths, and emit a fresh report under ignored `.work/reports/`. Reports record revision and run identity; an old artifact cannot make a failed new command pass. Store compact reviewed results/provenance in version control where useful, not build trees or large generated files.

## Current implementation scope
Tasks 001–006 are complete with evidence. The independent review accepted exact candidate `914c6fc`, and the unchanged aggregate gate passed with `m1_eligible: true`. M1 drafts remain pending. The crate is still a bootstrap with no GDX decoder/writer. Do not infer runtime readiness from development tooling or documentation gates.
## M1 start authorized after PR #1 merge

The user explicitly requested beginning M1 in a new PR after merging M0. This authorizes task 007 development despite the still-pending recorded independent M0 gate; it does not manufacture M0 acceptance. The new PR remains draft pending that review.

| Task | Status |
| --- | --- |
| [007 — Checked header](007-checked-header.md) | Implemented; review pending |
| [008 — Compression](008-bounded-compression.md) | Pending |
| [009 — Metadata](009-lazy-metadata.md) | Pending |
| [010 — Records](010-record-decoding.md) | Pending |
| [011 — Batches](011-serial-batches.md) | Pending |
