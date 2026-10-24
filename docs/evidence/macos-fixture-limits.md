# macOS fixture isolation correction

Checked revision: `bef020b`; working directory `/workspace/gdxrs`. User reported `Exception occurred in preexec_fn` on macOS after all ten valid dumps succeeded at merged M0 revision `0fec5c7`.

The old callback unconditionally lowered both soft and hard RLIMIT_AS and hid setup errors inside subprocess preexec_fn. A child launcher now applies limits before replacing itself with the native oracle. Darwin selects RLIMIT_DATA; Linux retains RLIMIT_AS. CPU/output/core limits and the five-second timeout remain. Existing tighter limits are preserved and hard limits are never raised. On Linux they remain unchanged; the dedicated Darwin child now lowers them to finite requested ceilings because reapplying a reported unlimited hard ceiling failed on the user’s Mac. Launcher setup/exec errors use reserved exit 125 and are raised by the parent with a named diagnostic rather than recorded as rejected malformed input. Other platforms fail explicitly.

Darwin's data limit bounds heap/data growth, not all virtual mappings or RSS. No full macOS address-space containment claim is made. Actual macOS execution is unrun here; platform-choice and limit-failure behavior were verified with mocks. Linux native malformed-input checks ran successfully.

Validation on Linux: fixtures-check passed (10 valid, 12 negative); fixtures-regenerate-check passed (10 regenerated), and 26 tooling tests passed with zero failures/skips. Four new tests cover Darwin limit choice, preservation of inherited ceilings, named setup errors and launcher failure propagation. Final fixtures-check and 26-test rerun passed after adding the unsupported-platform guard. git diff --check passed.

Only generator provenance hashes were deliberately updated in the manifest because the launcher changed. All binary fixtures and expected semantic hashes remain unchanged; regeneration compared full semantics and bytes except the documented audit string.

Follow-up: the user’s actual macOS rerun rejected the reported unlimited hard RLIMIT_DATA ceiling with `current limit exceeds maximum limit`. The child now supplies finite hard ceilings on Darwin for all four limits, preserving any tighter inherited soft/hard bounds. Two regressions model rejection of unlimited hard values and preservation of tighter Darwin bounds. Actual macOS rerun remains required; mock checks do not prove kernel behavior.

Follow-up validation on Linux: fixtures-check (10 valid / 12 negative) and full 10-fixture regeneration passed; 28 tooling tests passed with no failures/skips. Binary and expectation hashes remain unchanged. Only launcher provenance changed.

## Superseding macOS policy after second failed native rerun

The finite-hard-ceiling workaround also failed on the user's Mac with the same memory-limit error. The current launcher therefore does not attempt either RLIMIT_AS or RLIMIT_DATA on Darwin. This is an explicit platform limitation, not a successful memory-isolation test: command reports expose `oracle_isolation.memory_limit = unavailable`, and fixtures-check prints the limitation. CPU/output/core limits and the five-second timeout remain mandatory; their setup errors still fail. Linux retains its address-space cap. Do not use macOS fixture runs as evidence of memory containment.

Validation of this change on Linux: all 10 valid / 12 negative fixture checks passed, all 10 regenerated fixtures compared successfully, and 28 tooling tests passed with zero failures/skips. The Darwin mock now rejects both memory APIs and unlimited hard ceilings while verifying that the three supported controls are applied. Actual Darwin execution remains unrun in this environment. Fixture artifacts and semantic expectations remain unchanged; provenance hashes track the changed launcher/runner.
