# 023 — M3 bounded writer acceptance

Milestone: M3. Prerequisites: 018–022 complete. Status: complete. [Evidence](../docs/evidence/023.md).

Execute independently oversized records AND registration/metadata writing with
streaming generators/sinks under the established initial 256 MiB shared library
budget / enforced 512 MiB process cap where supported. Include a read→write pipeline
under ONE root budget, native validation of complete counts/ordered digests and
metadata IDs, >4×-cap physical/logical datasets, growing sizes, measured RSS,
logical root/scratch/handles, input/output hashes, seeds/hardware/compiler/cache
state and actual OS containment. Do not extend Linux-only measurements to other OSes.

Benchmark sorted raw encoding, metadata/registration, compression and finalization
costs separately against equivalent native work with repetitions/dispersion. Keep
short/failed I/O, duplicate/domain errors, cancellation, no-clobber publication and
cleanup qualification enabled. Preserve all reader/M2/native-platform gates and
both interoperability directions. Record findings/disposition and M3 API inventory
status in 023 evidence. Only this gate completes M3; general input sorting/string
registration adapters/append remain M4 and are not implied by partial writer success.
