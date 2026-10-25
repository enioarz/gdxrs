# 016 — M2 out-of-core acceptance

Milestone: M2. Prerequisites: 012–015 complete. Status: complete.

Build deterministic streaming development generators and validating sinks. Execute separate record-heavy and metadata-heavy inputs, each exceeding the enforced process memory cap, with files/decoded records at least 4× that cap. Initial target: 256 MiB library budget under a 512 MiB OS cap with runtime headroom. Document actual OS containment method, RSS, tracked memory/disk/handles, seeds, source/compiler/hardware, file hashes, cache state and independent expected counts/ordered digests. Do not substitute merely requested quotas for an actual OS cap.

Test wide streamed outputs, huge dictionaries/request sets, leading and non-leading slices, exact limits, cold first batch, spill merge/cleanup and growing input sizes under a fixed cap. Measure time, CPU, actual bytes read/decoded and dispersion separately from output rows; compare equivalent decoded work with the pinned oracle where feasible. Confirm the full M1 regression corpus and all M2 slice/metadata/spill paths. Run required feature/platform checks, explicitly recording unavailable platforms. docs/evidence/016.md must map every M2 exit criterion to executed evidence and review findings. Only this completed gate qualifies the first out-of-core read release; no writer/index/parallel claim.

Completion: [runtime evidence](../docs/evidence/016.md), [machine results](../docs/evidence/016-results.json), and [qualification protocol](../docs/decisions/0010-m2-scale-qualification.md). Qualified on Linux x86_64 with the stated performance/platform limitations.
