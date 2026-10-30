# 031 — General-write and modern semantic acceptance

Milestone: M4. Prerequisites: 024–030 complete. Status: in progress.

Audit the M4 native API inventory, record exact implemented semantics/exclusions
and close required parity gaps before claiming milestone completion. Execute
independently oversized unsorted/string data and dictionaries, mapped adapters,
error streams and append pipelines under the established 256 MiB shared root /
enforced 512 MiB process cap where supported. Require >4×-cap physical/logical
workloads, growing sizes, complete native counts/ordered digests/metadata IDs,
measured RSS, actual containment, quotas/handles, hashes and source/hardware/tool
provenance. Benchmark registration/remapping, sorting/merge, encoding and
copy/finalization separately against equivalent native work with repetitions and
explicit option/durability differences. Keep all reader/M3, native-platform and
both interoperability gates enabled. Record findings/disposition and 031 evidence.
No M4 release follows merely from successful task 024 or smoke tests.

## Deliverables and execution

- [Current API inventory](../docs/m4-api.csv), checked against the pinned YAML and
  the frozen M0 milestone roster with `python tools/dev.py m4-api-check`.
- `examples/m4_worker.rs` and `tools/m4/driver.cpp`: independent complete
  ordered-value/metadata sinks and explicit native composite timing reference.
- `python tools/dev.py m4-check --profile smoke`: six calibrated recipes, eight
  repeated benchmark configurations, growing small cases. Never qualifies M4.
- `python tools/dev.py m4-check --profile scale`: committed-source snapshots,
  growing >4×-cap families, RSS/quotas/handles, full validation and SHA-256 before
  generated artifact retirement. The 256 MiB root and 512 MiB OS cap are fixed;
  the eager native reference uses a separately recorded 16 GiB cap.
- `python tools/m4_boundary.py PATH_TO_ACCEPTANCE_NATIVE_SNAPSHOT` additionally
  calibrates every recipe at 131,073 inputs, including key wrap and diagnostic
  ordering across multiple 65,536-row blocks; fixed small tooling memory only.
- Preserve reader-check, writer-check, label-pages-check, m0-check,
  m3-writer-check and four-platform CI. Record findings and remaining gates in
  [031 evidence](../docs/evidence/031.md). Independent review remains a separate
  gate; successful smoke never closes this task.
