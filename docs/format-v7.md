# GDX v7 format and semantic specification

This is the M0 implementation specification for the modern, uniform little-/big-endian v7 profile. It is grounded in [GAMS-dev/gdx at `9e2a8c8`](https://github.com/GAMS-dev/gdx/tree/9e2a8c8932412010cd7d2f94727d1740143bf3da) and the independently expected [fixture corpus](../tests/fixtures/README.md). Rule IDs below bind source anchors and executable byte/semantic observations in [format-evidence.json](format-evidence.json); `python3 tools/dev.py spec-check` checks that binding. This is not a claim that a Rust decoder exists.

**Normative language:** “must” describes the intended Rust implementation. “Upstream” describes the observed reference, including behavior that must not be copied for corrupt inputs. Scalars' public defaults, aliases and string normalization are API projections, not additional physical records.

## F01 — Primitives and byte order

At physical offset 0 are three size/pattern pairs, in this order:

| Field | Size | Required pattern |
| --- | --- | --- |
| word size then word pattern | u8, 2 bytes | size 2, integer `0x1234` |
| integer size then integer pattern | u8, 4 bytes | size 4, integer `0x12345678` |
| double size then double pattern | u8, 8 bytes | size 8, binary64 nearest to π, bits `0x400921fb54442d18` |

The bootstrap occupies 17 bytes. These are byte-size indicators, not padding. Little-endian bytes are `02 34 12 04 78 56 34 12 08 18 2d 44 54 fb 21 09 40`; big-endian reverses each multi-byte pattern independently. Select word, integer and double order once from the markers. Signed integer fields are two's complement i32/i64; words are u16; doubles are IEEE binary64. i64 offsets use the integer order for the entire eight-byte field. Single-byte tags and string bytes never reverse.

The initial supported profile requires uniform primitive order (all little or all big), verified by `key-widths-be` and `semantics-0-be` read by the unchanged oracle. The upstream stream class also models mixed primitive orders, but no mixed-order target is supported initially: return `UnsupportedByteOrder` explicitly rather than guess. Size/order patterns that do not match are errors. Native execution on a big-endian machine is not claimed.

Source: [gmsstrm.cpp `TMiBufferedStream` construction and ordered reads, L780–854](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gdlib/gmsstrm.cpp#L780).

## F02 — Short strings and header

`str` below means `u8 length` immediately followed by that many bytes, without a terminating NUL or padding. Its encoded maximum is 255 bytes. Treat bytes as bytes; UTF-8 is not a file requirement. An empty string has one zero length byte. C-API names/labels have stricter limits below.

Immediately after the 17-byte bootstrap:

```text
u8 magic = 123
str identifier = "GAMSGDX"                 # 7 bytes, case-preserving
i32 file_format_version = 7
i32 compression = 0 | 1
str audit/system_identification
str producer
major_index                              # F03
```

The version field begins at physical offset 26 for this header structure. The audit/system string contains creation/system information; it is neither a trusted version selector nor required to have a fixed length. Calculate later positions from lengths. Require v7 and valid markers; the Rust profile rejects v5/v6 and unknown future versions before reading a modern layout. Upstream compares the identifier case-insensitively; the writer emits exactly `GAMSGDX`. A reader can accept its ASCII case variants without changing its length/framing. No trailing `.gdx` or filename convention changes the format.

Source: [gxfile.cpp `gdxOpenWriteEx`, L430–453](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L430), [`gdxOpenReadXX`, L1990](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1990).

## F03 — Major index and physical sections

The upstream writer reserves **80 bytes**, initially ten zero i64 slots. At finalization it overwrites the first 52 bytes with `i32 19510624`, followed by these six i64 **absolute physical byte positions**:

| Ordinal | Meaning |
| --- | --- |
| 0 | symbol-table start (`_SYMB_`) |
| 1 | UEL-table start (`_UEL_`) |
| 2 | set-text-table start (`_SETT_`) |
| 3 | acronym-table start (`_ACRO_`) |
| 4 | next-write position, end of symbol data / append insertion point |
| 5 | relaxed-domain-table start (`_DOMS_`), or zero when absent |

The remaining 28 reserved bytes are not extra supported section pointers; the writer leaves them zero. Symbol data begins at or after `major_index_start + 80`. In upstream-generated files, data sections precede `_SYMB_`, then `_SETT_`, `_UEL_`, `_ACRO_`, `_DOMS_` physically. The pointer ordinals are therefore **not** physical sort order. Ordinals 0 and 4 coincide in the fixtures; the append insertion point is not EOF.

Validate each required offset against file size with checked arithmetic before seeking. Zero is allowed for the optional domain section and alias data positions, not ordinary required metadata starts. Use validated physical section boundaries; do not allocate according to an offset difference without a memory reservation. Reject overlapping/inconsistent required sections rather than interpreting file contents as an offset table. Unused/reserved bytes are not a format extension supported by this implementation.

Source: [gxfile.cpp `gdxClose`, L540–627](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L540).

## F04 — Compression blocks

The header/major index are uncompressed. Metadata sections use the file compression flag. Individual data sections use the symbol's compressed flag, even if the file's metadata is compressed. Scalars written upstream are uncompressed. Each compressed section begins at a block header, independently of other sections.

```text
u8 kind                                  # 0 stored, 1 zlib
u8 payload_length_high
u8 payload_length_low
u8 payload[payload_length_high*256 + payload_length_low]
```

The two length bytes are big-endian regardless of primitive byte order. Length excludes the three-byte header. A stored payload is decoded bytes; a zlib payload is one complete zlib-wrapped DEFLATE stream including its integrity check. Decoded blocks contain 1–32768 bytes. Reject zero/oversized/truncated blocks, unknown kinds, invalid zlib framing/checksums, excess decoded bytes or unexpected trailing stream data. Decode with an output limit; never trust the compressor to enforce it.

The writer flushes at section/compression changes and emits stored blocks when zlib would not shorten the decoded data. Decoded block boundaries do not align with records or primitive fields. A record, string or integer can cross blocks. The `blocks` fixture proves one crossing record; it also contains both stored and zlib blocks. Section pointers refer to physical headers, never virtual decoded offsets.

Upstream's `FillBuffer` ignores `uncompress`'s return status. The checksum-damaged fixture is consequently accepted there. **Rust must still reject it.** This integrity behavior is an intentional hardening difference, not an interoperability failure on valid files.

Source: [gmsstrm.hpp `TCompressHeader`, L232](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gdlib/gmsstrm.hpp#L232), [gmsstrm.cpp `FillBuffer`, L582 and `FlushBuffer`, L667](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gdlib/gmsstrm.cpp#L582).

## F05 — Symbol table

In the section's decoded stream: `str "_SYMB_"`, `i32 number_of_symbols`, that many entries, then `str "_SYMB_"`. Symbol IDs are entry ordinal + 1; ID 0 is the synthetic universe, not an entry. An entry is exactly:

```text
str name
i64 data_position
i32 dimension
u8 type                                  # 0 set, 1 parameter, 2 variable, 3 equation, 4 alias
i32 user_information                     # subtype or alias target
i32 stored_record_count
i32 error_count
u8 has_set_text
str explanation
u8 data_is_compressed
u8 has_strict_domains
if has_strict_domains: i32 domain_symbol[dimension]
i32 comment_count
str comments[comment_count]
```

Validate dimensions 0–20, known types, Boolean flags 0/1, nonnegative counts and valid domain references. An alias stores target ID in user information, data position/count zero, and no record stream of its own. Descriptions/comments are up to 255 encoded bytes. An error count is metadata about rejected writer operations; rejected records are not serialized as successful records. In `operations`, symbol `errors` retains count 2 while only its accepted row is in the data stream.

Symbol and comment iteration can be sequential/paged. Names are variable-length: there is no fixed-size on-disk directory record and no O(1) ordinal seek without an auxiliary index. Do not eagerly collect a huge catalog.

Source: [gxfile.cpp writer L546–571 / reader L2040–2088](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2040).

## F06 — UEL table

Decoded sequence: `str "_UEL_"`, `i32 count`, `str label[count]`, `str "_UEL_"`. IDs are one-based file order; they are neither lexical rank nor user-mapped IDs. Labels are stored verbatim; C-API registration limits them to 63 bytes and strips certain trailing blanks. Stored non-UTF-8 bytes and distinct `a`/`A` entries are verified. Keep exact stored bytes and IDs even when registration/lookup adapters use different matching rules (F17).

There is no on-disk per-label offset array or reverse hash map. Page/checkpoint a variable-length table, or scan it under the configured budget. Compressed table pages require physical block position plus decoded offset/parser position. An in-memory offset vector proportional to all labels is not required by the format.

Source: [gxfile.cpp `ReadUELTable`, L1854](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1854), [strhash.hpp `SaveToStream`, L414](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gdlib/strhash.hpp#L414).

## F07 — Set text

Decoded sequence: `str "_SETT_"`, `i32 count`, `str text[count]`, `str "_SETT_"`. Text IDs are zero-based; normal upstream output includes empty entry 0. A set record's numeric level field can refer to one of these IDs. Node numbers exposed by the API are transient associations, **not additional serialized integers** in this table. Upstream may deduplicate repeated text and remap references when loading malformed/older producer output; the raw table and API projection must not be conflated.

Source: [gxfile.cpp `ReadSetTextList`, L1901](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1901), [`gdxSetTextNodeNr`, L3176](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L3176).

## F08 — Acronyms

Decoded sequence: `str "_ACRO_"`, `i32 count`, entries `(str name, str text, i32 acronym_ID)`, `str "_ACRO_"`. Physical records encode acronym numeric values using the ordinary-double tag, not a separate acronym tag. Upstream's default numeric representation is `acronym_ID * 10.0E300`; reading may apply configured acronym remapping. Preserve typed acronym identity rather than interpreting every large value as an ordinary finite result. The fixture tests acronym ID 7 and its metadata. Empty names can be generated upstream as `UnknownACRO<ID>` at save time.

Source: [gxfile.cpp `TAcronymList::SaveToStream`, L3997 / `TAcronym::SaveToStream`, L4173](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L3997).

## F09 — Strict and relaxed domains

Strict domain IDs are optional arrays in F05. Zero means universe/unconstrained; positive IDs reference set/alias symbols. Follow alias chains with cycle/range checks for validation; do not recurse without bounds.

If major-index domain pointer is nonzero, its decoded sequence is:

```text
str "_DOMS_"
i32 string_count
str domain_names[string_count]            # one-based references; zero means "*"
str "_DOMS_"
repeat:
    i32 symbol_ID
    if symbol_ID <= 0: stop               # upstream writes -1
    i32 name_ID[dimension_of_symbol]
str "_DOMS_"
```

Validate IDs/counts before indexing or skipping. A zero domain pointer means no relaxed-domain section; `optional-domains-absent` verifies that case. Strict and relaxed metadata are distinct: the public `DomainX` view prioritizes relaxed strings (mode 2), otherwise strict references (mode 3), otherwise `*` per dimension (mode 1). Relaxed strings need not refer to an existing set; strict domain membership validation is a separate operation requiring a bounded set lookup.

Source: [gxfile.cpp domain save L601–618 / read L2115–2134](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2115), [`gdxSymbolGetDomainX`, L2433](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2433).

## F10 — Data preamble and counts

At a non-alias symbol's validated data position, interpret its per-symbol compression flag and read:

```text
str "_DATA_"
u8 dimension                             # must agree with F05
i32 header_record_count
repeat dimension times: i32 minimum, i32 maximum
record_stream
u8 255                                   # terminal marker, not a record
```

Raw writes use header count `-1` because final size is not known. String/mapped writes can emit the pre-sort/input count; duplicates can make it differ from accepted records. The final symbol-table stored count is authoritative for physical successful records. Header count `-1` is a sentinel, not an allocation request; values below -1 are invalid. Empty scalar data has zero physical rows and only the terminator; its API view is specified in F14. The dimension/bounds prefix must still be read even if a caller requests zero projected value columns.

Source: [gxfile.cpp `InitDoWrite`, L1170](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1170), [`PrepareSymbolRead`, L968–999](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L968).

## F11 — Key widths and bounds

For each dimension, compute `N = maximum - minimum + 1` in checked widened arithmetic. Width is 1 byte if `1 <= N <= 255`, 2 bytes if `256 <= N <= 65535`, otherwise 4 bytes (including the empty-symbol upstream bounds where N is nonpositive). **The thresholds are 255 and 65535, not 256 and 65536.** Four-byte explicit keys are signed i32 relative offsets; 1/2-byte fields are unsigned. Add minimum with checked arithmetic and require the resulting key ID to be positive, within declared bounds for actual records and within the file's UEL count.

Default raw writing uses minimum 0 and maximum `INT32_MAX`, conservatively selecting width 4. Bounds supplied by callers may be conservative; they need not equal observed extrema. Empty symbols can have sentinel extrema (`INT32_MAX`, 0); do not reject those solely because no positive range exists. A nonempty record must still decode to valid IDs.

Source: [gxfile.cpp `GetIntegerSize`, L351](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L351), [`gdxDataWriteRawStart`, L2278](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2278).

## F12 — Key state and delta decoding

Initialize previous keys to `-256` per dimension for each new symbol. In v7 the key-tag threshold is the **current dimension**, with no v5/v6 dimension branch:

```text
tag = read_u8()
if tag == 255: finish and verify physical stored count
else if dimension == 0:
    require tag == 1                      # scalar writer's dummy record tag
else if 1 <= tag <= dimension:
    # tag is ONE-BASED first changed dimension
    for d in (tag - 1)..dimension:
        previous_keys[d] = read_relative(width[d]) + minimum[d]
else if dimension < tag < 255:
    previous_keys[dimension-1] += tag - dimension
else: reject InvalidKeyTag
decode value fields (F13)
validate key tuple, emit one record
```

The scalar restriction is a hardening rule for the supported writer profile; the upstream implementation is less strict on some malformed tags. General suffix changes preserve all earlier keys. Compact increments preserve all but the last key. Keys must not remain in their uninitialized state, overflow, or reference missing UELs. Serialized successful raw records are lexicographically increasing by integer IDs; sorted string/mapped input is normalized into that order.

Annotated bytes in `key-widths`'s `prefix` symbol (relative decoded offsets):

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 28 | `01 00000000 00000000 0a 0000000000000ac0` | change from dimension 1; two four-byte offsets 0 + minima 1 → `(1,1)`; ordinary `-3.25` |
| 46 | `03 0a 0000000000000ac0` | dimension 2 + increment 1 → `(1,2)` |
| 56 | `fe 0a 0000000000000ac0` | dimension 2 + increment 252 → `(1,254)` |
| 66 | `02 fb010000 0a 0000000000000ac0` | explicit suffix from dimension 2; relative 507 + 1 → `(1,508)` |

The ledger checks these exact annotated record bytes. `w255` proves one-byte bounds and a compact tag `fd` for delta 252 in one dimension. Record boundaries cannot be located by dividing a byte span by record count.

Source: [gxfile.cpp `DoWrite`, L1234–1379](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1234), [`DoRead`, L1382–1454](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1382).

## F13 — Value tags and normalization

Sets/parameters have one field; variables/equations have five ordered fields: level, marginal, lower bound, upper bound, scale. Every field begins with one u8 tag:

| Tag | Value | Payload |
| --- | --- | --- |
| 0 | UNDEF | none |
| 1 | NA | none |
| 2 | positive infinity | none |
| 3 | negative infinity | none |
| 4 | EPS | none |
| 5 | +0.0 | none |
| 6 | 1.0 | none |
| 7 | -1.0 | none |
| 8 | 0.5 | none |
| 9 | 2.0 | none |
| 10 | ordinary binary64, including encoded acronym values | 8 bytes in double order |

Other tags are errors. Upstream replaces invalid tags with zero; Rust must reject them. Preserve ordinary payload bits in the file view; expose tagged special identities separately. Upstream's default sentinel doubles are `1e300`, `2e300`, `3e300`, `4e300`, `5e300` for the first five tags. Those defaults are configurable API projections, not alternate on-disk layouts.

Upstream writing compares bit patterns for common constants, then maps either signed zero to tag 5, IEEE NaN to tag 1, and IEEE infinities to tags 2/3. Thus a writer's `-0.0` input is not proof of a negative-zero payload in a file. The fixture's authored expectations include these normalizations. Ordinary `2.5` encodes as `0a 0000000000000440` in little-endian form. Decode/skip every framed field even when it is not projected into the output.

Source: [gxfile.hpp `TgdxIntlValTyp`, L160](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.hpp#L160), [gxfile.cpp value encoder L1326–1367](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1326).

## F14 — Defaults and scalar projection

A zero-physical-row scalar is exposed upstream as one row with empty keys and subtype defaults. The symbol-info API reports count 1 while its serialized stored count is 0. Do not require extra bytes or rewrite the stored count to justify this public row. An empty indexed symbol stays empty. A nonempty scalar contains its explicit row instead of an additional default.

Defaults are `(level=0, marginal=0, lower, upper, scale=1)`. Sets/parameters use value 0. Variable user subtypes 1–9 have `(lower,upper)` respectively `(0,1)`, `(0,+INF)`, `(0,+INF)`, `(-INF,0)`, `(-INF,+INF)`, `(0,+INF)`, `(0,+INF)`, `(1,+INF)`, `(1,+INF)`. Unknown/out-of-range variable subtypes project upstream's unknown default `(0,0)` for bounds. Equation user values 53–59 have bounds `(0,0)`, `(0,+INF)`, `(-INF,0)`, `(-INF,+INF)`, `(0,0)`, `(0,+INF)`, `(0,0)`; out-of-range values use equality defaults. Keep stored user information even when applying such API projection.

Source: [gxfile.cpp `GetDefaultRecord`, L1605](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1605), [gclgms.h default tables, L190–212](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/generated/gclgms.h#L190).

## F15 — Aliases and universe

Type 4 aliases reference a set/alias symbol by user information, or 0 for the universe. Resolve with a visited-ID/depth bound; reject cycles, missing targets or non-set targets. Aliases have no own `_DATA_` section. Their public raw view follows the resolved target; alias metadata can still report zero physical rows. Universe ID 0 synthesizes each file UEL ID in registration order with one zero value. It requires label/table counts, not a stored record stream.

Source: [gxfile.cpp `gdxAddAlias`, L2142](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2142), [`PrepareSymbolRead`, L894–925](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L894).

## F16 — Sorted writes, mapping and diagnostics

Raw writes require increasing lexicographic integer tuples with no duplicates. Upstream rejects duplicate keys (`-100009`), backward order (`-100025`), domain violations (`-100058`) and duplicate symbols (`-100007`). Rejected rows are diagnostic events, not serialized rows. Mapped writes translate user IDs to raw IDs and string writes register/resolve labels; both sort by raw tuples at completion. Therefore mapped-ID order and lexical string order are not file record order. Sorting/diagnostic retention in Rust must spill or enforce budgets rather than reproduce upstream's eager buffers.

The `operations` recipe independently checks rejection error codes and retained error counts, sorted results and append output. Legacy error-count/last-error call-state APIs belong in the compatibility adapter; native Rust operations return contextual errors directly.

Source: [gxfile.cpp raw validation L1248–1292](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L1248), [`gdxDataWriteMap`, L2784](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L2784).

## F17 — Names, label lookup and text rules

Symbol identifiers emitted upstream are at most 63 bytes, begin with a letter and continue with letters/digits/underscore. Symbol lookup/uniqueness is case-insensitive. Labels are C-API strings of at most 63 bytes; registration trims trailing bytes <= ASCII space, rejects controls and simultaneous single/double quote usage, and retains non-UTF-8 high bytes. Valid explanatory text is at most 255 bytes; controls become `?` and mixed quote characters are normalized. An overlong symbol explanation is shortened with a `String overflow:` diagnostic prefix, not simply preserved as its first 255 bytes. Comments truncate at 255. A raw file reader preserves the stored bytes and validates framing without assuming UTF-8 or re-running writer normalization.

The pinned `TUELTable` inherits `TXStrHashList` with `caseInsensitive=true`: its registration/native find behavior folds ASCII case. File loading via `StoreObject` retains separate physical entries even if they differ only in case. The corpus verifies both. **Exact byte filters on stored labels and compatibility registration are separate contracts.** M1 must not silently merge stored IDs; a later compatible string adapter must document its folding policy instead of claiming all upstream label operations are byte-exact.

Source: [gxfile.cpp `GoodUELString`, L95 / `MakeGoodExplText`, L312](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L95), [gxfile.hpp `TUELTable`, L262](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.hpp#L262), [strhash.hpp template, L64](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gdlib/strhash.hpp#L64).

## F18 — Writer finalization and append

Write records and terminal markers, then trailing metadata; only at finalization patch the uncompressed major index with the actual physical positions. Compression and key-width choices cannot be changed by overwriting a variable-length compressed preamble. Unknown exact key bounds require conservative valid widths, caller bounds or an explicit spill/replay pass.

Append v7 opens at major-index ordinal 4, preserving prior symbol/UEL IDs, adds new symbols and rebuilds trailing tables. Existing symbol replacement is an error. The atomic copy/rewrite strategy in the architecture plan implements equivalent valid-file semantics with a stronger publication contract; it need not reproduce upstream's in-place crash behavior. Counts/IDs remain bounded by their signed 32-bit representations even though positions are i64. No silent overflow, saturation or new format dialect is allowed.

Source: [gxfile.cpp `gdxClose`, L540](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L540), [`gdxOpenAppend`, L3504](https://github.com/GAMS-dev/gdx/blob/9e2a8c8932412010cd7d2f94727d1740143bf3da/src/gxfile.cpp#L3504).

## F19 — Malformed-input policy

All primitive reads require enough bytes; all sums/products/conversions must be checked before allocation or seeks. Counts are limited by remaining section bytes and resource reservations as well as signed representation. Validate dimension/type/value/key tags, marker pairs, IDs/references, block lengths and terminal counts. Header count -1 is the documented exception, not a blanket permission for negative counts. Alias cycles and arithmetic overflow are errors, not panics or unbounded loops. Nonempty scalar multiplicity and raw tuple ordering must be validated by the supported profile.

The invalid manifest records intended Rust errors separately from reference outcomes. A reference accepting corrupt bytes is not an instruction to accept them. No successful partial result may hide a later error. Truncated/header/section/block, impossible offsets/counts/dimensions, unsupported versions, invalid tags and damaged zlib checksums all have explicit probes. A version-edited v7 file does not constitute a legacy compatibility test.

## F20 — Complete restart state

A future checkpoint at a record boundary requires: source/symbol identity; format and primitive byte order; physical section/block location and compression mode; offset within its decoded block; dimension/type/value-field count; key bounds and widths; **previous key tuple before the next record**; record ordinal/count bounds; and scalar/universe/alias cursor state where applicable. Special-value/acronym projection configuration belongs to the owning immutable reader/session and must remain equivalent across workers. A block header carries no previous key tuple. A compact last-dimension delta carries no earlier key values.

A checkpoint at an arbitrary byte position is insufficient. Choose boundaries before tags; resuming there needs no partially parsed primitive/value state. A record may span later blocks. Natural first-dimension-explicit records may avoid some prior-key storage only after proving all dimensions are replaced. The `prefix` delta trace and `blocks` crossing demonstrate why both key state and compressed decoded offsets are necessary. Actual checkpoint seek/parallel correctness remains M5 testing; this rule is verified dependency analysis, not a working index claim.

## F21 — Coverage and implementation boundary

Supported initial profile: v7, uniform little/big endian, stored and zlib blocks, all five symbol kinds and dimensions 0–20, optional relaxed domains present/absent, known common/special/acronym values. All are grounded in source plus oracle-verified fixtures. Mixed primitive order, genuine legacy formats and unknown extensions are explicitly unsupported initially. The pre-2002 producer-specific UEL-count heuristic in upstream `ReadUELTable` is also omitted: a modern reader validates the actual table rather than changing its count based on an audit-string year. This is a deliberate very-old-producer compatibility exclusion, not a measured performance gain. There are no unresolved field widths/tag/default questions needed to implement the supported M1 parser profile.

The fixture probe is a bounded development annotation tool, not production code to copy wholesale. Its rejection checks are not a fuzz campaign. Large >4 GiB offsets and budgets are representation requirements whose runtime scale tests remain later work. The API inventory contains **92 unique functions from `functions:`**, plus a separate six-accessor inventory for three read/write properties. Callback declarations in later YAML sections are not duplicate functions. Every inventory row is a plan, not an implemented API. M0 review/release is explicitly confirmed by the user; see decision 0003. M1 implementation evidence is recorded separately in tasks 007–011.
