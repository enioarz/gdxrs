# 028 — Explicit special-value and acronym policies

Milestone: M4. Prerequisite: 027 complete. Status: implementation complete; qualification in progress.

Close the M4 special-projection/acronym inventory using explicit per-session
policies, preserving the core typed file view. Cover custom read/write sentinel
maps, resets, acronym identity/remapping/next IDs and invalid/colliding policies
against the pinned oracle. Keep ordinary bits, IEEE values, NA/UNDEF/EPS and
acronyms distinguishable; lossy conversion must be explicitly selected. Policy
state and acronym metadata remain quota-bounded; no global mutable mapping or
hidden full table. Verify all fields/features and failure/publication behavior.
Record 028 evidence and exact exclusions; no full C ABI claim.
