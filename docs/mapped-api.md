# M4 mapped API disposition after task 026

[Task 026 evidence](evidence/026.md) records source-bound validation.
This is the current implementation inventory for the mapped rows in the original
[API plan](api-parity.csv). It does not claim full C-call/state compatibility.

| Native surface | Rust equivalent | Disposition |
| --- | --- | --- |
| gdxUELRegisterMapStart / Map / Done on writer | Writer::register_mapped_uels | Bounded one-shot fallible registration with first-encounter raw IDs; immutable sparse bijection; exact labels; conflicts terminal |
| gdxUELRegisterMapStart / Map / Done on reader | Reader::mapped_uels then MappedUels::register | Explicit source-bound raw-to-caller pairs; retained cleanup owner; no hidden UEL auto-expansion |
| gdxDataWriteMapStart / Map / Done | Writer::begin_mapped_symbol, MappedSymbolWriter::write / finish | Borrowing typed writer; externally sort translated rows into original raw order; no append or data-error recovery |
| gdxDataReadMapStart / Map / Done | Reader::scan_mapped, MappedCursor::next / close | External prescan/sort, mapped order, typed values, original raw keys, explicit missing policy and partial-error counters |
| gdxGetUEL | MappedUels::raw, then existing UEL cursor/lookup facilities | Caller-to-original-ID lookup implemented; direct caller-ID-to-label convenience and further UEL mutation remain task 029 |
| gdxUMUelGet / UMFindUEL map-number projection | MappedUels::mapped; writer mapped_id_for_raw / raw_id_for_mapped | Explicit translation and missing Option on reader; raw UEL/catalog APIs unchanged; label matching retains core policy |

Mapped reads return one fixed row at a time, with a bounded raw prescan batch;
projection/filter/batch conveniences are not added. Duplicate native caller IDs,
negative-ID memory corruption, native case folding and implicit recovery are not
emulated. See [decision 0020](decisions/0020-mapped-ids.md) for ownership, missing
semantics, quota/handle/occurrence limits, native probes and performance scope.
Error streams, broader policies/UEL operations, append and M4 acceptance remain
tasks 027–031. No changes to raw API identity or to the original API plan's
historical dispositions are implied.


Task 027 adds `Reader::scan_mapped_reporting` with a fallible borrowed callback
and explicit record/byte quotas. It reports every missing-map row and all missing
axes before the selected Error/SkipRecord policy. Original raw IDs, typed values,
input ordinals and mappings that do exist are preserved. Partial acknowledged
counts remain available through `MappedUels::data_error_statistics()` after a
failed prescan. See [domain/error policy](decisions/0021-domain-error-streams.md).


Task 029 adds direct `uel_information`, `find_uel`, `mapped_uel` and `information`
queries using the existing sparse index, plus source-checked `uel_in_session` for
renamed labels. `visit_domain` explicitly scans/sorts one axis and calls back in
mapped order with original IDs; missing observed mappings fail and callbacks are
fallible. See [UEL/metadata disposition](uel-metadata-api.md) and
[phase/ownership rules](decisions/0023-uel-metadata-operations.md).
