# PR 1 review fixes

Checked revision: `f89e2c02f7e8bda01c101409264572ba97d63e5e`. Working directory: `/workspace/gdxrs`.

R1 fixed: fresh successful benchmarks atomically select a validated current report. `m0-check --baseline PATH` supports explicit selection. Aggregate failures retain component outcomes. R3 fixed: report hashes must match measured files at the claimed Git commit and current sources; dirty benchmark sources cannot be measured under an unrelated revision.

R2 fixed: independently specified nonempty equations now store five distinct fields in both compression modes. Coverage rejects missing physical equation rows. Fixtures were deliberately regenerated; the F14 ledger additionally records the new equation subtype entry. Other observed rules were unchanged. Historical task evidence describes the earlier corpus.

Validation: 22 tooling tests passed, no failures/skips; full fixture regeneration passed; bootstrap, oracle smoke, fixture, specification and contract gates passed. Current corpus: 10 valid / 12 negative files, 122 symbol entries, 5153 stored rows. Small benchmark: four scenarios, 24 writes and 44 validating scans. Aggregate exit 2 is now due only to pending independent review. See [aggregate results](pr1-fixes-results.json) and [fresh baseline](pr1-fixes-baseline.json).

Reviewer became the fix author in this session; independent re-review remains pending. No M1 implementation or completion approval is implied. The original baseline document remains a historical measurement; fresh validation uses the current-run pointer or explicit report path.
