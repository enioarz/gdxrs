# 007 — Checked source I/O, quota, errors and v7 headers

Milestone: M1. Prerequisites: User-directed M1 advancement in decision 0003. Read AGENTS.md, the v7 format specification, reader/resource contracts and decision 0003 before editing.

## Deliverables and scope

Primary files: src/error.rs, src/budget.rs, src/model.rs, src/source.rs, src/format.rs, src/lib.rs. Related tests/documentation and evidence `docs/evidence/007.md` are allowed. Do not implement a dependent task before this gate passes. Preserve upstream fixtures and expectations. Applicable format rules: F01–F03/F19.

## Acceptance checks

Header-only parsing of every valid fixture; explicit old/future version and invalid-offset rejection; short-read/I/O/source-change tests; quota overflow/reservation release tests. Header opening must not prefetch data sections.

Use the pinned Rust toolchain and locked Cargo commands. Execute meaningful tests, not zero-test scaffolding. Record exact commands, counts, errors/skips, dependency/resource evidence and limitations at the tested commit. Do not weaken expected results to match the implementation.

## Completion gate

All required checks above actually pass and the evidence identifies the tested code. Self-review is recorded as author review, never independent M0 approval. Task status is updated separately from code where useful.
