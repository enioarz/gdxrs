# 008 — Bounded raw/zlib section streams

Milestone: M1. Prerequisites: 007 complete. Read AGENTS.md, the v7 format specification, reader/resource contracts and decision 0003 before editing.

## Deliverables and scope

Primary files: src/stream.rs, Cargo.toml, Cargo.lock, docs/decisions/0004-compression.md. Related tests/documentation and evidence `docs/evidence/008.md` are allowed. Do not implement a dependent task before this gate passes. Preserve upstream fixtures and expectations. Applicable format rules: F04/F10.

## Acceptance checks

Decode stored/zlib blocks with a fixed 32 KiB output limit; enforce complete zlib/checksum consumption; cross-block primitive reads; corruption and truncation failures; measure codec state and audit direct/transitive dependencies; no-default-features rejects compression.

Use the pinned Rust toolchain and locked Cargo commands. Execute meaningful tests, not zero-test scaffolding. Record exact commands, counts, errors/skips, dependency/resource evidence and limitations at the tested commit. Do not weaken expected results to match the implementation.

## Completion gate

All required checks above actually pass and the evidence identifies the tested code. Self-review is recorded as author review, never independent M0 approval. Task status is updated separately from code where useful.
