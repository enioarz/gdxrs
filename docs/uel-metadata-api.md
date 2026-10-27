# Modern UEL and metadata API disposition (task 029)

Pinned reference: GAMS-dev/gdx `9e2a8c8932412010cd7d2f94727d1740143bf3da`,
`src/gxfile.cpp` and `src/gdxapi.yaml`. The separate `tools/metadata_ops` helper
qualifies complete author/read and session expectations. This closes task 029's
scoped inventory; it is not full native C ABI/call-state compatibility.

| Native API | Implemented typed equivalent / unsupported disposition |
| --- | --- |
| UELRegisterRawStart / Raw / Done | Existing Writer::register_uel and bounded fresh bulk registration; IDs follow first registration. Native call-state wrapper unsupported. |
| UELRegisterStrStart / Str / Done | Existing string phase and registered-string deferred phase. Reader source-catalog auto-expansion is unsupported; source counts/IDs stay immutable. |
| UELRegisterMapStart / Map / Done | Existing bounded writer/reader immutable sparse bijections; zero supported. Incremental remapping, ambiguity/negative IDs and implicit expansion unsupported. |
| RenameUEL (gxfile.cpp 2731) | Writer::rename_uel and read MetadataSession::rename_uel. ID preserving, exact collision checks and bounded disk updates; read edits do not persist. Exact bytes replace native folding/right trimming deliberately. |
| UMFindUEL / UMUelGet (2699/3637) | Reader/Writer/view find_uel / uel_information, plus MappedUels::find_uel / uel_information. Missing label/map is Option; invalid raw IDs error. |
| GetUEL | MappedUels::mapped_uel resolves caller-to-original-ID plus source label; uel_in_session joins a source-checked explicit view. No missing-ID sentinel strings or label substitution. |
| UMUelInfo (2713) | UelMappingInformation gives source/writer count, mapped count and optional highest caller ID. No dense high-ID allocation. Native highest=0 ambiguity is replaced by Option. |
| UELMaxLength / SymbMaxLength / SymbIndxMaxLength (3552/3630) | Explicit streaming maximum helpers; symbol-index maximum explicitly scans records with fixed axis state. View variants use overlay labels. No hidden work on ordinary scans. |
| AddSetText / GetElemText (2182/2307) | Existing exact-byte, deduplicated pre-freeze registration and new fixed-size text information. No late/read-session catalog expansion or native quote normalization. |
| SetTextNodeNr (3169) | Writer/read-view signed node state for existing text IDs, including 0. Nonzero assignment cannot be overwritten. Nodes are ephemeral and reopen as zero, matching actual native serialization. |
| SetHasText (3523) | Query via Writer::has_set_text / SymbolMetadata::has_set_text. Accepted nonzero set-text levels mark and serialize the flag. It is not a native setter. |
| SymbolAddComment / GetComment (3595) | Writer comments now allowed between finished symbols; existing streamed reader comments. Exact <=255 bytes; native silent truncation not emulated. |
| SymbolSetDomain / GetDomain | Existing strict pre-freeze registration and streamed original referenced IDs, including aliases. Post-freeze strict mutation unsupported. |
| SymbolSetDomainX / GetDomainX (2600) | New set_relaxed_domains permits repeated late display updates, independently preserving strict IDs/checks. Reader already exposes both. Bad names/arity fail; native bad-name-to-star coercion unsupported. |
| AddAlias | Existing registration retains direct target IDs, validates chains and supports universe/aliases. Retargeting/replacing aliases unsupported. |
| FindSymbol / SymbolInfo / InfoX / SymbolDim / SystemInfo | Existing folded symbol lookup, typed metadata, counts and handles. No mutable global current-symbol state. |
| CurrentDim | Explicit symbol metadata / cursor shape; global current-call-state emulation unsupported. |
| GetDomainElements (3182) | New MappedUels::visit_domain uses explicit bounded prescan/sort and fallible mapped-order callbacks with original IDs. Complete mappings required. Existing unique_domains supplies selected/raw extraction. Implicit expansion, native filter error-list/callback call-state emulation unsupported; use explicit typed selection and error policies. |
| AcronymGet/SetInfo and other acronym/value operations | Task 028, including pre-freeze bounded metadata replacement, explicit projection and collision/overflow rules; see value/acronym inventory. |
| StoreDomainSets / AllowBogusDomains | Safe disk membership always retained for strict checks. Disabling validation or allowing unchecked domains is deliberately unsupported. |
| Symbol names/descriptions/type/dimension/user fields | Declared at registration; there is no generic symbol-replacement API. Identity/schema mutation remains unsupported. |
| File mutation / append / external conversion / tracing / legacy error state | Append is task 030; hidden conversion and legacy C state are outside 029. No in-place modification of an opened source. |

Native source writes only set-text strings at gxfile.cpp 577–584; Node numbers are
user/session pointers and are not persisted. Native node assignment checks current
zero, so a nonzero value cannot be cleared/reassigned. New setters return bool for
that normal refusal and errors for invalid IDs. Native SetDomainX stores a display
array independently from strict symbols, and GetDomainX gives display precedence;
GetDomain still returns strict IDs. Those simultaneous references are qualified.

Native probes also demonstrate UEL folding and source-current symbol folding.
Rust preserves the established exact-label / folded-symbol policies. Native bad
lookup sentinel strings, right trimming, comment truncation, implicit expansion
and unsafe/ambiguous maps are not counted as parity. No existing golden, fixture,
isolation bound or frozen source fingerprint is weakened to cover these differences.

See [decision 0023](decisions/0023-uel-metadata-operations.md) for phases, journals,
rollback, callback ownership, quotas and performance limits; executable recipes
are `examples/create_metadata_ops.rs` and `examples/metadata_context.rs`.
