# 040 — M6 performance and containment acceptance

Milestone: M6. Prerequisite: 036–039 complete/reviewed.
Status: pending.

Qualify ordered/cancelable parallel scans on growing independently oversized raw,
compressed and metadata-bearing workloads under one 256 MiB root and enforced
512 MiB Linux process cap. Include actual stacks, queued output/reorder peaks,
handles and cancellation/cleanup. Validate full native/serial/indexed/parallel ordered
digests, original IDs and special-value bits. Run >=3 trials; separate cold/warm,
build/reopen, first-batch, complete output and sink costs. Report workload-specific
gains, regressions, admitted concurrency and break-even against task 036 and current
serial; a serial fallback is not evidence of parallel benefit. Preserve all reader,
writer, M3/M4/M5 and four-platform gates. Publish exact scope and failure disposition,
not general throughput parity or an automatic release.

Optional parallel block compression, alternate codecs, SIMD/unsafe changes and
writer/metadata optimizations require separate measured tasks and invariant review;
they are not implicitly required or implemented by this read-concurrency gate.
No Polars integration belongs in core M6. Evidence: `docs/evidence/040.md`.
