# 046 — Scan lifecycle, threads and cancellation

Milestone: M7. Prerequisite: 045 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Integrate the qualified execution policy and explicit thread options with the
adapter's synchronous/Python/Polars lifecycle. Define requested/admitted concurrency
and prevent unbudgeted nesting of Polars and core worker pools. Release the GIL only
under valid ownership rules; do not import the legacy wrapper's global-lock or
unsafe Send assumptions. Preserve independent repeated scans and source identity.

Expose fallible close/cancellation, wake blocked producers/consumers and join all
owned workers when a query stops, reaches its global limit or fails. Propagate core
errors and secondary cleanup diagnostics without replaying acknowledged rows.
Document that blocked synchronous OS reads cannot be forcibly interrupted.

Acceptance: consumer drop, sink exceptions, Python cancellation, slow first
partitions, bounded queue pressure, partial output, spawn/read/decode/source errors,
concurrent scans and repeated execution. Verify file/scratch release, especially
on Windows, and joins/admitted concurrency on all four native platforms. Confirm
that retained batches/handles keep only their documented owners and reservations.

Evidence: `docs/evidence/046.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
