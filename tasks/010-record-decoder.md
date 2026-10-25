# 010 — Stateful serial raw record decoding

Milestone: M1. Prerequisites: 009 complete. Read AGENTS.md, the v7 format specification, reader/resource contracts and decision 0003 before editing.

## Deliverables and scope

Primary files: src/records.rs, src/model.rs. Related tests/documentation and evidence `docs/evidence/010.md` are allowed. Do not implement a dependent task before this gate passes. Preserve upstream fixtures and expectations. Applicable format rules: F10–F15/F19/F20.

## Acceptance checks

Compare every physical/public record to fixture expectations, both byte orders/compression modes and all fields; test tag/width/count/order corruption; scalar/universe/alias behavior; preserve numeric/acronym identity; validate EOF without a record prescan.

Use the pinned Rust toolchain and locked Cargo commands. Execute meaningful tests, not zero-test scaffolding. Record exact commands, counts, errors/skips, dependency/resource evidence and limitations at the tested commit. Do not weaken expected results to match the implementation.

## Completion gate

All required checks above actually pass and the evidence identifies the tested code. Self-review is recorded as author review, never independent M0 approval. Task status is updated separately from code where useful.
