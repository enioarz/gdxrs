# 015 — Large exact label requests and unique domains

Milestone: M2. Prerequisite: 014 complete. Status: pending.

Use the bounded spill machinery to resolve request streams larger than RAM against streamed UEL metadata without an in-memory offset vector/map. Sort/merge exact bytes while preserving original IDs and request semantics; integrate spill-backed membership with selection evaluation under one root quota. Define whether request ordinals/results are streamed or externally reordered, with explicit error/drop ownership.

Provide unique-domain enumeration from a selected record stream using bounded external sort/deduplication; retained dimensions/IDs must preserve their meaning. Avoid a label bitmap sized by the full UEL universe. Specify public lending/streaming APIs before coding and keep reference/source identity explicit. Compare small memory/spill paths with independent expectations, including non-leading filters, unknown labels, case distinctions, duplicates, wide results and exact limits. Exercise multi-pass runs, errors, quota exhaustion and cleanup. Record evidence in docs/evidence/015.md; scale qualification remains task 016.
