# 0003 — User-directed advancement to M1

On 2026-10-24 the user explicitly confirmed that M0 had already been independently reviewed and released, and that the repository's pending review snapshot was stale. This confirmation supersedes the earlier local assumption that M0 remained unreviewed. M1 proceeds on `m1-serial-reader`, based on M0 candidate `48e5b86e766c47a6782fde19b3ca7c6e33642673`. That SHA identifies this branch's starting content; it is not an invented release tag/version or a claim about a remote release SHA.

`docs/reviews/m0.json` records the explicit user confirmation while retaining the old checklist as historical data. M0 release approval is scoped to M0. Its stale whole-tree fingerprint must not block intentional new M1 files. No reviewer identity, release tag, release commit, or item-by-item checklist outcome is fabricated when those details were not supplied.

M1 is split into tasks 007–011 with individual commits/checks. Native reads are the scope; writing, selections, persistent indexes and Polars integration remain later milestones.

## Stable safe allocation constraint

The pinned compiler rejects `Arc::try_new` as unstable (`allocator_api`). Implementing a custom fallible shared owner would require unsafe code or another dependency. Instead, all file-sized/count-sized/dynamic data buffers use fallible exact reservations and root-budget guards. Fixed bounded source/handle/error control allocations through standard `Arc::new`/`Box::new` are charged conservatively before construction but follow Rust's standard process-aborting allocator OOM behavior. This limitation is explicit; recoverable failure for *every* allocation is not claimed. No file count controls their size, and quota exhaustion is still rejected before allocation. Codec state construction is bounded and charged separately. This refinement keeps the safe, minimal-dependency core and preserves the meaningful memory ceiling against data-sized allocations.
