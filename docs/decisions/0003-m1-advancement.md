# 0003 — User-directed advancement to M1

On 2026-10-24 the user explicitly requested continuing M1 implementation. M1 proceeds on `m1-serial-reader`, based on M0 commit `48e5b86e766c47a6782fde19b3ca7c6e33642673`, under that instruction. This is an exception to the local prerequisite gate, not independent review or an approval fabricated by the implementation author. `docs/reviews/m0.json` remains pending. M0 task 006 is not retroactively marked complete.

M1 is split into tasks 007–011 with individual commits/checks. Native reads are the scope; writing, selections, persistent indexes and Polars integration remain later milestones.

## Stable safe allocation constraint

The pinned compiler rejects `Arc::try_new` as unstable (`allocator_api`). Implementing a custom fallible shared owner would require unsafe code or another dependency. Instead, all file-sized/count-sized/dynamic data buffers use fallible exact reservations and root-budget guards. Fixed bounded source/handle/error control allocations through standard `Arc::new`/`Box::new` are charged conservatively before construction but follow Rust's standard process-aborting allocator OOM behavior. This limitation is explicit; recoverable failure for *every* allocation is not claimed. No file count controls their size, and quota exhaustion is still rejected before allocation. Codec state construction is bounded and charged separately. This refinement keeps the safe, minimal-dependency core and preserves the meaningful memory ceiling against data-sized allocations.
