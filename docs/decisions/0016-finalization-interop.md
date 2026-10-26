# Complete metadata and finalization qualification (task 022)

The existing transaction owns publication. Finish emits canonical symbol,
set-text, UEL, acronym and optional relaxed-domain sections by bounded journal
replay. It validates completed counts, data positions and set-member extents,
checks section offset ordering, then patches only the raw major index. It
flushes, syncs and closes before exclusive hard-link publication. A published
file survives post-publication cleanup failure; explicit abort retries owned
cleanup and never removes that destination. The existing std File close-error
and directory-durability limitations remain in decision 0012. Finalization needs
no new root capacity; fixed packets and existing codec/output buffers suffice.

The combined independent recipe exposed an incorrect task-020 assumption: native
GDX `gdxAddAlias(I,J); gdxAddAlias(J,K)` retains K's direct target J (ID 2) and
description `Aliased with J`. It does not flatten K to I (ID 1). Serialize the
original resolved reference after chain validation, for alias definitions as
well as strict domains. A forward A -> B -> I unit assertion previously expected
I (ID 3); task 022 corrects it to B (ID 2), demonstrated failing with the old
production code. This is a source/oracle-backed semantic fix, not a weakened test.
Historical task-020 evidence stating that definitions flatten is superseded here.

The native small recipe executor reopens raw UEL registration for each U directive
and deduplicates `a`/`A`. The new shared authoring fixture uses distinct `a`/`C`
labels so native authoring produces valid registered references. Existing frozen
case-distinct-label and Rust-written registration checks remain enabled; no label
normalization or frozen fixture changes are introduced.

The Rust example has four IDs and nine symbols, complete metadata plus records,
255-byte descriptions/comments, chained aliases, strict alias/universe domains,
relaxed domains, set text, acronyms, a 20-dimensional variable and scalar/default
views. Data sections deliberately differ from symbol ID order. Independent Python
expectations check both authoring directions across raw/zlib and supported feature
combinations, including canonical metadata offsets and section markers. Earlier
45-symbol subtype/width/block tests remain enabled. Native GDX remains a development
oracle only. A codec-stage fault hook joins existing output/metadata fault hooks;
tests visit individual finish I/O sites and verify publication/cleanup ownership.

Task 023 retains the larger-than-RAM, one-root pipeline, RSS and performance gates.
No append, mapped/unsorted writer, alternate dialect or new dependency is added.
