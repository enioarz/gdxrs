# Numbered implementation tasks

These specifications break **M0** into bounded units of work. They are instructions for future implementation; adding them does not start implementation. Read the root [AGENTS.md](../AGENTS.md) and [architecture plan](../IMPLEMENTATION_PLAN.md) first.

## Execution order

| Task | Scope | Prerequisites | Current status |
| --- | --- | --- | --- |
| [001 — Development bootstrap](001-development-bootstrap.md) | Minimal Rust crate, reproducible tools and command entry point | None | Pending; first eligible task |
| [002 — Pinned upstream oracle](002-pinned-oracle.md) | Reproducible C++ reference build and semantic dump format | 001 complete | Pending |
| [003 — Oracle fixture corpus](003-oracle-fixtures.md) | Independently specified fixtures, manifests and negative cases | 002 complete | Pending |
| [004 — Verified v7 specification](004-v7-format-specification.md) | Byte layout, semantic rules and complete API inventory | 003 complete | Pending |
| [005 — Reader and resource contracts](005-reader-resource-contracts.md) | Concrete types, ownership, error and memory contracts for M1 | 004 complete | Pending |
| [006 — Baseline and M0 review](006-baseline-and-m0-review.md) | Measured oracle baseline, repeatability and independent review | 005 complete | Pending |

Dependencies mean the task's acceptance checks passed and its evidence record is present, not merely that its files exist. Task 006 additionally requires the independent review described there before M1 implementation is eligible. Source research may reveal missing fixtures; add them in the current task, record the change, and rerun the affected prior gates instead of treating this table as a reason to ignore new evidence.

M1 and later implementation tickets will be written from the verified format and contracts. Do not infer authorization to implement them from this queue. The next tickets must separate checked primitives, compression blocks, metadata navigation, record decoding, and batch scanning into reviewable changes with exact fixture references.

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

## Scope of this documentation change

Only `AGENTS.md`, these task specifications, and a navigation link in the architecture plan are being added now. No task has been executed, no oracle has been built, and no Rust functionality has been validated.
