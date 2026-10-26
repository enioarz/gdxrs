# Instructions for contributors and coding agents

## Start here

Read [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md), [tasks/README.md](tasks/README.md), and the assigned numbered task before editing. The plan describes the architecture; task specifications define the next deliverable and its acceptance checks. If they conflict, identify the conflict and resolve it explicitly rather than choosing whichever requirement is easier. User instructions take precedence.

Use the existing checkout. Each cloud task is already isolated; do not create a Git worktree unless explicitly requested. Check `git status` first and preserve existing work. Do not modify the research repositories or `polars-gdx` as part of a core task.

The repository contains a bounded serial Rust GDX v7 reader, pinned oracle, verified fixture corpus, format/API specification, reader contracts and baseline/review tooling. M0 is released by explicit user confirmation. M1 is merged; M2 includes bounded exact raw-ID slicing, small exact-byte label lookup and optional quota-controlled label pages. Quota-controlled scratch sorting is available for M2. Spill-backed large lookup/domain integration is available. Task 016 has qualified separately oversized records and metadata under an enforced OS address-space cap on Linux x86_64. Task 017 also qualifies native reader functionality on macOS ARM64/Intel and Windows x64/MSVC. Cross-platform OS memory-cap/performance results remain unmeasured. M3 has begun: tasks 018–020 add bounded v7 creation, explicit atomic publication, raw/stored/zlib sections, disk-backed registration/metadata and empty symbols. Task 021 adds borrowing sorted raw record encoding with checked bounds, typed values and disk-backed strict-domain membership; see docs/decisions/0015-sorted-record-writer.md. Domain sets must be completed before dependent symbols. Full finalization qualification and the writer release gate remain pending in tasks 022–023. Metadata phases, quotas and the bulk fresh-dictionary API are specified in docs/decisions/0014-writer-metadata.md; use bulk registration for large dictionaries and retain its first-registration IDs. Incremental name/reference scans may be quadratic. The encoder allocation audit is docs/decisions/0013-bounded-writer-codec.md; do not assume its private fixed boxes are fallible. Indexes, parallel reads and Polars integration remain future work. The measured wide uncompressed read gap is closed by the checked scalar-buffer fast path in docs/evidence/016-throughput.md. Retain its workload scope, original timings and platform limits; do not generalize it into cross-workload throughput parity. Consult the task queue and evidence for current capabilities. Commands and files specified under a pending task's **Deliverables** are future work; do not claim that its tests, fixtures, or performance baselines already exist.

## Scope and task discipline

- Work on the assigned task and satisfy its prerequisites. If no task was named but implementation was requested, select the earliest eligible task in `tasks/README.md` and state that choice.
- Do not start a dependent task before its gate passes. Do not implement an entire milestone in one change or add speculative scaffolding for later modules.
- Preserve the task's acceptance criteria. A failing test is not authorization to weaken its assertion, replace its expected output, skip it, or loosen a threshold.
- Document a source contradiction or necessary design change with evidence. Resolve routine implementation choices autonomously within the task. Escalate only decisions that change an agreed contract or prevent useful progress.
- Keep future tasks marked pending. Track progress in `tasks/README.md` and put completion evidence in `docs/evidence/NNN.md`, naming the code revision the checks exercised.
- Commit only intended task files when asked or when continuing an already authorized commit workflow. Do not push, publish, or change remote branches without applicable user authorization.

## Non-negotiable implementation constraints

1. **GDX v7 only initially.** Reject v5/v6 and unknown future versions explicitly. A GAMS/library release number is not a file format version. No speculative fallback decoder.
2. **Independent Rust core.** Production builds must not link GDX, run GAMS tools, compile C++, or require Python. The pinned C++ implementation is a development oracle only.
3. **Minimal dependencies.** Start with `std`. Compression is the planned exception, tentatively `miniz_oxide`; add it only in the codec task with its transitive dependency/license audit. Arrow, Polars, PyO3, Serde, Rayon, async runtimes and database engines do not belong in the core. Changes to this policy require a recorded design rationale and review.
4. **Bounded memory includes metadata.** Account for output capacities, UEL dictionaries, filters, checkpoints, caches, workers, merge queues, error lists and temporary buffers. Do not hide a full input, symbol, metadata table or index in `read_to_end`, `collect`, a `Vec`, a hash map, or a whole-file mapping. Bounded batches and explicitly budgeted small-fixture tooling are allowed; document the bound.
5. **Bounded disk and handles too.** Spill work needs a temporary-space quota, bounded merge fan-in, exclusive temp-file creation, error propagation and cleanup. No implicit unbounded cache or background queue.
6. **Untrusted input must fail safely.** Check offsets, counts, dimensions, tags, additions, multiplications and integer conversions before allocation/access. No `unwrap`, `expect`, unchecked indexing or assertions on unvalidated file data. Do not silently saturate, clamp, truncate or substitute defaults for corrupt data. Fixed internal invariants may be asserted with justification.
7. **Preserve semantics.** Keep UEL IDs one-based in the core, distinguish symbol-name and label matching rules, preserve special-value identities and defaults, and never silently turn NA/UNDEF into NaN, EPS into zero, or invalid IDs into the first label.
8. **Streaming is the default.** No full-symbol prescan to satisfy a short read. Limit counts mean accepted records, not decoded records. Explicit collection is a separate consumer choice. Projection does not imply skipping encoded fields that must be consumed for framing.
9. **No unsupported seek assumptions.** A compressed block boundary is not a record boundary; a record boundary is not necessarily an independent restart. Persist and verify complete decoder state before checkpoint/parallel work.
10. **Safe Rust first.** No unsafe code, SIMD, memory mapping, alternate native codecs or global decoder locks without a measured need, a reviewed invariant and focused verification. Do not use speculative optimizations to bypass checks.
11. **Errors stay visible.** Preserve command exit status; distinguish partial output from success. Explicit writer finalization reports errors. Never silently retry a failed streaming scan after output has escaped, duplicating rows.
12. **Verification remains enabled.** Keep TLS, package checksums and zlib integrity checks. Never update expected hashes merely because a download or fixture check failed. Investigate and document deliberate fixture changes.

13. **Preserve native platform compatibility.** Keep the Linux, macOS ARM64/Intel and Windows x64/MSVC CI matrix green. Do not replace native tests with cross-compilation, skip failing feature suites, suppress platform warnings or make portable reader tooling depend on Unix-only modules. See [task 017 evidence](docs/evidence/017.md) for scope.

## Verification and evidence

- Use the assigned task's commands from the repository root. Commands that do not exist yet are implementation requirements, not permission to skip the gate.
- A zero-test run, compile-only check, Rust-to-Rust round trip, or matching row count alone does not demonstrate GDX interoperability. Require independent expected metadata/records and both cross-implementation directions once the writer exists.
- Report exact test/fixture counts, failures, skips and unrun cases. Missing tools, credentials or platforms are limitations, not passes. Do not mark a check complete from an old report or from its intended behavior.
- Keep ordinary numeric bits and tagged special values distinguishable in test expectations. Keep fixture hashes, generators, source revisions and provenance together.
- Performance reports must identify source revision, hardware, compiler, inputs, cache/index state, result validation and measurement method. Include index creation and validation costs separately. Never claim a speedup from selecting fewer output rows without measuring decoded work.
- Library budget accounting is not an RSS limit. Out-of-core claims require a streaming consumer, independently oversized metadata tests, measured memory and an enforced OS limit where supported.
- Review gates are technical checks, not automatic requests for user permission. Complete authorized work first. A review must record findings and disposition; do not invent an independent reviewer or claim self-review is independent. If a required independent review is unavailable, finish the evidence and leave dependent work gated.

## Completion report

Summarize the task, changed files, checks actually run, observed counts/results, remaining risks, and the next eligible task. Link the evidence record. For documentation-only changes, validate links and the diff; do not install toolchains or manufacture passing runtime tests.
