> Historical M0 handoff draft. Promoted into [numbered task 009](../../../tasks/009-lazy-metadata.md); see the [current queue](../../../tasks/README.md) and task evidence for implementation status. The original draft below is retained as release history.

# Draft 009 — Lazy metadata navigation

Status: **pending, not implemented**. Prerequisites: tasks 007–008 accepted. Authority: [F05–F09/F15/F17/F19](../../format-v7.md), [reader contract](../../reader-contract.md), [value/label model](../../decisions/0002-value-and-label-model.md). No contract expansion is implied.

## Scope and allowed files

Implement `Reader` opening and lending symbol/UEL/comment/set-text/acronym/domain cursors, bounded `SymbolHandle` and symbol lookup. Allowed: `src/metadata/`, necessary `src/model/` and `src/read/reader.rs`, narrow source/budget integration, `src/lib.rs`, focused `tests/metadata.rs`, evidence `docs/evidence/009.md` and queue status. No record decoding, reverse-dictionary catalog, sidecar or collection API.

## Required behavior and evidence

Use `semantics-0`, `semantics-1`, `semantics-0-be`, `key-widths`, `case-distinct-labels`, `optional-domains` and `optional-domains-absent`. Compare metadata with complete authored TSV expectations: field order, physical/projected counts, zero-based text IDs, acronym IDs, strict/relaxed domains, comments and raw byte labels. `a` and `A` must remain distinct stored UELs; `ff` is retained as a byte with fallible UTF-8 conversion. Symbol lookup folds ASCII case only as specified.

Exercise acceptance scenarios `open-no-record-scan`, `raw-label-bytes`, `metadata-cold-lookup`, `source-handle-mismatch` and metadata portions of `universe-and-alias`. Counting reads must show no record-section scan, including lookup of late metadata; necessary compressed metadata blocks are allowed. Cache/page directory and every concurrently held decoder share the root quota; no array/map proportional to total UEL/symbol count. Verify borrowed entry invalidation at next advance and bounded retained handles after Reader drop.

Assert `InvalidCount`, `InvalidDimension`, `InvalidType`, `InvalidReference` and `UnexpectedEof` on corresponding malformed fields/truncations. A different-reader handle yields `SourceMismatch` before seeks. Locally derived missing/non-set aliases yield `InvalidReference`; cycles terminate with `AliasCycle` within at most symbol-count hops without an eager visited set. Preserve declared bounds and terminal error context.

## Acceptance and review

Run formatting/Clippy, nonzero `cargo test --locked --test metadata`, fixture/specification/contract checks and `git diff --check`. Record exact semantic comparisons, read-range counters and capacity/reservation peaks. Review each metadata allocation, cursor borrow lifetime, page eviction and alias bound before task 010. Small metadata tests do not certify larger-than-RAM behavior; that remains M2.
