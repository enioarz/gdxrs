# 005 — Reader and resource contracts

**Milestone:** M0. **Prerequisites:** task 004 complete. **Goal:** settle M1's observable behavior and memory ownership before implementation.

## Deliverables

- `docs/reader-contract.md`: concrete signatures/types and behavior for opening, metadata iteration, symbol lookup and serial row/batch cursors.
- `docs/resource-contract.md`: budget scope, allocation reservations, ownership, lifetimes, minimum working set, I/O bounds and errors.
- `docs/decisions/0002-value-and-label-model.md`: chosen value representation, special-value conversion and byte-string policy, justified by task 004.
- `docs/reader-acceptance.json`: scenario IDs tied to fixture IDs, operations, expected records/metadata, error categories and resource assertions for M1/M2.
- `tools/dev.py contracts-check` and `docs/evidence/005.md`.

## Decisions that must be closed

Choose one coherent initial design, not a list of alternatives. Record tradeoffs and defer unneeded performance optimizations explicitly.

| Area | Contract that must be explicit |
| --- | --- |
| IDs | One-based validated symbol/UEL types, raw conversion bounds, alias and universe representation |
| Values | All five fields, finite-value bit handling, distinct GDX special classes and acronym IDs; defaults and conversion APIs |
| Labels | Borrowed/owned byte representations, fallible UTF-8 conversion, exact label matching and distinct symbol-name rules |
| Sources | File-backed and any generic seekable interfaces; ownership, seek independence, immutability and truncation errors |
| Batches | Row/column layout, supported projection, byte/row capacity, reuse, returned lifetimes and error behavior after partial work |
| Metadata | Lazy iteration and lookup cost, budgeted caches, behavior without a metadata index; no mandatory full dictionary |
| Limits | Dimension/filter validation, zero limits, accepted-record counting, cancellation and exact terminal behavior |
| Errors | Categories and contextual fields for unsupported version, compression unavailable, invalid data, I/O, budget and source changes |
| Resources | Every allocation owner, shared-versus-cursor budgets, reservation release on error/drop, caller retention and worker-independent bounds |

## Required scenarios

- Open and inspect a small symbol without reading unrelated symbol data; metadata scans may be needed and must be described separately from record scans.
- Obtain the first batch without a full-symbol scan; repeated batches reuse bounded capacity.
- Scalar, empty/default scalar, every symbol kind, aliases and full value fields agree with fixture expectations.
- Non-UTF-8 bytes and special values retain identity unless the caller explicitly chooses a conversion.
- Unknown version, corrupt tags, overflow and out-of-file sections produce specific errors rather than panic/default substitution.
- Insufficient memory fails before the oversized allocation. Cache eviction and error/drop release reservations. A small batch request cannot allocate according to the total symbol record count.
- Caller-held prior batches are either charged to an owner until dropped or explicitly outside the scanner budget; the API must state which. Explain how a streaming caller stays bounded.
- Future filters/limits/projection and metadata spilling have specified extension points; do not implement or stabilize every M2 API prematurely.

## Implementation steps

1. Define signatures and state transitions in documentation using concrete types; remove unresolved placeholders from the M1 contract. Do not add unimplemented public production functions that compile by panicking.
2. Specify budget units and inclusions precisely, with worked small/large examples. Include vector capacities, compressed and decoded buffers, codec scratch, metadata pages, temporary lookup state and error paths. Explain how library allocation accounting differs from measured RSS.
3. For each M1 scenario, choose fixture IDs and specify the assertions future tests will make. A `Read + Seek` counting/fault-injection wrapper should measure reads/seeks and inject short reads/errors; identify which tests need a real filesystem.
4. Define a pre-implementation memory verification strategy: fixed budget, increasing dataset sizes, peak tracked allocation, RSS measurement and OS-enforced cap for later M2 tests. A hardcoded allocator counter that ignores the codec or dictionaries is unacceptable.
5. `contracts-check` validates that every required area/scenario is represented, references valid fixtures/rule IDs and has explicit expected observations. This is documentation coverage validation, **not an API behavior test**. Record that distinction in its output.

## Acceptance checks

```sh
python3 tools/dev.py contracts-check
python3 tools/dev.py spec-check
python3 tools/dev.py fixtures-check
git diff --check
```

Review the chosen model against both upstream semantics and the future Polars requirements. No mandatory full-file Enum/dictionary, eager records vector, native GDX handle or global mutex may leak into the contract. State which decisions remain for M2 and later; no M1-required decision can remain “TBD.”

## Completion gate

M1 has an implementable contract and fixture-linked acceptance scenarios, with a credible allocation model. No runtime behavior has been implemented by this task. Unlocks task 006.
