# 022 — Complete v7 metadata/finalization interoperability

Milestone: M3. Prerequisite: 021 complete. Status: complete. [Evidence](../docs/evidence/022.md).

Finalize all metadata spools in canonical v7 order with bounded replay, validated
counts/references/section offsets, patch only the uncompressed major index, flush/
sync/close and publish through the 018 transaction. Cover complete current writer
metadata: descriptions/comments, set text, strict/relaxed domains, acronyms, aliases
and original UEL/symbol IDs. No public partial file on pre-publication failure.
Explicitly distinguish post-publication cleanup failure from successful finish.

Build a deterministic bounded Rust fixture authoring example and independent
expected metadata/record recipes. Require Rust writes→pinned native reads AND
native writes→Rust reads, stored/zlib and default/minimal/all features. Native CI
must preserve functional tests on all four platforms, even if oracle compilation
needs a separate qualified development runner. Test allocation, write, seek, codec,
sync, exclusive-publication and cleanup failures at each phase. Record full
interop coverage/failures and 022 evidence. No append/unsorted/mapped writer (M4).
