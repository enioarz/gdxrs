# Oracle fixture corpus

Task 003's corpus contains 10 valid GDX v7 files and 12 deliberately invalid files. The total binary size is about 1 MiB (well below the 16 MiB limit). Expected TSV files enumerate complete public metadata and record streams, including the synthetic universe, alias views and default scalar rows. They are not just row counts or result hashes.

## Provenance and independence

`tools/fixture_cases.py` authors deterministic inputs and expected semantics without reading a GDX file or using a Rust decoder. Generation writes expectations **before** calling the pinned upstream library. `tools/oracle/fixture_generator.hpp` executes the TSV recipes through the public upstream write APIs. Full GDX/zlib source pins, generator file SHA-256 values, the generator's base revision, individual artifact hashes and verified byte observations are in `manifest.json`. A base revision plus the generator content hashes identifies uncommitted generator work honestly; task evidence names the final tested implementation commit.

The semantic dump grammar is documented in [the oracle guide](../../docs/oracle.md). Expectations replace only the volatile creation/system string with `<creation-system>`; version, compression, counts, producer, label bytes, IDs, domains, comments, text, acronyms, types, subtypes and every record field still compare exactly. Regeneration also compares all bytes after masking that one length-delimited audit string on the pinned Linux build. No other normalization is permitted.

The upstream sources are MIT/Zlib licensed; their full notices are retained in [docs/licenses](../../docs/licenses/GAMS-dev-gdx.txt). Generated inputs/expectations were authored for this project. No upstream implementation source is added to the production crate.

## Coverage

| Fixture | Tested properties |
| --- | --- |
| `semantics-0`, `semantics-1` | All symbol kinds; dimensions 0/1/2/20; empty sets and synthesized default scalars; all variable/equation subtypes/defaults; every value tag; aliases to set/universe; strict/relaxed domains; descriptions/comments/set text/acronyms; non-UTF-8 bytes; maximum label/name/explanation/comment lengths; both compression modes |
| `key-widths` | Actual width thresholds at range cardinalities 255/256/65535/65536; repeated key prefixes; changes in first/last dimension; compact delta and explicit suffix paths; 65,536 UELs |
| `blocks` | 5,000 deterministic finite records; zlib and stored blocks; one record crossing a decoded 32 KiB block boundary; multiple records per block |
| `operations` | Mapped and string writes sorted into raw-ID order; duplicate-key and domain-error observations; duplicate-symbol rejection; append preserving old symbols/IDs |
| `case-distinct-labels` | Raw file entries `a` and `A` retained distinctly; only one annotated UEL byte changed from `semantics-0`; upstream reads the full expected raw stream |
| `key-widths-be`, `semantics-0-be` | Opposite-endian fixtures derived by reversing only annotated multi-byte numeric fields, not strings/tags or whole-file byte order; unchanged public semantics independently confirmed upstream |
| `optional-domains-absent` | Empty `_DOMS_` section removed and its header pointer set to zero; remaining file and raw public semantics verified |

`tools/format_probe.py` is an independent development-only annotation tool bounded to 16 MiB of file/decoded data and 100,000 table entries/records. It validates framing, parses metadata and records, records numeric spans and verifies actual widths/tags/block crossings. It is not a production reader, does not establish the future library's memory bound, and currently requires uniform primitive byte order (all little or all big). The C++ reference and checker may allocate whole small metadata tables; larger-than-RAM claims are outside this task.

The registration experiment exposed a source-supported distinction: the pinned `TUELTable` inherits the **case-insensitive** `TXStrHashList`, so registering `A` after `a` does not create another ID. The input recipes test that normalization. `StoreObject` during file loading preserves case-distinct entries, demonstrated by the narrowly transformed fixture. Core raw-ID reads/exact byte filters must preserve stored bytes; a future upstream-compatible registration/lookup adapter must document its case folding explicitly. Do not assume all upstream string APIs match exact-byte file lookup.

Similarly, upstream writing normalizes negative zero to positive zero, IEEE NaNs to GDX NA, and IEEE infinities to GDX infinity tags. The expected file view records those facts while preserving other finite binary64 bits. Empty scalar physical record counts and public synthesized default counts differ deliberately.

## Commands

```sh
python3 tools/dev.py fixtures-check
python3 tools/dev.py fixtures-regenerate-check
python3 -W error::ResourceWarning -m unittest discover -s tools/tests -v
```

Generation is explicit and refuses to overwrite an existing directory:

```sh
python3 tools/dev.py fixtures-generate --output .work/new-fixture-candidate
```

Use the manifest, semantic comparison and byte observations to review candidates before deliberately replacing tracked artifacts. Changing a checksum is never a repair for an unexplained mismatch. `fixtures-check` rejects stale generator identities, empty/missing/duplicate fixture IDs, missing files, path escapes, tampered expectations/artifacts, insufficient coverage and changed malformed-input observations. `fixtures-regenerate-check` uses a separate temporary directory and compares the complete regenerated semantics and bytes/properties. Existing user files are not overwritten.

The native recipe CLI is `gdxrs-oracle recipe OUTPUT RECIPE {0|1}`. Inputs are trusted development recipes, not an exposed product interface. Directives cover raw labels/ranges, maps, text, acronyms, symbol starts, bounds, strict/relaxed domains, comments, raw/string/mapped rows, symbol ends, aliases and append. Prefixing a supported operation with `!` requires its failure and records the upstream error/data-error count; these observations are compared against independently authored values in `operations`.

## Malformed inputs

Invalid fixtures retain their exact source mutation offset/bytes or truncation recipe and an **intended future Rust error**, separately from the observed C++ outcome. Version-number edits are rejection probes, **not genuine legacy-format files**. No v5/v6 reader is being validated.

The pinned upstream accepts the checksum-damaged compressed file because its block loader ignores zlib's return status after decompression. The independent probe rejects it. The future Rust codec **must reject** it with `InvalidCompression`; reproducing that upstream permissiveness would violate the integrity contract. The other 11 malformed invocations return failure on this build. Subprocess observation applies CPU, output-size and core-dump limits and a timeout. Linux additionally applies a 256 MiB address-space limit. macOS uses a 256 MiB data/heap limit because Darwin does not reliably support RLIMIT_AS; this does not bound every virtual mapping or RSS. Inherited lower soft/hard limits are preserved. A dedicated child launcher applies limits before exec of the native oracle; setup/exec failures are reported separately from malformed-input rejection with a named diagnostic. macOS execution still requires platform validation. Crashes/timeouts/acceptance remain distinct observations, never passes by implication. Platforms without this oracle-isolation support remain unvalidated.

Large >4 GiB offsets, huge metadata and memory-capped streaming experiments remain runtime-generated checks for later milestones. This small corpus verifies format/semantic boundaries; it does not prove out-of-core behavior or throughput.

The launcher change updates only generator provenance in the manifest; fixture artifacts and expected hashes are unchanged.
