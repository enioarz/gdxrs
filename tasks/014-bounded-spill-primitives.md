# 014 — Quota-controlled spill runs and merge

Milestone: M2. Prerequisite: 013 complete. Status: complete; [evidence](../docs/evidence/014.md).

Introduce narrowly scoped reusable spill primitives for byte-label/ID records: fallible bounded run buffers, sorted runs, bounded merge fan-in, memory/file-handle/temporary-byte quotas, exclusive scratch creation, checked lengths and robust cleanup. Define temporary-directory ownership, disk format/integrity, atomic completion and error behavior before coding. Incomplete data must never appear successful; no hidden unbounded path list or merge heap. Caller-selected limits are enforced before writes/growth; use std only unless a measured need is recorded.

Tests force multiple runs and merge passes, duplicates, invalid lengths, tiny quotas, cancellation, input/read/write/flush/disk-full-style faults and cleanup after every failure. Verify independently expected sorted output and memory/disk/handle high-water marks. Keep finalization errors visible. Retain exact task evidence in docs/evidence/014.md. Do not implement production GDX writing or persistent slice indexes.
