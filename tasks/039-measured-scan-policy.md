# 039 — Measured explicit scan policy

Milestone: M6. Prerequisite: 038 complete/reviewed with measured costs.
Status: pending.

Use reproducible measurements to choose between serial, indexed and parallel scans
within caller-specified limits. Include build/strict reopen costs and already-valid
session state; never build/open an index implicitly. Keep explicit overrides and
observable choices/costs. Choose conservatively for head/zero limits, small inputs,
poor pruning and admission pressure. Do not scan a whole source or directory just
to choose a plan. Preserve exact filters, global limits, source trust and acknowledged
progress across every choice. Require both deterministic policy tests and actual
end-to-end comparisons with held-out representative inputs. No unmeasured universal
threshold or hidden unbounded cache. Evidence: `docs/evidence/039.md`.
