# 033 — Bounded persistent checkpoint sidecars

Milestone: M5. Prerequisite: 032 complete/reviewed. Status: pending.

Specify and implement a versioned disposable sidecar with fixed-size checkpoint
pages and a bounded disk-backed directory. Build incrementally in one explicit
serial pass, at configurable record/decoded-byte strides, including intervals
within huge first-key groups. Store half-open ordinal bounds, complete checkpoint
state and conservative leading/per-axis summaries. Never collect all checkpoints,
summaries or root-directory entries in RAM or modify the GDX file.

Define Off/UseIfValid/explicit Build policies and resource/error contracts first.
Bind source length, full content identity, metadata/schema/codec and index options;
validate a full source digest once per immutable open/session by default. Any
optional trusted identity shortcut must require an explicit immutable-snapshot
contract. A digest dependency requires the planned feature/license/allocation
audit; do not implement custom cryptography. Unkeyed checksums do not authenticate
malicious checkpoint authors: document the cache trust model and source-derived
validation needed before accepting serialized decoder state/summaries.

Enforce one root, page-cache cap, total sidecar/temp quota and handle admission.
Use exclusive temp ownership and explicit flush/sync/close/atomic no-clobber or
explicit replacement publication, preserving primary/cleanup failures and retries.
Validate versions, lengths/counts, offsets, overflow, page checksums and ranges
before allocation/seek. Reject stale/incompatible/corrupt caches before output.

Prove streamed build/reopen, small and >RAM index directories, interruption,
short writes, disk-full/cleanup failure, source mutation and corrupted/truncated
sidecars. Native frozen expectations remain the data truth. Record build and strict
identity-validation work separately. No query acceleration claim until task 034.
