> Historical M0 handoff draft. Promoted into [numbered task 011](../../../tasks/011-lending-batches.md); see the [current queue](../../../tasks/README.md) and task evidence for implementation status. The original draft below is retained as release history.

# Draft 011 — Serial lending batch cursors

Status: **pending, not implemented**. Prerequisites: tasks 007–010 accepted. Authority: [reader contract](../../reader-contract.md), [resource contract](../../resource-contract.md), [F10/F12–F15/F19](../../format-v7.md) and all 23 M1 scenarios in [reader acceptance](../../reader-acceptance.json).

## Scope and allowed files

Implement `ScanOptions`, `ScanCursor::next_batch`, lending `BatchView`/`RecordView`, projections, exact row limits and cancellation. Allowed: `src/read/scan.rs`, `src/read/batch.rs`, narrow model/Reader/budget integration, `src/lib.rs`, focused `tests/batches.rs`, borrow-check doctests/test helpers, `docs/evidence/011.md` and queue status. No selection engine, writer, sidecar, workers or owned/unbounded collection API.

## Required behavior and evidence

Use `blocks` for first batch, limit 0, empty projection, cancellation and fault injection; `semantics-0`/`semantics-1` for 20-key/five-field projection, scalars/defaults and equations; `key-widths-be` for byte-order equivalence. Projected output retains original dimension/field order; every unselected encoded field is still consumed. Empty projection still counts rows and obeys the 65,536-row ceiling. Limits count public rows, including scalar/universe projection.

Exercise `first-batch`, `zero-limit`, `projection-framing`, `empty-projection`, `batch-too-small`, `borrowed-batch-reuse`, `short-reads-and-terminal-error`, `budget-rejection-and-drop`, `source-truncation`, and integrate all other M1 scenarios. Assert `InvalidProjection` for excess mask bits, `InvalidOptions` for contradictory capacities, `BatchTooSmall` before an impossible output allocation, and `BudgetTooSmall` before exceeding the shared quota. Script short reads, then inject `Io` during batch fill: expose no partial batch, preserve terminal category/context on later calls and emit no duplicates. Observable truncation yields `SourceChanged`.

Compute charged capacity from actual element layouts; reuse at most one internal batch; compile-fail borrowing checks must reject retaining a view across next fill. Counting-source tests must prove no whole-symbol prescan, zero-limit no data decoding and bounded first-batch prefix reads. Retained cursors, metadata/codec/output buffers share one root cap; drop releases exactly once.

## Acceptance and review

Run formatting/Clippy, nonzero batch/record/metadata/compression/header suites, borrow-check doctests, and no-default-features uncompressed/error tests. Run fixture/specification/contract checks and `git diff --check`. Record a scenario-to-test map for all 23 M1 cases with exact counts; any unexecuted required case keeps M1 open. Review public lifetimes, errors, measured read ranges and complete allocation accounting before declaring serial-reader completion. The two M2 scenarios, OS-capped scale work and performance targets remain separate unrun gates.
