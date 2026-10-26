# 018 — Writer contract and atomic empty-v7 foundation

Milestone: M3. Prerequisites: 001–017 complete/merged. Status: complete; [evidence](../docs/evidence/018.md).

Specify M3's concrete ownership, root-memory, output/scratch, ordering, registration,
metadata, encoding and failure contracts before implementation. Split remaining
M3 work into bounded numbered tasks; no entire-milestone implementation here.

Implement the first useful public writer: create/finalize a valid empty modern v7
file, plus explicit cancellation. Support standalone root budgets and a reader-
shared root constructor for the eventual bounded read→write pipeline. Little-
endian uncompressed output only at this stage. No symbol/UEL registration stubs.

Use a fallible fixed output buffer, bounded path/control allocations, checked
positions/output quota and exact short/interrupted-write handling. Reserve before
growth. Write a same-directory exclusive private temporary file, finalize/flush/
sync and close it before atomic no-clobber publication. Never overwrite an existing
file/symlink, including a destination created after writer open. Surface failure,
terminal state, physical writes versus logical extent, publication state and
secondary cleanup errors. Explicit abort permits cleanup retry; Drop is best effort
and never publishes. Report post-publication cleanup failure without deleting the
valid published destination or falsely reporting complete.

Verify native GDX reads Rust-written empty output against an independent expected
semantic stream and byte layout, not just a Rust round trip. Test exact quota
boundaries, shared root denial, allocation/create/short/zero/interrupted write,
seek/sync/publication/removal faults, collisions, Unicode paths, cancellation,
terminal errors, publication timing, cleanup retry and final owner budget release.
Preserve complete reader regression and native Linux/macOS ARM64+Intel/Windows CI.
Record code-bound evidence in docs/evidence/018.md. Future writer tasks remain pending.
