# 042 — Selectable gdxrs backend adapter

Milestone: M7. Prerequisite: 041 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Add an explicitly selectable gdxrs backend in `enioarz/polars-gdx`, pinned to the
qualified core revision. Adapt symbol metadata, domains, original UEL IDs, exact
label lookup and record access through the agreed contract. Preserve alias/universe,
scalar defaults, empty symbols and all value-field semantics. Keep the existing
backend available for differential tests and migration; do not silently retry it
after a gdxrs source, decode or resource failure.

Acceptance: existing relevant adapter tests run against both backends; independent
frozen expectations cover symbol kinds, aliases, case-sensitive labels, unknown
labels, special values/acronyms and malformed/unsupported inputs. The gdxrs-only
build and runtime path must work without GAMS, C++ or a native GDX library; legacy
backend/native-oracle requirements remain explicit development or optional paths.
No eager full-symbol or mandatory full-dictionary collection is introduced.

Evidence: `docs/evidence/042.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
