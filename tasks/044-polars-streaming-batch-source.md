# 044 — Cursor-backed Polars batch source

Milestone: M7. Prerequisite: 043 complete/reviewed.
Status: pending.
Implementation repository: `enioarz/polars-gdx`.

Replace the eager full `RawSymbolData` path for the gdxrs backend with a real
cursor-backed batch source in `polars-gdx`. Honor `batch_size` as the agreed maximum
rows per emitted batch and enforce the separate byte budget for wide rows/labels.
Support projection and stable schemas; emit incrementally without accumulating the
whole symbol or dictionary. Ensure each yielded Arrow/Polars batch owns valid data
after the lending core cursor advances or closes. Reuse or transfer storage only
where ownership permits; do not promise universal zero-copy conversion.

Acceptance: first-batch tests establish no hidden full-symbol prescan, with explicit
index build/strict validation costs distinguished when that mode is requested.
Compare concatenated small-fixture batches with native expectations at one-row,
uneven and large batch sizes, zero-column projections and empty symbols. Retain an
earlier batch while advancing/closing to verify lifetime safety. A noncollecting
sink demonstrates bounded producer buffering and backpressure; caller collection
is an explicit materializing operation, not streaming acceptance.

Evidence: `docs/evidence/044.md` in the implementation repository, linked
from this task queue when completed; include both adapter and core revisions.
