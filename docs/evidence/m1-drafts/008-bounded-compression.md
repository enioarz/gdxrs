> Historical M0 handoff draft. Promoted into [numbered task 008](../../../tasks/008-bounded-compression.md); see the [current queue](../../../tasks/README.md) and task evidence for implementation status. The original draft below is retained as release history.

# Draft 008 — Bounded compression blocks

Status: **pending, not implemented**. Prerequisite: task 007 accepted. Authority: [F04/F19/F20](../../format-v7.md), [resource contract](../../resource-contract.md), and acceptance scenarios `checksum-integrity`, `short-reads-and-terminal-error`, `budget-rejection-and-drop`. This draft does not change those contracts.

## Scope and allowed files

Implement stored/zlib section decoding over the checked source with incremental primitive consumption across blocks. Allowed: `src/io/blocks.rs`, narrow integration in `src/io/`/`src/format/`, `src/lib.rs`, focused `tests/compression.rs`, `Cargo.toml`/`Cargo.lock` only for the reviewed compression feature/dependency, and `docs/evidence/008.md` plus queue status. Do not add a writer, index, parallel decoder or whole-section buffer.

## Required behavior and evidence

Use `blocks`, `semantics-1`, `semantics-0`, `damaged-zlib-checksum` and `truncated-block`. Verify three-byte headers with endian-independent big-endian payload length, both block kinds, maximum 32 KiB decoded output and the corpus's crossing record/primitive consumption. Assert `InvalidCompression` for damaged checksums, unknown kind, zero/excess decoded length, bad framing and trailing stream data; truncated framing/payload yields `UnexpectedEof`. Derived malformed blocks belong in bounded test helpers, without replacing tracked expected artifacts.

Audit and pin the selected `miniz_oxide` version, transitive dependencies and licenses before adding it. Confirm checksum validation and bounded output in the actual API. Measure all codec state/heap allocations and verify the existing 128 KiB scratch ceiling **before** claiming accounting. If it cannot be met, stop this slice for an explicit design review; do not silently raise the ceiling. Charge encoded/decoded buffers and simultaneous metadata/data decoders to the root quota. Failure/drop releases reservations once and never publishes corrupt decoded data.

Provide the default compression feature and a `--no-default-features` path that supports uncompressed files and returns `CompressionUnavailable` for compressed input. No native library/Python runtime dependency.

## Acceptance and review

Run formatting, Clippy, focused nonzero `cargo test --locked --test compression`, and corresponding no-default-features tests. Run fixture/specification checks and `git diff --check`. Record actual codec allocation measurements separately from RSS and from M2 scale claims. Review dependency/license evidence, checksum/trailing-data rejection, cross-block correctness and denied-allocation paths before task 009. Missing measurements keep this task blocked.
