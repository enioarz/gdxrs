# Draft 007 — Checked primitives and header validation

Status: **pending, not implemented**. Prerequisite: accepted M0 at `914c6fc4f29407ea34d010e2c1d8faf1c3eb420d`. This post-review draft restates [F01–F03/F19](../../format-v7.md), the [reader contract](../../reader-contract.md) and [resource contract](../../resource-contract.md); it does not amend them. Promote into the numbered implementation queue on a subsequent M1 change after reviewing scope.

## Scope and allowed files

Implement checked primitive reads, uniform byte-order selection, v7 header/index validation, bounded contextual errors, positioned-file reads and the shared reservation foundation needed by subsequent tasks. Allowed: `src/lib.rs`, `src/error.rs`, `src/format/`, `src/io/source.rs`, `src/model/ids.rs`, `src/resource.rs`, focused `tests/primitives.rs`/`tests/header.rs` and test source helpers, plus `docs/evidence/007.md` and queue status. No new dependency or codec/metadata/record implementation.

## Required behavior and evidence

Use `semantics-0`, `semantics-0-be`, `key-widths`, `key-widths-be`, `optional-domains` and `optional-domains-absent`. Validate 17-byte primitive markers, length-delimited strings, the 80-byte reservation, six physical pointers and optional zero domain pointer. Compare decoded header fields with the independently verified fixture ledger. Do not infer physical order from pointer ordinals or allocate section-sized buffers.

Assert `UnsupportedVersion` for `version-5-probe`, `version-6-probe`, `future-version-probe`; `UnexpectedEof` for `truncated-header`; `InvalidOffset` for `bad-offset`; `UnsupportedByteOrder` for a locally derived mixed-order probe. Invalid sizes/magic use `InvalidHeader`. Script short reads and injected failures (`Io`), and detect truncation of an opened copied file (`SourceChanged`). Validating required header pointers must not scan data records.

Implement checked reservation arithmetic/fallible allocation, minimum 1 MiB budget, fixed context and temporary path-conversion reservations. Exercise overflow, denied reservations before allocation, allocation failure and exactly-once release. No input-sized count allocation, unchecked casts/indexing or unsafe code. Source helpers must support counting reads for later tests without adding a public generic-source API.

## Acceptance and review

Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, and focused `cargo test --locked --test primitives --test header`; both suites must execute nonzero assertions/tests. Run `python3 tools/dev.py fixtures-check` and `spec-check`, and `git diff --check`. Record exact revision, test counts, observed failures and unrun platform cases. Review checked arithmetic, actual read ranges, platform handle semantics and every allocation site before task 008. This is not a working-reader completion claim.
