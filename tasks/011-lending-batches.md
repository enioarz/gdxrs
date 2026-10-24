# 011 — Bounded lending batches and M1 acceptance

Milestone: M1. Prerequisites: 010 complete. Read AGENTS.md, the v7 format specification, reader/resource contracts and decision 0003 before editing.

## Deliverables and scope

Primary files: src/scan.rs, tests/reader.rs, examples/dump.rs, tools/dev.py. Related tests/documentation and evidence `docs/evidence/011.md` are allowed. Do not implement a dependent task before this gate passes. Preserve upstream fixtures and expectations. Applicable format rules: F10–F21 and reader-acceptance.json.

## Acceptance checks

Projection and exact global limits, zero-limit I/O, terminal failures/no partial batch, quota lifecycle/actual capacity, borrowed-buffer compile-fail test, source mismatch/truncation and counting/fault injection. Full Rust output must match all ten oracle-verified TSV expectations. Run fmt, Clippy, default/no-default/all-feature suites and appropriate M0 tooling; distinguish M2-only scenarios and actual measurements from design. No writer/Polars/index code.

Use the pinned Rust toolchain and locked Cargo commands. Execute meaningful tests, not zero-test scaffolding. Record exact commands, counts, errors/skips, dependency/resource evidence and limitations at the tested commit. Do not weaken expected results to match the implementation.

## Completion gate

All required checks above actually pass and the evidence identifies the tested code. Self-review is recorded as author review, never independent M0 approval. Task status is updated separately from code where useful.
