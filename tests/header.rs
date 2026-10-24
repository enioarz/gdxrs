use gdxrs::format::{ByteOrder, ErrorKind, Header};
use std::io::{self, Read};
const LE: &[u8] = include_bytes!("fixtures/v7/semantics-0.gdx");
const BE: &[u8] = include_bytes!("fixtures/v7/semantics-0-be.gdx");
fn header(bytes: &[u8]) -> Result<Header, gdxrs::format::Error> {
    Header::read(bytes, bytes.len() as u64)
}
#[test]
fn oracle_headers_and_bounded_consumption() {
    for (bytes, order, compressed) in [
        (LE, ByteOrder::Little, false),
        (BE, ByteOrder::Big, false),
        (
            include_bytes!("fixtures/v7/semantics-1.gdx").as_slice(),
            ByteOrder::Little,
            true,
        ),
    ] {
        let mut remaining = bytes;
        let h = Header::read(&mut remaining, bytes.len() as u64).unwrap();
        assert_eq!(h.byte_order, order);
        assert_eq!(h.compressed_metadata, compressed);
        assert_eq!(h.producer(), b"gdxrs-fixtures");
        assert_eq!((bytes.len() - remaining.len()) as u64, h.data_start);
    }
}
#[test]
fn every_header_truncation_fails() {
    let end = header(LE).unwrap().data_start as usize;
    for n in 0..end {
        assert_eq!(
            Header::read(&LE[..n], LE.len() as u64).unwrap_err().kind,
            ErrorKind::UnexpectedEof,
            "cut {n}"
        );
    }
}
#[test]
fn rejects_versions_patterns_flags_and_offsets() {
    for version in [5i32, 6, 8, -1] {
        let mut b = LE.to_vec();
        b[26..30].copy_from_slice(&version.to_le_bytes());
        assert_eq!(header(&b).unwrap_err().kind, ErrorKind::UnsupportedVersion);
    }
    let mut b = LE.to_vec();
    b[4..8].reverse();
    assert_eq!(
        header(&b).unwrap_err().kind,
        ErrorKind::UnsupportedByteOrder
    );
    for (at, kind) in [
        (0, ErrorKind::InvalidHeader),
        (17, ErrorKind::InvalidHeader),
        (30, ErrorKind::InvalidHeader),
    ] {
        let mut b = LE.to_vec();
        b[at] = 99;
        assert_eq!(header(&b).unwrap_err().kind, kind);
    }
    let h = header(LE).unwrap();
    let index = h.data_start as usize - 80;
    for value in [-1i64, 0, i64::MAX] {
        let mut b = LE.to_vec();
        b[index + 4..index + 12].copy_from_slice(&value.to_le_bytes());
        assert_eq!(header(&b).unwrap_err().kind, ErrorKind::InvalidOffset);
    }
    let mut b = LE.to_vec();
    b[index + 52] = 1;
    assert_eq!(header(&b).unwrap_err().kind, ErrorKind::InvalidHeader);
    let mut b = LE.to_vec();
    b[index + 20..index + 28].copy_from_slice(&h.section_offsets[0].to_le_bytes());
    assert_eq!(header(&b).unwrap_err().kind, ErrorKind::InvalidOffset);
}
struct Short<'a> {
    bytes: &'a [u8],
    interrupt: bool,
    fail_at: Option<usize>,
    read: usize,
}
impl Read for Short<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.fail_at == Some(self.read) {
            return Err(io::Error::other("injected"));
        }
        let n = self.bytes.read(&mut out[..1])?;
        self.read += n;
        Ok(n)
    }
}
#[test]
fn short_reads_interruptions_and_io_context() {
    let source = Short {
        bytes: LE,
        interrupt: true,
        fail_at: None,
        read: 0,
    };
    assert!(Header::read(source, LE.len() as u64).is_ok());
    let source = Short {
        bytes: LE,
        interrupt: false,
        fail_at: Some(31),
        read: 0,
    };
    let e = Header::read(source, LE.len() as u64).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Io);
    assert_eq!(e.offset, 31);
    assert!(std::error::Error::source(&e).is_some());
}
#[test]
fn optional_domains_and_negative_fixtures() {
    let h = header(include_bytes!("fixtures/v7/optional-domains-absent.gdx")).unwrap();
    assert_eq!(h.section_offsets[5], 0);
    assert_eq!(
        header(include_bytes!("fixtures/invalid/bad-offset.gdx"))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidOffset
    );
    assert_eq!(
        header(include_bytes!("fixtures/invalid/truncated-header.gdx"))
            .unwrap_err()
            .kind,
        ErrorKind::UnexpectedEof
    );
}
