# 011 — Serial lending batch cursors

Milestone: M1. Prerequisites: 010.

Allowed files: src/, tests/, docs/, plus task/evidence records. Rules: F10, F12, F13, F14, F15, F19. Fixture IDs: blocks, semantics-0, semantics-1.

Implement reader-contract lending cursors, projection framing, accepted-row limits and zero-limit no-data-read behavior. Enforce shared budgets, fallible allocation and no partial batch on failure. Prior views cannot survive next fill; first batch requires no symbol prescan.

Acceptance: reader-acceptance M1 scenarios, borrow compile-fail checks, allocation/read counters, full oracle comparisons and locked checks. Preserve input expectations; tests must exercise actual runtime behavior, including failure paths. Record exact counts, commands, revision, limits and unrun cases in docs/evidence/011.md. Review source invariants, fixture agreement and resource accounting before dependent work; substantive fixes require re-review. No milestone-wide implementation in one task.
