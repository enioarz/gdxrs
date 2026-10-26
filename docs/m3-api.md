# M3 sorted writer API inventory

This records the Rust M3 surface, not complete native C API parity. See the
[full modern API inventory](api-parity.csv) for later milestone ownership and
[task 023 evidence](evidence/023.md) for release qualification and measured limits.

| Capability | Rust surface | Qualification / limits |
| --- | --- | --- |
| Exclusive new-file transaction | `Writer::create`, `finish`, `abort`, status/statistics/temp path | Existing parent, no-clobber hard-link publication, explicit errors; path-based seekable files only |
| Shared reader/writer root | `Reader::create_writer`, `write_options` | Same library cap; source ownership independent; task 023 measures a streaming pipeline |
| Bounded output | `WriteOptions::{memory_budget_bytes,maximum_output_bytes}` | Checked logical extent and explicit finalization; accounting is separate from RSS |
| Raw/stored/zlib encoding | `WriteCompression::{None,Zlib}` | Independent <=32 KiB blocks, level-1 miniz with stored fallback; optional compression dependency |
| Metadata scratch quotas | `begin_metadata`, `MetadataOptions`, `metadata_statistics` | Private checksummed journal, aggregate logical-byte/handle caps and retryable cleanup |
| Incremental raw registration | `register_uel`, `register_symbol` | Original one-based writer-owned tokens; exact label bytes, folded symbol names; incremental disk lookup may be quadratic |
| Fresh bulk dictionary | `register_uels_fresh` | Fallible streamed labels, external sort/dedup, first-registration IDs through a fallible callback; caller-retained tokens need their own bounded storage |
| Complete supported metadata | `WriteSymbol`, `add_comment`, `add_set_text`, `add_acronym`, `set_domains`, `freeze_metadata` | Descriptions, comments, set text, acronyms, aliases, strict/relaxed domains and original IDs; fixed producer/audit bytes |
| Alias identity | `SymbolReference::{Registered,Name,Universe}` | Forward references and bounded chain validation; retain original referenced alias IDs in definitions/domains |
| Sorted raw records | `begin_symbol`, borrowing `SymbolWriter::{write,finish}`, `KeyBounds` | Dimensions 0–20, all symbol kinds except own alias records; one/five typed fields; supplied inclusive or conservative bounds; no prescan |
| Strict domain membership | `WriteDomain::Strict` | Complete target set before dependent symbols; fixed disk index and binary search; universe aliases allowed |
| Values/defaults | `Value`, `SpecialValue`, `AcronymId` | Preserve file bits and typed special/acronym identity; checked set-text/acronym references; native empty scalar defaults |
| Failure handling | Terminal `Result` errors, `WriteStatus`, publication flag, abort retry | Bad/duplicate/out-of-order rows and abandoned symbols prevent publication; post-publication cleanup failure retains the valid destination |

Remaining M4 work includes general unsorted/string/mapped record adapters,
external record sorting and error streams, UEL operation parity, append/rewrite and
remaining modern semantic operations. Larger dictionary token recovery/mapping and
incremental registration throughput are separate from the fresh bulk API.
C ABI adapters, companion tools and full native function parity remain M8.
Indexes, parallel codecs and Polars integration remain separate milestones.

`std::fs::File` drop cannot report close failures. Flush/sync failures are checked;
directory entries are not synced, and crash recovery/directory durability are not
promised. The caller must provide a trusted, stable parent/private namespace.
Quota accounting includes core-owned capacities; fixed caller example state and
runtime headroom are covered by the measured process cap, rather than relabeled
as tracked library memory. Native platform functionality does not imply portable
RSS/throughput qualification; task 023's OS measurements are Linux-only.
