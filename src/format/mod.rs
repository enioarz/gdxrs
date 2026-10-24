//! Checked primitives and the uncompressed v7 header (rules F01–F03).
use std::io::{self, Read};

/// A header failure and its physical byte position.
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: u64,
    pub cause: Option<io::Error>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    UnexpectedEof,
    UnsupportedVersion,
    UnsupportedByteOrder,
    InvalidHeader,
    InvalidOffset,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} at byte {}", self.kind, self.offset)
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|e| e as _)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}
impl ByteOrder {
    fn integer(self, bytes: [u8; 4]) -> i32 {
        match self {
            Self::Little => i32::from_le_bytes(bytes),
            Self::Big => i32::from_be_bytes(bytes),
        }
    }
    fn position(self, bytes: [u8; 8]) -> i64 {
        match self {
            Self::Little => i64::from_le_bytes(bytes),
            Self::Big => i64::from_be_bytes(bytes),
        }
    }
}

/// Validated uncompressed header only; metadata and records are not read.
/// Strings retain their stored bytes, including non-UTF-8 bytes.
#[derive(Debug)]
pub struct Header {
    pub byte_order: ByteOrder,
    pub compressed_metadata: bool,
    pub section_offsets: [u64; 6],
    pub data_start: u64,
    audit: [u8; 255],
    audit_len: usize,
    producer: [u8; 255],
    producer_len: usize,
}
impl Header {
    pub fn audit(&self) -> &[u8] {
        &self.audit[..self.audit_len]
    }
    pub fn producer(&self) -> &[u8] {
        &self.producer[..self.producer_len]
    }

    /// Read from physical offset zero using a caller-supplied immutable source length.
    /// Uses fixed stack storage; performs no seeks or heap allocations on success.
    /// This low-level entry point does not establish source immutability.
    pub fn read(input: impl Read, file_len: u64) -> Result<Self, Error> {
        let mut r = Checked {
            input,
            offset: 0,
            file_len,
        };
        let mut order = None;
        for pattern in [
            &[0x34, 0x12][..],
            &[0x78, 0x56, 0x34, 0x12][..],
            &[0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 0x09, 0x40][..],
        ] {
            if usize::from(r.bytes::<1>()?[0]) != pattern.len() {
                return Err(r.error(ErrorKind::InvalidHeader));
            }
            let mut bytes = [0; 8];
            r.fill(&mut bytes[..pattern.len()])?;
            let actual = &bytes[..pattern.len()];
            let current = if actual == pattern {
                ByteOrder::Little
            } else if actual.iter().eq(pattern.iter().rev()) {
                ByteOrder::Big
            } else {
                return Err(r.error(ErrorKind::InvalidHeader));
            };
            if order.is_some_and(|o| o != current) {
                return Err(r.error(ErrorKind::UnsupportedByteOrder));
            }
            order = Some(current);
        }
        let order = order.ok_or_else(|| r.error(ErrorKind::InvalidHeader))?;
        if r.bytes::<1>()? != [123] {
            return Err(r.error(ErrorKind::InvalidHeader));
        }
        let mut identifier = [0; 255];
        let len = r.string(&mut identifier)?;
        if !identifier[..len].eq_ignore_ascii_case(b"GAMSGDX") {
            return Err(r.error(ErrorKind::InvalidHeader));
        }
        if order.integer(r.bytes()?) != 7 {
            return Err(r.error(ErrorKind::UnsupportedVersion));
        }
        let compressed_metadata = match order.integer(r.bytes()?) {
            0 => false,
            1 => true,
            _ => return Err(r.error(ErrorKind::InvalidHeader)),
        };
        let mut audit = [0; 255];
        let audit_len = r.string(&mut audit)?;
        let mut producer = [0; 255];
        let producer_len = r.string(&mut producer)?;
        let index = r.offset;
        if order.integer(r.bytes()?) != 19510624 {
            return Err(r.error(ErrorKind::InvalidHeader));
        }
        let data_start = index
            .checked_add(80)
            .ok_or_else(|| r.error(ErrorKind::InvalidOffset))?;
        let mut offsets = [0; 6];
        for (i, value) in offsets.iter_mut().enumerate() {
            let raw = order.position(r.bytes()?);
            *value = u64::try_from(raw).map_err(|_| r.error(ErrorKind::InvalidOffset))?;
            if !(i == 5 && *value == 0) && !(*value >= data_start && *value < file_len) {
                return Err(r.error(ErrorKind::InvalidOffset));
            }
        }
        if r.bytes::<28>()? != [0; 28] {
            return Err(r.error(ErrorKind::InvalidHeader));
        }
        if !(offsets[4] <= offsets[0]
            && offsets[0] < offsets[2]
            && offsets[2] < offsets[1]
            && offsets[1] < offsets[3])
            || (offsets[5] != 0 && offsets[5] <= offsets[3])
        {
            return Err(r.error(ErrorKind::InvalidOffset));
        }
        Ok(Self {
            byte_order: order,
            compressed_metadata,
            section_offsets: offsets,
            data_start,
            audit,
            audit_len,
            producer,
            producer_len,
        })
    }
}
struct Checked<R> {
    input: R,
    offset: u64,
    file_len: u64,
}
impl<R: Read> Checked<R> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            offset: self.offset,
            cause: None,
        }
    }
    fn fill(&mut self, mut output: &mut [u8]) -> Result<(), Error> {
        let end = self
            .offset
            .checked_add(output.len() as u64)
            .ok_or_else(|| self.error(ErrorKind::InvalidOffset))?;
        if end > self.file_len {
            return Err(self.error(ErrorKind::UnexpectedEof));
        }
        while !output.is_empty() {
            match self.input.read(output) {
                Ok(0) => return Err(self.error(ErrorKind::UnexpectedEof)),
                Ok(n) => {
                    self.offset += n as u64;
                    output = &mut output[n..];
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(Error {
                        kind: if e.kind() == io::ErrorKind::UnexpectedEof {
                            ErrorKind::UnexpectedEof
                        } else {
                            ErrorKind::Io
                        },
                        offset: self.offset,
                        cause: Some(e),
                    });
                }
            }
        }
        Ok(())
    }
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut b = [0; N];
        self.fill(&mut b)?;
        Ok(b)
    }
    fn string(&mut self, bytes: &mut [u8; 255]) -> Result<usize, Error> {
        let n = usize::from(self.bytes::<1>()?[0]);
        self.fill(&mut bytes[..n])?;
        Ok(n)
    }
}
