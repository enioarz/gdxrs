# 0004 — Bounded pure-Rust zlib decoding

Use exactly `miniz_oxide 0.9.1` with default features disabled, optional behind the default-enabled `compression` feature. It adds one transitive dependency, `adler2 2.0.1`; Cargo.lock retains registry checksums verified by Cargo. There is no with-alloc, SIMD, Serde, native library or build script on this path. Licenses: miniz_oxide MIT OR Zlib OR Apache-2.0; adler2 0BSD OR MIT OR Apache-2.0. Both are development-audited pure-Rust crates; production core still forbids unsafe code.

The low-level fixed-slice inflater is used, not `decompress_to_vec`. The state is reset for every independent zlib block. PARSE_ZLIB_HEADER and USING_NON_WRAPPING_OUTPUT_BUF are enabled; checksum-ignore flags are never enabled. Success requires Done, full encoded input consumption, and 1–32768 decoded bytes. A caller-owned 32768-byte output buffer bounds growth; encoded input is capped at the u16 maximum 65535 bytes. Stored blocks over 32768 bytes fail too. Short fields may cross decoded blocks.

On the pinned Linux x86_64/Rust 1.99.1 build, `size_of::<DecompressorOxide>()` is **10,504 bytes**, below the 128 KiB ceiling. That state is reserved before construction; fixed stream storage is additionally conservatively reserved. No codec-side heap allocation feature is enabled. The fixed safe control-owner OOM limitation in decision 0003 still applies. No-default-features has no third-party runtime dependency and rejects compressed streams explicitly.

Verification includes a complete independently hashed 50,025-byte decoded symbol, an eight-byte read spanning the 32 KiB boundary, checksum rejection, and a valid independent zlib stream expanding to 32,769 bytes rejected without buffer growth. The latter payload was independently checked with Python zlib before the final Rust test; an initial hand-copied test payload had a checksum error and was corrected from its declared zero-byte input, rather than weakening the output-size assertion.


Task 019 extends the same pinned dependency with its `with-alloc` feature for the
encoder only; [decision 0013](0013-bounded-writer-codec.md) records the separately
charged fixed heap state and standard allocator-OOM limitation. The fixed-slice
inflater API/validation above remains unchanged; the original feature description
is the historical task-008 configuration.
