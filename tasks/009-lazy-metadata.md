# 009 — Lazy symbols, labels, text, comments, acronyms and domains

Milestone: M1. Prerequisites: 008 complete. Read AGENTS.md, the v7 format specification, reader/resource contracts and decision 0003 before editing.

## Deliverables and scope

Primary files: src/reader.rs, src/metadata.rs, src/lib.rs. Related tests/documentation and evidence `docs/evidence/009.md` are allowed. Do not implement a dependent task before this gate passes. Preserve upstream fixtures and expectations. Applicable format rules: F05–F09/F14–F17.

## Acceptance checks

Stream every fixture catalog and compare complete metadata to authored TSV expectations; preserve IDs/bytes, scalar/alias counts and optional domains; reject invalid IDs/alias cycles; no catalog-wide allocation; retained views borrow cursor storage.

Use the pinned Rust toolchain and locked Cargo commands. Execute meaningful tests, not zero-test scaffolding. Record exact commands, counts, errors/skips, dependency/resource evidence and limitations at the tested commit. Do not weaken expected results to match the implementation.

## Completion gate

All required checks above actually pass and the evidence identifies the tested code. Self-review is recorded as author review, never independent M0 approval. Task status is updated separately from code where useful.
