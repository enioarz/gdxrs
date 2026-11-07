# Task 039 — code quality follow-up

Final implementation revision `78c93e0`; Rust cleanup revision `4e33250`.
The only changes between them are the policy benchmark harness and its Python
regression tests. Full reports, hashes and failed timing observations are retained
in [the structured record](039-code-quality-results.json).

The follow-up fixes checkpoint-test formatting and retains the reviewed checkpoint
preservation behavior. Plan accounting has one construction path for overrides,
conservative serial plans and measured choices. Indexed and parallel dispatch use
the same Ready-session validation; inconsistent status/owner combinations fail
before cursor construction. Public Rustdoc includes a compiling complete example.
Defaults are documented as adjustable guardrails rather than performance promises.

Policy-report validation requires four unique query shapes, three worker
measurements, positive VM observations and finite nonnegative query durations
(positive for nonempty queries). Results and source identities are checked before
owned inputs are retired. An unqualified progress snapshot and original inputs
remain available if a check fails. The 1.5x timing tolerance, worker choices and
input sizes are unchanged. No dependency, serialized format or default execution
choice changed.

## Validation

- Full `reader-check` passes: default 192/48/6/7; minimal 179/47/6/7;
  all features 238/59/8/8; index-only 223/58/8/8
  (unit/integration/ordinary-doctest/compile-fail counts). All suites run without
  skips or filters; ten frozen native fixtures / 141,358 public records match.
- All 11 policy unit tests and seven policy tooling tests pass.
- `m0-check` passes, including all 80 tooling tests.
- All-target/all-feature Clippy passes; reader-check also runs the remaining
  feature configurations. Formatting and local documentation links pass.
- Both capped `m6-policy --profile smoke` and `--profile small` pass on the final
  implementation. The small result requires committed, unchanged measured sources.

Three earlier timing failures remain in the structured record. At `4e33250`,
4,096-row all queries took 4.29 ms versus a 1.15 ms baseline (raw) and 11.27 ms
versus 1.32 ms (compressed); the larger profile passed. At `78c93e0`, smoke passed,
while a 32-row leading query on a 4M-label source took 2.07 ms versus 0.98 ms;
one unchanged small rerun passed. No tolerance or assertion was relaxed. The
new harness retained that failed query's source, sidecar and progress report.
The causes are not established; these observations do not establish stable tail
latency or a general speedup.

[Native CI](https://github.com/enioarz/gdxrs/actions/runs/42398605912) is refreshed
on the implementation; it was still running when this documentation was finalized.
This maintenance note is not independent approval, an M6 release or task-040
performance/oversized acceptance. Earlier qualification records remain tied to
their original revisions.
