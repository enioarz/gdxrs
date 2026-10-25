> Historical M0 handoff draft. Promoted into [numbered task 010](../../../tasks/010-record-decoder.md); see the [current queue](../../../tasks/README.md) and task evidence for implementation status. The original draft below is retained as release history.

# Draft 010 — Raw record decoding and semantic values

Status: **pending, not implemented**. Prerequisites: tasks 007–009 accepted. Authority: [F10–F15/F19/F20](../../format-v7.md), [value model](../../decisions/0002-value-and-label-model.md) and [reader contract](../../reader-contract.md).

## Scope and allowed files

Implement the serial per-symbol raw decoder and typed value/default/alias/universe projection. Allowed: `src/read/record.rs`, `src/model/value.rs`, narrow existing format/metadata integration, focused `tests/records.rs`, `docs/evidence/010.md` and queue status. Batches/projection are task 011; no filters, writer, indexes or checkpoints here.

## Required behavior and evidence

Read `_DATA_`, matching dimensions, header `-1` sentinel, checked bounds and widths. Use fixed state for up to 20 dimensions/five fields. Verify compact last-key increments, explicit suffix changes, positive valid IDs and strict raw tuple ordering; terminal marker and physical stored count must agree. Preserve all ordinary/acronym bits and special identities, consuming every value field.

Compare all ordered public records against independent TSV expectations across every valid fixture, including stored `equation_values` in both `semantics-0`/`semantics-1`, every variable/equation default subtype, `key-widths`/`key-widths-be` boundary cardinalities, `semantics-0-be`, `blocks` crossing, operations and aliases. Empty indexed symbols remain empty; zero-stored-row scalars synthesize exactly one subtype-default row; explicit scalars do not gain a default. Universe rows follow one-based file UEL order.

Cover scenarios `scalar-explicit`, `scalar-default`, `empty-indexed`, `all-kinds-and-fields`, `universe-and-alias`, `special-identities`, `both-byte-orders`, `malformed-fields`. Assert `InvalidKeyTag`, `InvalidValueTag`, `InvalidDimension`, `InvalidCount`, `InvalidReference` and `InvalidAcronymId` for relevant malformed states. Checked invalid arithmetic must fail before conversion/access. Tag-10 nonfinite payloads remain number bits and are never cast to acronym IDs. Decoder errors are terminal with no serial retry.

Keep decoding fixed/bounded; no full-symbol prescan, per-row label strings or arrays based on record count. Use the shared budget for any owned storage and record release behavior.

## Acceptance and review

Run formatting/Clippy, nonzero `cargo test --locked --test records`, fixture/specification/contract checks and `git diff --check`. Record full stream comparisons, actual row/field counts, malformed outcomes and resource observations. Review independent expectations, framing, count/default distinctions and error paths before task 011. A Rust round trip or matching counts alone is insufficient.
