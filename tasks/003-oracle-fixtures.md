# 003 — Oracle fixture corpus

**Milestone:** M0. **Prerequisites:** task 002 complete. **Goal:** create a small, independent corpus that future decoders and writers cannot pass by sharing the same mistake.

## Deliverables

- Generator source under `tools/oracle/` and `tests/fixtures/README.md` describing provenance, regeneration and comparison rules.
- Small `.gdx` fixtures and independent expected semantic records under `tests/fixtures/v7/`.
- `tests/fixtures/manifest.json` with schema version, stable fixture IDs, source/generator revisions, SHA-256 digests, compression mode, byte order, expected symbol/record counts, tested properties and semantic expectation paths.
- Negative cases or deterministic mutation recipes under `tests/fixtures/invalid/`, with intended future Rust error categories and observed upstream behavior kept separate.
- `tools/dev.py fixtures-check`, focused checker tests, and `docs/evidence/003.md`.

## Required fixture matrix

Several properties may share one small file. Every row must map to fixture IDs and verified properties in the manifest.

| Group | Required cases |
| --- | --- |
| Symbol structure | All five symbol kinds; universe access; dimensions 0, 1, 2 and 20; empty symbols and empty/default scalars |
| Values | All five variable/equation fields, subtype defaults, ordinary finite values, signed zero, special classes and acronyms; record upstream normalization explicitly |
| Keys | Nonlexical UEL registration order, repeated key prefixes, first/last changed dimensions, actual one/two/four-byte key-width boundaries and last-dimension delta transitions |
| Metadata | Strict and relaxed domains, aliases including universe aliases, comments, descriptions, set text associations, case-distinct labels, boundary string lengths |
| Compression | Uncompressed sections, zlib blocks, stored-block fallback, records crossing decoded block boundaries, multiple record boundaries inside one block |
| Operations | Raw and string generation; mapped ordering; append of a new symbol; duplicate key/symbol and domain-error observations |
| Encoding/layout variants | Endian markers and optional v7 metadata; byte-string validity behavior characterized against source/API |
| Rejection cases | v5/v6/unknown version markers, truncated headers/sections/blocks, impossible offsets/counts, invalid tags/dimensions and damaged zlib checksum |

## Implementation steps

1. Write expected semantic cases from explicit generator inputs before comparing oracle output. Normalize only documented upstream behavior. Full expected records, key order and metadata are required for small fixtures; counts alone are insufficient.
2. Generate through the pinned oracle where supported. Keep generator choices deterministic. If headers include nondeterministic fields, document them and compare semantically; hashes still identify the exact checked-in artifacts. Do not repeatedly rewrite manifests to hide drift.
3. Verify claimed byte-level properties using independent byte inspection or development-only instrumentation with a recorded patch. For example, a filename containing `compressed` does not prove a record crosses a block boundary. Keep upstream semantic oracle behavior unmodified.
4. Provide explicit mutation offsets/recipes and expected error categories for negative cases. A v7 file with its version number changed is only a version-rejection probe, not a genuine v5/v6 file. Mark that distinction in the manifest.
5. If an endian/layout fixture cannot be emitted directly by upstream, derive it with a narrowly specified independent transformation and oracle verification, or record the gap as blocking the affected compatibility claim. Do not reverse all file bytes or invent unverified fixtures.
6. Keep total checked-in binary fixtures initially below 16 MiB; use runtime generation for large data and >4 GiB offset experiments. If a required property cannot fit, document the reason and create a reproducible generated case rather than silently dropping it.
7. Make `fixtures-check` verify hashes, manifest references, required coverage, expected counts and full oracle semantic comparisons. Reject an empty corpus, duplicate IDs, missing expectations and unexplained skipped cases. Distinguish negative fixtures from valid fixtures in test totals.

## Acceptance checks

```sh
python3 tools/dev.py fixtures-check
python3 tools/dev.py oracle-self-test
python3 tools/dev.py check-bootstrap
git diff --check
```

Exercise checker failure paths in isolated temporary copies: change one valid expected record, remove one mandatory fixture, and alter one artifact byte. Each corresponding check must fail for the expected reason. Restore nothing by overwriting tracked files; use temporary inputs from the start.

Regenerate into a separate output directory and compare semantic results and deterministic artifacts with the tracked corpus. The checker must report actual file/symbol/record counts, negative-case counts and all unverified coverage. A documented required gap keeps this task incomplete; it is not a passing skip.

## Completion gate

The matrix is backed by real artifacts and reproducible evidence, and changing an expected record causes failure. No Rust decoder exists yet; no Rust interoperability or performance claim is justified. Unlocks task 004.
