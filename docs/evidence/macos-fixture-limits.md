# macOS fixture isolation correction

Checked revision: `bef020b`; working directory `/workspace/gdxrs`. User reported `Exception occurred in preexec_fn` on macOS after all ten valid dumps succeeded at merged M0 revision `0fec5c7`.

The old callback unconditionally lowered both soft and hard RLIMIT_AS and hid setup errors inside subprocess preexec_fn. A child launcher now applies limits before replacing itself with the native oracle. Darwin selects RLIMIT_DATA; Linux retains RLIMIT_AS. CPU/output/core limits and the five-second timeout remain. Existing tighter limits are preserved and hard limits are never raised or lowered. Launcher setup/exec errors use reserved exit 125 and are raised by the parent with a named diagnostic rather than recorded as rejected malformed input. Other platforms fail explicitly.

Darwin's data limit bounds heap/data growth, not all virtual mappings or RSS. No full macOS address-space containment claim is made. Actual macOS execution is unrun here; platform-choice and limit-failure behavior were verified with mocks. Linux native malformed-input checks ran successfully.

Validation on Linux: fixtures-check passed (10 valid, 12 negative); fixtures-regenerate-check passed (10 regenerated), and 26 tooling tests passed with zero failures/skips. Four new tests cover Darwin limit choice, preservation of inherited ceilings, named setup errors and launcher failure propagation. Final fixtures-check and 26-test rerun passed after adding the unsupported-platform guard. git diff --check passed.

Only generator provenance hashes were deliberately updated in the manifest because the launcher changed. All binary fixtures and expected semantic hashes remain unchanged; regeneration compared full semantics and bytes except the documented audit string.
