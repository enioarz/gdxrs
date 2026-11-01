# 037 — Bounded ordinal-range cursors

Milestone: M6. Prerequisite: 036 baseline/contract complete and reviewed.
Status: complete for serial ranges; PR #32 awaits review. See [evidence](../docs/evidence/037.md).

Implement the serial range primitive needed before parallel execution. Define exact
half-open raw record ordinals, validate start/end/count arithmetic and source/symbol
identity, resume from a trusted checkpoint without decoding its prefix, and stop
at the exact range end. Filtering/projection do not change raw ordinals. Empty and
zero accepted-limit ranges stay lazy, including empty `[total,total)` ranges.
A nonempty range consumed through source EOF retains framing validation unless
its accepted limit stops first. Do not expose private checkpoint import or skip
strict sidecar trust.

Charge range state/checkpoints/batches to the existing Reader root; never collect
partitions. Specify explicit session ownership, checkpoint lifetime and errors before
implementation. Verify split/recombined ordered output against independent frozen
fixtures, including compressed/shared blocks, default scalars, aliases/universe,
special values, exact boundaries, malformed ranges, stale sources, limits and budget
denial. Keep all reader/writer/native platform gates. No threads, automatic planner,
new dependency or performance qualification belongs here. Evidence: `docs/evidence/037.md`.
