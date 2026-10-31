# 034 — Exact indexed selected scans

Milestone: M5. Prerequisite: 033 complete/reviewed. Status: in progress.

Integrate validated sidecars with explicit Reader policies. Plan bounded streamed
candidate intervals from conservative summaries; scan/filter exact raw IDs and
preserve order, projections and globally accepted limits. Skip only proven
nonmatching intervals, coalesce adjacent spans, and prefer sequential scans when
pruning is poor. No hidden build or prescan for a short query. Restart only at
validated complete checkpoints; respect half-open ranges and pending terminators.

Missing/stale/corrupt sidecars may fall back to correct serial scans only before
output. A failed checkpoint/interval after output begins is terminal: retain
acknowledged work/error context and never silently restart/duplicate rows. Honor
source integrity, existing lending/cancellation semantics and one root/page-cache
budget. Count actual decoded/emitted rows, physical bytes/buffers, intervals
considered/pruned, cache hits and validation/build work separately.

Compare all indexed results with independently authored fixtures and serial
selection: leading/non-leading/empty filters, long groups, key-width/delta and
cross-block records, aliases, scalars, universe, zero/exact limits, malformed
sidecars and source changes before/during output. Include fail-closed fault and
resource tests on four native platforms. Do not add parallel execution or postings
without evidence and a separate specification. Task 035 remains the exit gate.
