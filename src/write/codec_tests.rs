use super::encoder::Encoder;
#[cfg(feature = "compression")]
use super::tests::fault;
use super::tests::{Parent, Reset, options};
use super::*;

#[test]
fn raw_section_primitives_are_independent_fixed_bytes_and_sink_failure_is_terminal() {
    let root = Budget::new(1024 * 1024);
    let mut e = Encoder::new(&root, WriteCompression::None).unwrap();
    let mut bytes = Vec::new(); // Small, explicitly bounded test collector only.
    e.i32(-123456, |b| {
        bytes.extend_from_slice(b);
        Ok(())
    })
    .unwrap();
    e.string(b"\x00\xffabc", |b| {
        bytes.extend_from_slice(b);
        Ok(())
    })
    .unwrap();
    e.bytes(&0x0123456789abcdefu64.to_le_bytes(), |b| {
        bytes.extend_from_slice(b);
        Ok(())
    })
    .unwrap();
    e.end(|_| panic!("raw end must emit nothing")).unwrap();
    assert_eq!(
        bytes,
        b"\xc0\x1d\xfe\xff\x05\x00\xffabc\xef\xcd\xab\x89\x67\x45\x23\x01"
    );
    assert_eq!(root.statistics().current_bytes, 0);
    assert_eq!(
        e.bytes(b"x", |_| Err(Error::new(
            ErrorKind::OutputLimitExceeded,
            "test"
        )))
        .unwrap_err()
        .kind,
        ErrorKind::OutputLimitExceeded
    );
    assert_eq!(
        e.end(|_| panic!("retry must not emit")).unwrap_err().kind,
        ErrorKind::InvalidWriteState
    );
    let mut e = Encoder::new(&root, WriteCompression::None).unwrap();
    assert_eq!(
        e.string(&[0; 256], |_| panic!("oversize string must not emit"))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidOptions
    );
    assert_eq!(
        e.end(|_| panic!("failed validation must not emit"))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidWriteState
    );
}

#[cfg(not(feature = "compression"))]
#[test]
fn reduced_writer_rejects_zlib_before_files_or_reservations() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let root = Budget::new(1024 * 1024);
    assert_eq!(
        Encoder::new(&root, WriteCompression::Zlib)
            .err()
            .unwrap()
            .kind,
        ErrorKind::CompressionUnavailable
    );
    assert_eq!(root.statistics().peak_bytes, 0);
    assert_eq!(
        Writer::create(
            parent.destination(),
            WriteOptions {
                compression: WriteCompression::Zlib,
                ..options()
            }
        )
        .err()
        .unwrap()
        .kind,
        ErrorKind::CompressionUnavailable
    );
    parent.empty();
}

#[cfg(feature = "compression")]
fn encode(input: &[u8], stride: usize) -> Vec<u8> {
    let root = Budget::new(1024 * 1024);
    let mut e = Encoder::new(&root, WriteCompression::Zlib).unwrap();
    let mut out = Vec::new();
    for b in input.chunks(stride) {
        e.bytes(b, |b| {
            assert!(b.len() <= 32771);
            out.extend_from_slice(b);
            Ok(())
        })
        .unwrap();
    }
    e.end(|b| {
        out.extend_from_slice(b);
        Ok(())
    })
    .unwrap();
    e.end(|_| panic!("empty flush emitted a block")).unwrap();
    assert!(root.statistics().peak_bytes < 512 * 1024);
    drop(e);
    assert_eq!(root.statistics().current_bytes, 0);
    out
}
#[cfg(feature = "compression")]
fn decode(bytes: &[u8]) -> Vec<u8> {
    use miniz_oxide::inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress, inflate_flags},
    };
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let tag = bytes[offset];
        let n = u16::from_be_bytes(bytes[offset + 1..offset + 3].try_into().unwrap()) as usize;
        assert!((1..=32768).contains(&n));
        let payload = &bytes[offset + 3..offset + 3 + n];
        if tag == 0 {
            out.extend_from_slice(payload);
        } else {
            assert_eq!(tag, 1);
            let mut state = DecompressorOxide::new();
            let mut decoded = [0; 32768];
            let (status, used, written) = decompress(
                &mut state,
                payload,
                &mut decoded,
                0,
                inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
                    | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
            );
            assert_eq!((status, used), (TINFLStatus::Done, n));
            assert!(written > n && written <= 32768);
            out.extend_from_slice(&decoded[..written]);
        }
        offset += n + 3;
    }
    out
}

#[cfg(feature = "compression")]
#[test]
fn independent_vectors_empty_single_full_final_incompressible_and_resets() {
    assert!(encode(b"", 1).is_empty());
    assert_eq!(encode(b"x", 1), b"\x00\x00\x01x");
    assert_eq!(encode(b"abc", 1), b"\x00\x00\x03abc");
    // Deterministic independent input recipe, not an encoder-derived expectation.
    let mut noise = vec![0; 32768];
    let mut seed = 0x12345678u32;
    for b in &mut noise {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *b = seed as u8;
    }
    let stored = encode(&noise, 113);
    assert_eq!(&stored[..3], b"\x00\x80\x00");
    assert_eq!(&stored[3..], noise);
    for len in [32767, 32768, 32769, 65536, 65537] {
        let data = vec![b'x'; len];
        let whole = encode(&data, len);
        assert_eq!(encode(&data, 7), whole);
        assert_eq!(whole[0], 1);
        assert_eq!(decode(&whole), data);
        // Each repeated full block must use a fresh dictionary and checksum.
        if len >= 65536 {
            let n = u16::from_be_bytes(whole[1..3].try_into().unwrap()) as usize + 3;
            assert_eq!(&whole[..n], &whole[n..2 * n]);
        }
    }
}

#[cfg(feature = "compression")]
#[test]
fn boundary_spanning_strings_scalars_checksum_and_budget_failure() {
    let root = Budget::new(1024 * 1024);
    let mut e = Encoder::new(&root, WriteCompression::Zlib).unwrap();
    let mut encoded = Vec::new();
    let mut sink = |b: &[u8]| {
        encoded.extend_from_slice(b);
        Ok(())
    };
    e.bytes(&[b'a'; 32766], &mut sink).unwrap();
    e.i32(0x12345678, &mut sink).unwrap();
    e.string(&[b'z'; 255], &mut sink).unwrap();
    e.bytes(&[b'q'; 32508], &mut sink).unwrap();
    e.string(b"boundary", &mut sink).unwrap();
    e.end(&mut sink).unwrap();
    let mut expected = vec![b'a'; 32766];
    expected.extend_from_slice(b"\x78\x56\x34\x12\xff");
    expected.extend_from_slice(&[b'z'; 255]);
    expected.extend_from_slice(&[b'q'; 32508]);
    expected.extend_from_slice(b"\x08boundary");
    assert_eq!(decode(&encoded), expected);
    let n = u16::from_be_bytes(encoded[1..3].try_into().unwrap()) as usize;
    encoded[3 + n - 1] ^= 1;
    use miniz_oxide::inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress, inflate_flags},
    };
    let (status, _, _) = decompress(
        &mut DecompressorOxide::new(),
        &encoded[3..3 + n],
        &mut [0; 32768],
        0,
        inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
            | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    assert_eq!(status, TINFLStatus::Adler32Mismatch);
    drop(e);
    let small = Budget::new(1);
    assert_eq!(
        Encoder::new(&small, WriteCompression::Zlib)
            .err()
            .unwrap()
            .kind,
        ErrorKind::BudgetTooSmall
    );
    assert_eq!(small.statistics().current_bytes, 0);
    let visible = std::mem::size_of::<miniz_oxide::deflate::core::CompressorOxide>();
    let stages = [
        320 * 1024,
        320 * 1024 + visible,
        320 * 1024 + visible + 32768,
        320 * 1024 + visible + 32768 + 32771,
    ];
    for extent in stages {
        let quota = Budget::new(extent - 1);
        assert_eq!(
            Encoder::new(&quota, WriteCompression::Zlib)
                .err()
                .unwrap()
                .kind,
            ErrorKind::BudgetTooSmall
        );
        assert_eq!(quota.statistics().current_bytes, 0);
        assert!(quota.statistics().peak_bytes < extent);
    }
    let exact = Budget::new(stages[3]);
    let state = Encoder::new(&exact, WriteCompression::Zlib).unwrap();
    assert_eq!(exact.statistics().current_bytes, stages[3]);
    drop(state);
    assert_eq!(exact.statistics().current_bytes, 0);
    crate::budget::inject_buffer_failure();
    assert_eq!(
        Encoder::new(&root, WriteCompression::Zlib)
            .err()
            .unwrap()
            .kind,
        ErrorKind::AllocationFailed
    );
    assert_eq!(root.statistics().current_bytes, 0);
    let mut e = Encoder::new(&root, WriteCompression::Zlib).unwrap();
    e.bytes(b"final", |_| panic!("partial block emitted early"))
        .unwrap();
    assert_eq!(
        e.end(|_| Err(Error::new(ErrorKind::Io, "test sink")))
            .unwrap_err()
            .kind,
        ErrorKind::Io
    );
    assert_eq!(
        e.end(|_| panic!("failed flush replayed")).unwrap_err().kind,
        ErrorKind::InvalidWriteState
    );
    drop(e);
    assert_eq!(root.statistics().current_bytes, 0);
    let mut e = Encoder::new(&root, WriteCompression::Zlib).unwrap();
    let mut calls = 0;
    assert_eq!(
        e.bytes(&[b'x'; 65537], |_| {
            calls += 1;
            Err(Error::new(ErrorKind::Io, "full block sink"))
        })
        .unwrap_err()
        .kind,
        ErrorKind::Io
    );
    assert_eq!(calls, 1);
    assert_eq!(
        e.bytes(b"retry", |_| panic!("failed full block replayed"))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidWriteState
    );
    drop(e);
    assert_eq!(root.statistics().current_bytes, 0);
}

#[cfg(feature = "compression")]
#[test]
fn compressed_writer_limits_faults_and_finalization_preserve_transaction() {
    let _reset = Reset::new();
    let parent = Parent::new();
    let opts = WriteOptions {
        compression: WriteCompression::Zlib,
        ..options()
    };
    let mut w = Writer::create(parent.destination(), opts.clone()).unwrap();
    let root = Arc::clone(&w.budget);
    let stats = w.finish().unwrap();
    assert!(stats.published);
    let length = stats.output_bytes;
    assert_eq!(stats.bytes_written, length + 52);
    assert!(length <= 209);
    drop(w);
    assert_eq!(root.statistics().current_bytes, 0);
    fs::remove_file(parent.destination()).unwrap();
    let mut w = Writer::create(
        parent.destination(),
        WriteOptions {
            maximum_output_bytes: length,
            ..opts.clone()
        },
    )
    .unwrap();
    assert_eq!(w.finish().unwrap().output_bytes, length);
    drop(w);
    fs::remove_file(parent.destination()).unwrap();
    // Admission is over the complete framed block, before either header or payload.
    let mut w = Writer::create(parent.destination(), opts.clone()).unwrap();
    w.limit = 126 + 3;
    assert_eq!(w.finish().unwrap_err().kind, ErrorKind::OutputLimitExceeded);
    assert_eq!(w.statistics().output_bytes, 126);
    assert!(!w.statistics().published);
    drop(w);
    parent.empty();
    for event in [Event::Write, Event::Seek, Event::Sync, Event::Publish] {
        let mut w = Writer::create(parent.destination(), opts.clone()).unwrap();
        fault(event, 0, false, io::ErrorKind::StorageFull);
        assert_eq!(w.finish().unwrap_err().kind, ErrorKind::Io);
        assert_eq!(w.statistics().status, WriteStatus::Failed);
        assert!(!w.statistics().published);
        parent.empty();
    }
}

// Optional development export/verification uses the same test on fresh bounded
// fixtures. Ordinary CI executes the full Rust stream checks in a private temp.
#[cfg(feature = "compression")]
#[test]
fn native_codec_probes() {
    use crate::{format::Order, source::Source, stream::Stream};
    let parent = Parent::new();
    let area = std::env::var_os("GDXRS_CODEC_PROBES")
        .map(PathBuf::from)
        .unwrap_or_else(|| parent.destination().parent().unwrap().to_owned());
    let verify_native = std::env::var_os("GDXRS_CODEC_VERIFY_NATIVE").is_some();
    let mut noise = vec![0; 32768];
    let mut seed = 0x12345678u32;
    for b in &mut noise {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *b = seed as u8;
    }
    let cases = [
        ("empty", vec![]),
        ("one", vec![b'x']),
        ("small", b"abc".to_vec()),
        ("full", vec![b'x'; 32768]),
        ("final", vec![b'x'; 32769]),
        ("repeat", vec![b'x'; 65537]),
        ("noise", noise),
        (
            "boundary",
            [
                vec![b'a'; 32766],
                b"\x78\x56\x34\x12\xff".to_vec(),
                vec![b'z'; 255],
                vec![b'q'; 32508],
                b"\x08boundary".to_vec(),
            ]
            .concat(),
        ),
    ];
    for (name, expected) in cases {
        let path = if verify_native {
            area.join(format!("{name}.native"))
        } else {
            let encoded = encode(&expected, 17);
            let path = area.join(format!("{name}.blocks"));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(&encoded).unwrap();
            drop(file);
            let mut raw = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(area.join(format!("{name}.raw")))
                .unwrap();
            raw.write_all(&expected).unwrap();
            path
        };
        let source = Source::open(&path, OpenOptions::default()).unwrap();
        let length = source.length;
        assert!(length <= 131072);
        if expected.is_empty() {
            assert_eq!(length, 0);
            continue;
        }
        let mut stream =
            Stream::new(source, 0, length, true, Order::Little, "writer codec probe").unwrap();
        let mut actual = vec![0; expected.len()]; // Test-only, <=65543 bytes.
        stream.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected, "{name}");
        assert_eq!(stream.byte().unwrap_err().kind, ErrorKind::UnexpectedEof);
    }
    eprintln!(
        "encoder_visible_state_bytes={}",
        std::mem::size_of::<miniz_oxide::deflate::core::CompressorOxide>()
    );
}
