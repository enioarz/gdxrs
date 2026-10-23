# 004 — Verified v7 format specification and API inventory

**Milestone:** M0. **Prerequisites:** task 003 complete. **Goal:** remove binary-format guesswork before production parser code is written.

## Deliverables

- `docs/format-v7.md`: normative byte layout and decoding/encoding rules, with source and fixture references for each rule.
- `docs/format-evidence.json`: a machine-readable ledger of rule IDs, upstream revision/function references, fixture IDs, expected observations and verification state.
- `docs/api-parity.csv`: one row per unique upstream public function, grouped by capability and mapped to a planned Rust equivalent/adapter or explicit deferral/exclusion.
- Development-only inspection/checking code under `tools/`; `tools/dev.py spec-check`.
- `docs/evidence/004.md`, including a separate source-versus-fixture review pass.

## Required specification contents

1. **Primitive representation:** signedness, widths, byte order, size/order markers, string lengths/bytes and identifier constraints. Document rejection rules separately from upstream's permissive error handling.
2. **Header and navigation:** exact field sequence, magic/version values, compression flags, section-offset table, valid offset/count ranges and relationships. Explain where count fields have sentinel values rather than literal lengths.
3. **Compression framing:** three-byte header layout, length encoding, stored versus zlib payload, decoded-size bounds, checksums, end conditions, section transitions and records spanning blocks.
4. **Metadata sections:** symbol records, UELs, set text, acronyms, strict/relaxed domains, comments, optional variants and trailing markers. Include enough information to skip/page each table without full materialization.
5. **Record codec:** symbol preamble, key bounds and width selection, initial key state, delta tags, terminal marker, value tags/ordinary payloads, scalar and zero-record handling. Include exact pseudocode and small annotated byte traces for representative cases.
6. **Semantic rules:** aliases/universe, symbol types/subtypes, defaults, special-value/acronym mapping, signed zero/NaNs, label and symbol-name matching, mapped ordering, duplicate handling and append behavior.
7. **Writer rules:** required ordering, unknown-count handling, metadata finalization, section-offset backpatching and representation limits. Distinguish optional optimization from required format behavior.
8. **Restart analysis:** list all state carried across records/blocks, identify when it is reset, and explain what a future checkpoint must retain. This is a prerequisite analysis, not permission to implement indexes.
9. **Compatibility boundaries:** supported v7 variants and byte orders, explicit unsupported versions, malformed-input behavior, and unresolved questions with affected capabilities.

## Implementation steps

- Read the pinned `gxfile.cpp`, `gxfile.hpp`, `gmsstrm.cpp/.hpp`, `gdxapi.yaml`, `gclgms.h` and relevant tests. Use immutable source links with function names and line references, not just links to the repository homepage.
- For each rule assign an ID and link an oracle fixture or independent byte observation. Record the actual field widths and tag values; prose such as “decode like upstream” is not an implementation specification.
- If an API view normalizes file contents, separate the file encoding from the API projection. If source and experiment disagree, add a minimal fixture and resolve the contradiction; do not pick the convenient interpretation.
- Extract the complete public-function list from the API definition's function section, distinguishing callback declarations/duplicates elsewhere. Compare against `api-parity.csv` automatically. Each row includes source function, intended native/adapter behavior, milestone, status, test requirement and rationale for deferral/exclusion. No function may disappear under an “etc.” entry.
- `spec-check` verifies ledger references/coverage, the exact API inventory and annotated fixture observations using development tooling. It must not import or depend on a future `gdxrs` parser.
- Do not create a production decoder as a way to “prove” its own specification. A small independent annotation tool is acceptable if it is clearly development-only and its assumptions are checked against source/oracle observations.

## Acceptance checks

```sh
python3 tools/dev.py spec-check
python3 tools/dev.py fixtures-check
git diff --check
```

Record rule counts, observed versus unverified rules, unique API function counts, and missing/duplicate references. Removing a public-function row or changing an expected annotated byte must make the relevant check fail in a temporary copy.

Perform a second review pass against source for every item needed by M1: primitives, headers, compression, metadata navigation, records and special values. Required unresolved items block completion. Later API behavior can remain deferred in the parity inventory with an assigned milestone; unknown rules required to parse v7 cannot.

## Completion gate

A contributor can implement the v7 serial reader from the explicit specification and fixtures without inventing field widths, tag meanings or default semantics. The API inventory is complete. Unlocks task 005; M1 remains gated by task 006.
