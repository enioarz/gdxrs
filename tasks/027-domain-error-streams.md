# 027 — Domain validation and bounded data-error streams

Milestone: M4. Prerequisite: 026 complete. Status: complete for the scoped deferred-domain and error-stream API.

Inventory pinned native domain checks/data-error APIs and define explicit Rust
strict/error-recovery policies, input ordinals and duplicate behavior. Retain
terminal rejection as the default. Any opt-in error stream must spool under a
quota or use a fallible bounded callback, preserve keys/typed values/provenance,
and never accumulate errors or silently lose rows. Resolve deferred/forward
membership validation without buffering domain sets or dependent symbols in RAM.
Require native-oracle expectations for accepted/error streams, aliases/universe,
strict/relaxed domains, duplicates and rollback/publication. Test error-output
quotas, callback faults, partial results and cleanup retry. Record 027 evidence.


Implementation/policy: [decision 0021](../docs/decisions/0021-domain-error-streams.md).
Native API inventory: [domain/error status](../docs/domain-error-api.md).
Qualification: [027 evidence](../docs/evidence/027.md), including source-bound
local and four-platform native checks. Task 028 is next eligible; M4 scale
acceptance remains task 031.
