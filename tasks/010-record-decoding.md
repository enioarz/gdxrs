# 010 — Serial record decoding

Milestone: M1. Prerequisites: 009.

Allowed files: src/, tests/, docs/, plus task/evidence records. Rules: F10, F11, F12, F13, F14, F19. Fixture IDs: key-widths, key-widths-be, semantics-0, semantics-1, blocks, bad-key-tag, bad-value-tag.

Preserve keys and typed special/acronym bits; decode all five equation/variable fields. Verify scalar physical/public count distinction. Invalid tags/counts/overflow must return contextual errors and terminal state. Decoder state fixed to 20 dimensions and bounded block buffers.

Acceptance: Full ordered fixture comparisons and negative/error-injection tests, locked checks and Clippy. Preserve input expectations; tests must exercise actual runtime behavior, including failure paths. Record exact counts, commands, revision, limits and unrun cases in docs/evidence/010.md. Review source invariants, fixture agreement and resource accounting before dependent work; substantive fixes require re-review. No milestone-wide implementation in one task.
