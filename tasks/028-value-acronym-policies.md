# 028 — Explicit special-value and acronym policies

Milestone: M4. Prerequisite: 027 complete. Status: complete for the scoped explicit value/acronym policy API.

Close the M4 special-projection/acronym inventory using explicit per-session
policies, preserving the core typed file view. Cover custom read/write sentinel
maps, resets, acronym identity/remapping/next IDs and invalid/colliding policies
against the pinned oracle. Keep ordinary bits, IEEE values, NA/UNDEF/EPS and
acronyms distinguishable; lossy conversion must be explicitly selected. Policy
state and acronym metadata remain quota-bounded; no global mutable mapping or
hidden full table. Verify all fields/features and failure/publication behavior.
Record 028 evidence and exact exclusions; no full C ABI claim.


Policy/ownership: [decision 0022](../docs/decisions/0022-value-acronym-policies.md).
Native API inventory/exclusions: [value/acronym status](../docs/value-acronym-api.md).
Qualification: [028 evidence](../docs/evidence/028.md), including source-bound
local and four-platform native checks. Task 029 is next eligible; M4 scale
acceptance remains task 031.
