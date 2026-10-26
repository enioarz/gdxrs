# 020 — Bounded raw registration and metadata spools

Milestone: M3. Prerequisite: 019 complete. Status: complete; [evidence](../docs/evidence/020.md), [contract](../docs/decisions/0014-writer-metadata.md).

Specify and implement streamed exact-byte UEL registration retaining one-based
first-registration IDs, ASCII-folded unique symbol names and complete metadata
spools under the writer root budget, aggregate scratch-byte/handle limits. Define
an explicit bulk fresh-dictionary API and bounded duplicate detection; no unchecked
unique-input promise, full dictionary/name-offset vector or silent lexical remap.
Reuse/generalize proven spill machinery with explicit budget ownership rather
than manufacturing a dummy Reader. Avoid simultaneous per-job quotas exceeding a
writer's total cap. Validate lengths/counts/references/alias chains and source IDs.

Spool descriptions, comments, set text, acronym and strict/relaxed domain metadata
incrementally, preserving ordinal/ID meaning and framing. Choose an explicit phase
contract for registration/freeze/symbol writing; resolve forward references before
publication. Test duplicate/case/raw bytes, empty/scalar/aliases, huge registration
with forced tiny runs, metadata corruption, quotas, errors/cleanup and original
IDs. Verify serialized metadata upstream as it becomes publishable; record 020
evidence. No general string/mapped/unsorted record writer (M4) or eager metadata.
