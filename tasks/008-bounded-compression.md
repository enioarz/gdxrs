# 008 — Bounded compression blocks

Milestone: M1. Prerequisites: 007 and independent M0 acceptance.

Allowed files: src/format/, tests/, Cargo.toml, Cargo.lock, docs/, plus task/evidence records. Rules: F04, F19. Fixture IDs: blocks, damaged-zlib-checksum, truncated-block.

Audit codec dependency/licenses. Decode at most 32768 bytes per block with checksum verification and reject invalid framing/trailing streams as InvalidCompression. Measure codec scratch against the 128 KiB ceiling before approving the allocation design. Stored and zlib crossings must match fixture bytes.

Acceptance: Focused block tests, failure injection and allocation measurements; locked checks and Clippy. Preserve input expectations; tests must exercise actual runtime behavior, including failure paths. Record exact counts, commands, revision, limits and unrun cases in docs/evidence/008.md. Review source invariants, fixture agreement and resource accounting before dependent work; substantive fixes require re-review. No milestone-wide implementation in one task.
