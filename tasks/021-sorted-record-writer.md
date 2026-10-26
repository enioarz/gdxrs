# 021 — Sorted raw symbol records

Milestone: M3. Prerequisite: 020 complete. Status: in progress.

Expose a borrowing symbol writer over Writer's shared budget/output. Accept
registered typed UEL IDs and typed values, with validated dimension/kind/subtype,
supplied inclusive key bounds or conservative 0..i32::MAX widths. Require strictly
increasing lexicographic raw keys; duplicates/out-of-order rows are errors, not
last-wins. Check references and declared strict-domain membership with bounded
storage; preserve aliases/universe and scalar/default semantics. No full-symbol
prescan/buffer, speculative restart, or compressed-header backpatch.

Encode verified v7 first-change/delta keys, widths 1/2/4, value tags and ordinary
bits/acronyms/special identities, checked counts and terminators. Failed rows or
abandoned symbol writers make publication impossible. Test dimensions 0/1/2/20,
all types/subtypes/fields, empty/default scalars, exact transitions, bounds, malformed
caller input, codec boundaries, partial writes and root/output limits. Compare
complete Rust-written streams upstream and against independent expectations;
record 021 evidence. Whole-file metadata finalization remains 022.
