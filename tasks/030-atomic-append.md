# 030 — Atomic copy/rewrite append

Milestone: M4. Prerequisite: 029 complete. Status: complete for atomic checked-rewrite append.

Implement append of new symbols via a separate no-clobber/copy/rewrite transaction,
with an explicit replacement policy for the original destination. Preserve prior
UEL/symbol IDs, metadata and existing data blocks where verified safe; otherwise
stream checked decoding/re-encoding under one root. Account for O(file-size)
copy/rewrite, aggregate scratch/output space and handles. Do not overwrite the
source until all new data/metadata validate, sync and close. No hidden in-place
append or speculative crash-recovery guarantee. Qualify raw/zlib, source mutation,
existing/late destination collisions, all publication/I/O phases, cancellation and
cleanup ownership natively on all four platforms. Record 030 evidence.

Implementation policy: [decision 0024](../docs/decisions/0024-atomic-append.md).
API inventory: [atomic append](../docs/append-api.md).
Qualification: [030 evidence](../docs/evidence/030.md) and source-bound local/four-platform results.
Task 031 is next; general-input scale/performance acceptance remains pending.
