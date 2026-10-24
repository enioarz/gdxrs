# Development oracle

This tooling is an independent C++ reference, not a Rust implementation. It is never linked or launched by Cargo. Initial build support is Linux x86_64 with GCC, Make and CMake; other hosts are not yet validated.

Source pins and build options are in `tools/oracle/sources.json`. GDX is pinned to `9e2a8c8932412010cd7d2f94727d1740143bf3da`, and zlib to its recorded upstream submodule revision `da607da739fa6047df13e66a2af6b8bec7c2a498`. The API-generator submodule is recorded but unnecessary: checked-in generated headers suffice. The builder checks exact commits and refuses dirty/different existing sources rather than resetting them. Native sources live under ignored `.work/oracle/gdx`.

## Build and run

Install CMake 3.16+ and GCC with C++17 support. This instance uses GCC 14.2.0 and locally installed CMake 3.31.10 from the official PyPI wheel:

```sh
python3 -m pip install --target .work/python cmake==3.31.10
python3 tools/dev.py oracle-build
python3 tools/dev.py oracle-self-test
```

The runner prefers a `cmake` on PATH, otherwise the local wheel's native executable. It disables system-zlib discovery explicitly, builds `gdx-static` only in Release mode with two workers, and statically links the driver to it and the pinned bundled zlib. Tests, examples, companion tools and auto-conversion are disabled. No GAMS installation is needed. Building again reuses matching outputs; driver/pin/compiler/CMake identities select separate build directories. A source change invalidates the current driver even before rebuilding. Build directories can be large and are not part of the production memory contract.

`.work/oracle/current.json` records the executable and its digest. Invoke that path directly:

```text
gdxrs-oracle smoke FILE {0|1}
gdxrs-oracle recipe FILE RECIPE {0|1}
gdxrs-oracle dump FILE [OUTPUT]
```

`smoke` creates an upstream-written fixture in uncompressed/block-compressed mode. `dump` streams its public raw API representation to stdout or OUTPUT. OUTPUT is a caller-chosen file and is truncated; do not specify the input file as output. Driver failures return 1, successes return 0. `oracle-self-test` additionally checks missing/truncated files and invalid commands. Production use of the driver is unsupported; upstream eagerly allocates metadata and does not provide this project's future hostile-input or memory guarantees.

Task 003 adds `recipe` for deterministic fixture generation and `fixtures-check`/`fixtures-regenerate-check` for corpus verification; see [tests/fixtures/README.md](../tests/fixtures/README.md). These replace neither production Rust tests nor the independent M0 review.

## Semantic dump version 1

UTF-8 ASCII text, LF lines, tab-separated fields, no quoting. An empty byte string is an empty field; preserve trailing fields. All strings returned by the C API are encoded as lowercase hexadecimal bytes up to its NUL terminator. Numeric IDs/counts are decimal. Their validity/ranges originate in the public API, not a claim that the file stores all of them in the same width. A raw key tuple is comma-separated positive one-based UEL IDs (empty for scalars).

| Tag | Fields after tag |
| --- | --- |
| `GDXRS-ORACLE` | `1` (schema version) |
| `F` | file format version, compression level, symbol count, UEL count, producer hex, file-version/system-identification hex |
| `U` | raw UEL ID, label hex, in file order |
| `T` | set-text ID (including 0), node number, text hex |
| `A` | acronym ordinal, acronym ID, name hex, explanation hex |
| `S` | symbol ID, name hex, dimension, type, public record count, user/subtype information, explanation hex |
| `D` | symbol ID, domain mode (1 implicit, 2 relaxed, 3 strict), then one `strict-symbol-ID:domain-label-hex` field per dimension |
| `C` | symbol ID, comment ordinal, comment hex |
| `R` | symbol ID, key tuple, comma-separated typed values |
| `E` | symbol ID, actual emitted record count |

`S`/`R` also include symbol 0, the API's synthetic universe, followed by ordinary symbols in file order. Aliases expose the API's resolved record stream, not separately stored alias data. Values have these forms:

- `f:hhhhhhhhhhhhhhhh`: ordinary API-returned IEEE-754 binary64 bits as a 16-digit hexadecimal integer, independent of host byte order.
- `s:undef`, `s:na`, `s:pinf`, `s:minf`, `s:eps`: GDX special identities identified by the API.
- `a:N`: acronym identity returned by the API.

Sets/parameters/universe/aliases expose one value; variables/equations expose all five in level/marginal/lower/upper/scale order. The smoke corpus has three symbols, six stored records and three universe records in each mode. Expected metadata/records are authored from generator inputs in `smoke_expectation`, not copied from driver output.

The file-version field includes creation-time/system information and is not a deterministic byte-for-byte header oracle. The self-test checks format/compression/producer/counts and every other emitted semantic line, while documenting this variable field. The raw API can synthesize scalar defaults and normalize signed zero, specials, names or explanatory text. This dump preserves API-observable semantics and finite bits returned by the API; it does not expose inaccessible original storage bits or embedded-NUL strings. Future format fixtures must characterize these distinctions rather than infer layout from the dump.

Set-text/comment enumeration terminates on the API's documented false result; no native function gives a complete public set-text table count. Metadata enumeration and record reads are streaming in the driver, but `oracle-self-test` captures a deliberately bounded small dump for exact comparison. Larger benchmarks must not capture whole dumps in memory.

## Provenance

See `docs/licenses/GAMS-dev-gdx.txt` and `docs/licenses/zlib.txt` for the full notices from the pinned sources. The driver calls their public APIs; it does not change oracle implementation behavior. No upstream source/binary is vendored into the production crate. Preserve these notices when adapting fixtures or code in later tasks.
