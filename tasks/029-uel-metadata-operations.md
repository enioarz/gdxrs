# 029 — Modern UEL and metadata operations

Milestone: M4. Prerequisite: 028 complete. Status: complete for the scoped modern UEL/metadata API.

Close remaining M4 UEL/metadata inventory (including rename, mapped lookup,
set-text node numbers and mutable metadata operations) with native expectations
and explicit phase rules. Preserve original IDs, exact-byte label matching,
folded symbol matching and all alias/domain references. Use bounded disk updates
or rewrite passes rather than retaining tables. Define collisions, invalid IDs,
frozen-state mutation and error rollback. Exercise large/long labels, duplicate
names, sparse text nodes, callbacks, quota/I/O faults and atomic finalization.
Record 029 evidence with API-by-API implemented/unsupported disposition.


Policy/ownership: [decision 0023](../docs/decisions/0023-uel-metadata-operations.md).
API-by-API disposition: [UEL/metadata status](../docs/uel-metadata-api.md).
Qualification: [029 evidence](../docs/evidence/029.md), including source-bound
local and four-platform native checks. Task 030 is next eligible; M4 scale
acceptance remains task 031.
