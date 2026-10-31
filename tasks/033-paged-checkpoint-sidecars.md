# 033 — Bounded persistent checkpoint sidecars

Milestone: M5. Prerequisite: 032 complete/reviewed. Status: in progress.

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

## Deliverables

`IndexOptions`, Off/UseIfValid open status, explicit owned `IndexBuilder`, paged
`CheckpointIndex`/`IndexInterval`, optional audited `index` digest feature, and
versioned source-verified sidecar bytes; see decisions 0027/0028. Build performs one
serial record pass plus a separately timed whole-source digest pass. Strict reopen
regenerates source-derived state/summaries before trusting any serialized bytes.

`python tools/dev.py index-check --profile smoke|scale` qualifies the paged
container separately from task 035: an 8 MiB shared root / enforced 64 MiB process
with a directory independently larger than that process cap, growing sizes, exact
native source validation, source SHA-256 checks and full canonical page replay.
This does not replace task 035's 256/512 MiB >4× milestone/query gates. Source-bound
reports and exact four-platform feature results belong in docs/evidence/033.md.
