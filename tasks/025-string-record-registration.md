# 025 — Streamed string records and first-registration IDs

Milestone: M4. Prerequisite: 024 complete. Status: complete; [evidence](../docs/evidence/025.md).

The selected two-pass API, exact-byte policy, native authoring differences,
seven-handle minimum and occurrence-count limit are specified in
[decision 0019](../docs/decisions/0019-string-records.md).

Define a bounded string-key record adapter using a two-pass disk dictionary/remap
or immutable-run dictionary, selected with source/measurement evidence. Preserve
first-registration UEL order across symbols and exact label bytes/case. Stream
record mappings and external sort into 024; do not require retained token arrays,
a full hash map or lexical ID assignment. Bound dictionary, remap, record and
merge storage under shared root/scratch/handle caps. Keep malformed/duplicate/domain
behavior explicit and errors terminal unless a separately specified policy applies.
Compare complete string-generated metadata and streams upstream, including
repeated/non-UTF-8/case-distinct labels and large dictionaries. Force tiny budgets,
spill corruption/I/O failures and cleanup retries. Record 025 evidence; no mapped
IDs, append or M4 release claim.
