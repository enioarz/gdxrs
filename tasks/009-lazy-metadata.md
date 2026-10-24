# 009 — Lazy metadata navigation

Milestone: M1. Prerequisites: 008.

Allowed files: src/, tests/, docs/, plus task/evidence records. Rules: F05, F06, F07, F08, F09, F15, F17. Fixture IDs: semantics-0, semantics-1, case-distinct-labels, key-widths.

Implement positioned local source and shared reservation accounting; lazy symbol/UEL/domain/text/acronym views. Missing references and cycles fail explicitly. Counting sources prove no unrelated record reads; large dictionaries cannot require proportional retained RAM.

Acceptance: Fixture metadata comparison, counted reads, quota rejection/drop tests and locked checks. Preserve input expectations; tests must exercise actual runtime behavior, including failure paths. Record exact counts, commands, revision, limits and unrun cases in docs/evidence/009.md. Review source invariants, fixture agreement and resource accounting before dependent work; substantive fixes require re-review. No milestone-wide implementation in one task.
