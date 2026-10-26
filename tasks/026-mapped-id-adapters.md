# 026 — Bounded mapped UEL and record adapters

Milestone: M4. Prerequisite: 025 complete. Status: pending.

Specify caller-map ownership, missing/colliding entries, mapped ordering and
original raw-ID identity against the pinned oracle. Implement quota-controlled
mapped registration/write and mapped read streams; use external sorting when
mapping changes order. Reuse checked dictionary/merge primitives rather than a
full map or result collection. Keep raw APIs unchanged and original IDs available.
Validate complete order, values, duplicates, sparse/negative/out-of-range maps,
partial/error behavior, tiny batches/budgets and cleanup faults in both native
directions. Record 026 evidence and the mapped API inventory disposition.
