# 035 — Persistent slice acceleration acceptance

Milestone: M5. Prerequisite: 032–034 complete/reviewed. Status: in progress.

Qualify independently oversized records, metadata and sidecar directories/builds
under one 256 MiB library root/enforced 512 MiB Linux process cap, growing sizes,
>4×-cap physical/logical inputs, streamed sinks, measured RSS and explicit bounded
sidecar/temp bytes/handles. Preserve native semantic/record-ID validation and full
serial-vs-indexed ordered digests; keep reader, writer, M3/M4 and four-platform
gates enabled. Do not generalize Linux containment to other platforms.

Benchmark >=3 trials of cold serial, explicit build, strict full-source identity
validation/reopen, first indexed query and warm session reuse separately. Narrow
leading slices must materially reduce bytes/blocks/records decoded and warm
latency. Report validation/build break-even, skew and non-leading misses, disk
size/cache state, options, binary/source hashes, hardware/compiler/storage and
all failures/unrun cases. Warm timing alone does not establish cold performance.

Close source/cache validity, corruption/fallback, half-open interval and accepted
limit findings. Record source-bound structured evidence and self-review disposition;
no invented independent approval or M5 release from smoke alone. Optional writer
checkpoint capture/postings need separate measured follow-ups; they are not implicit
work or prerequisites without a documented change to this contract.

Command: `python tools/dev.py m5-check --profile smoke|scale`. The exact resource,
input-class and cache contracts are [decision 0030](../docs/decisions/0030-m5-acceptance.md).
Smoke cannot qualify this task; committed-source scale and preserved regression
gates are required. Evidence belongs in [035](../docs/evidence/035.md).
