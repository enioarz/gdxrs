# 007 — Checked primitives and header validation

Milestone: M1. Prerequisites: Explicit user request to begin M1 after PR #1 merge; independent M0 review record remains pending.

Allowed files: src/lib.rs, src/format/mod.rs, tests/header.rs, plus task/evidence records. Rules: F01, F02, F03, F19. Fixture IDs: semantics-0, semantics-0-be, semantics-1, optional-domains-absent, bad-offset, truncated-header.

Validate uniform primitive patterns, v7 only, byte strings, compression flag, 80-byte index and physical offset relationships. Errors carry position and original I/O cause. No metadata/record scans; fixed stack storage, at most 626 successful header bytes consumed.

Acceptance: cargo test --locked; cargo clippy --locked --all-targets -- -D warnings; cargo fmt --all -- --check; cargo tree --locked. Preserve input expectations; tests must exercise actual runtime behavior, including failure paths. Record exact counts, commands, revision, limits and unrun cases in docs/evidence/007.md. Review source invariants, fixture agreement and resource accounting before dependent work; substantive fixes require re-review. No milestone-wide implementation in one task.
