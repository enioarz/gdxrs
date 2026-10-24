use crate::{
    Error, ErrorKind, Result,
    budget::{Buffer, Reservation},
    format::Order,
    model::Bytes,
    source::Source,
};
#[cfg(feature = "compression")]
use miniz_oxide::inflate::{
    TINFLStatus,
    core::{DecompressorOxide, decompress, inflate_flags},
};
use std::{mem::size_of, sync::Arc};

pub(crate) struct Stream {
    source: Arc<Source>,
    order: Order,
    compressed: bool,
    start: u64,
    end: u64,
    next: u64,
    block: u64,
    output: Buffer<u8>,
    #[cfg(feature = "compression")]
    input: Option<Buffer<u8>>,
    index: usize,
    valid: usize,
    logical: u64,
    label: &'static str,
    #[cfg(feature = "compression")]
    codec: DecompressorOxide,
    _codec: Reservation,
    _state: Reservation,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Checkpoint {
    pub physical: u64,
    pub decoded: usize,
}
impl Stream {
    pub fn checkpoint(&self) -> Checkpoint {
        if self.compressed {
            Checkpoint {
                physical: self.block,
                decoded: self.index,
            }
        } else {
            Checkpoint {
                physical: self.block + self.index as u64,
                decoded: 0,
            }
        }
    }
    pub fn resume(
        source: Arc<Source>,
        point: Checkpoint,
        end: u64,
        compressed: bool,
        order: Order,
        label: &'static str,
    ) -> Result<Self> {
        let mut stream = Self::new(source, point.physical, end, compressed, order, label)?;
        if point.decoded != 0 {
            stream.fill()?;
            if point.decoded > stream.valid {
                return Err(stream.error(ErrorKind::InvalidOffset, "invalid decoded checkpoint"));
            }
            stream.index = point.decoded;
        }
        Ok(stream)
    }
    pub fn new(
        source: Arc<Source>,
        start: u64,
        end: u64,
        compressed: bool,
        order: Order,
        label: &'static str,
    ) -> Result<Self> {
        if start >= end || end > source.length {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                "invalid section boundaries",
            ));
        }
        #[cfg(not(feature = "compression"))]
        if compressed {
            return Err(Error::new(
                ErrorKind::CompressionUnavailable,
                "compression feature is disabled",
            ));
        }
        if size_of::<Self>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "stream state exceeds audited bound",
            ));
        }
        let state = source.budget.reserve(16 * 1024)?;
        let codec = source.budget.reserve(Self::codec_state_bytes())?;
        if Self::codec_state_bytes() > 128 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "codec exceeds audited scratch ceiling",
            ));
        }
        let output = Buffer::zeroed(&source.budget, 32768)?;
        #[cfg(feature = "compression")]
        let input = if compressed {
            Some(Buffer::zeroed(&source.budget, 65535)?)
        } else {
            None
        };
        Ok(Self {
            source,
            order,
            compressed,
            start,
            end,
            next: start,
            block: start,
            output,
            #[cfg(feature = "compression")]
            input,
            index: 0,
            valid: 0,
            logical: 0,
            label,
            #[cfg(feature = "compression")]
            codec: DecompressorOxide::new(),
            _codec: codec,
            _state: state,
        })
    }
    pub fn codec_state_bytes() -> usize {
        #[cfg(feature = "compression")]
        {
            size_of::<DecompressorOxide>()
        }
        #[cfg(not(feature = "compression"))]
        {
            0
        }
    }
    pub fn context(&self, error: Error) -> Error {
        let physical = if self.compressed {
            self.block
        } else {
            self.block + self.index as u64
        };
        error.at(
            self.source.id,
            self.label,
            physical,
            self.compressed.then_some(self.index as u32),
        )
    }
    pub fn error(&self, kind: ErrorKind, message: &'static str) -> Error {
        self.context(Error::new(kind, message))
    }
    fn fill(&mut self) -> Result<()> {
        if self.next >= self.end {
            return Err(self.error(ErrorKind::UnexpectedEof, "section exhausted"));
        }
        self.block = self.next;
        self.index = 0;
        if !self.compressed {
            let n = usize::try_from((self.end - self.next).min(32768))
                .map_err(|_| self.error(ErrorKind::InvalidOffset, "buffer span overflow"))?;
            self.source
                .read_exact(self.next, &mut self.output.data[..n])?;
            self.next += n as u64;
            self.valid = n;
            return Ok(());
        }
        #[cfg(feature = "compression")]
        {
            if self.end - self.next < 3 {
                return Err(self.error(ErrorKind::UnexpectedEof, "truncated compression header"));
            }
            let mut header = [0; 3];
            self.source.read_exact(self.next, &mut header)?;
            self.next += 3;
            let n = usize::from(u16::from_be_bytes([header[1], header[2]]));
            if header[0] > 1 || n == 0 {
                return Err(self.error(ErrorKind::InvalidCompression, "invalid block tag/length"));
            }
            if n as u64 > self.end - self.next {
                return Err(self.error(ErrorKind::UnexpectedEof, "truncated block payload"));
            }
            if header[0] == 0 {
                if n > 32768 {
                    return Err(
                        self.error(ErrorKind::InvalidCompression, "stored block exceeds 32 KiB")
                    );
                }
                self.source
                    .read_exact(self.next, &mut self.output.data[..n])?;
                self.valid = n;
            } else {
                let input = self.input.as_mut().ok_or_else(|| {
                    Error::new(ErrorKind::InvalidCompression, "missing codec buffer")
                })?;
                self.source.read_exact(self.next, &mut input.data[..n])?;
                self.codec.init();
                let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
                    | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
                let (status, used, written) = decompress(
                    &mut self.codec,
                    &input.data[..n],
                    &mut self.output.data,
                    0,
                    flags,
                );
                if status != TINFLStatus::Done || used != n || written == 0 || written > 32768 {
                    return Err(self.error(
                        ErrorKind::InvalidCompression,
                        "invalid zlib/checksum/framing or decoded size",
                    ));
                }
                self.valid = written;
            }
            self.next += n as u64;
            Ok(())
        }
        #[cfg(not(feature = "compression"))]
        {
            Err(self.error(
                ErrorKind::CompressionUnavailable,
                "compression feature is disabled",
            ))
        }
    }
    pub fn read_exact(&mut self, mut out: &mut [u8]) -> Result<()> {
        while !out.is_empty() {
            if self.index == self.valid {
                self.fill()?;
            }
            let n = out.len().min(self.valid - self.index);
            out[..n].copy_from_slice(&self.output.data[self.index..self.index + n]);
            self.index += n;
            self.logical = self
                .logical
                .checked_add(n as u64)
                .ok_or_else(|| self.error(ErrorKind::InvalidOffset, "decoded position overflow"))?;
            out = &mut out[n..];
        }
        Ok(())
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut b = [0; N];
        self.read_exact(&mut b)?;
        Ok(b)
    }
    pub fn byte(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub fn boolean(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(self.error(ErrorKind::InvalidHeader, "invalid Boolean flag")),
        }
    }
    pub fn i32(&mut self) -> Result<i32> {
        let b = self.array()?;
        Ok(self.order.i32(b))
    }
    pub fn i64(&mut self) -> Result<i64> {
        let b = self.array()?;
        Ok(self.order.i64(b))
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.array()?;
        Ok(self.order.u16(b))
    }
    pub fn bits(&mut self) -> Result<u64> {
        let b = self.array()?;
        Ok(self.order.u64(b))
    }
    pub fn count(&mut self) -> Result<u32> {
        let n = u32::try_from(self.i32()?)
            .map_err(|_| self.error(ErrorKind::InvalidCount, "negative count"))?;
        let bound = if self.compressed {
            u128::from(self.end - self.start) * 8192
        } else {
            u128::from(self.end - self.start)
        };
        if u128::from(n) > bound {
            return Err(self.error(ErrorKind::InvalidCount, "count exceeds section capacity"));
        }
        Ok(n)
    }
    pub fn string(&mut self) -> Result<Bytes> {
        let n = usize::from(self.byte()?);
        let mut b = [0; 255];
        self.read_exact(&mut b[..n])?;
        Bytes::from_slice(&b[..n])
    }
    pub fn marker(&mut self, marker: &[u8]) -> Result<()> {
        if self.string()?.as_slice() != marker {
            return Err(self.error(ErrorKind::InvalidHeader, "section marker mismatch"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    #[test]
    fn raw_section_and_codec_accounting() {
        let source = Source::open(
            std::path::Path::new("tests/fixtures/v7/optional-domains.gdx"),
            OpenOptions::default(),
        )
        .unwrap();
        let h = crate::format::Header::read(&source).unwrap();
        let base = source.budget.statistics().current_bytes;
        let (start, end) = h.section(0, source.length).unwrap();
        let mut s =
            Stream::new(Arc::clone(&source), start, end, false, h.order, "symbols").unwrap();
        s.marker(b"_SYMB_").unwrap();
        assert_eq!(s.count().unwrap(), 1);
        drop(s);
        assert_eq!(source.budget.statistics().current_bytes, base);
        assert!(Stream::codec_state_bytes() <= 128 * 1024);
        eprintln!("codec_state_bytes={}", Stream::codec_state_bytes());
    }
    #[cfg(feature = "compression")]
    #[test]
    fn compressed_blocks_and_cross_block_primitives() {
        let source = Source::open(
            std::path::Path::new("tests/fixtures/v7/blocks.gdx"),
            OpenOptions::default(),
        )
        .unwrap();
        let h = crate::format::Header::read(&source).unwrap();
        let mut s = Stream::new(source, 194, h.offsets[0], true, h.order, "records").unwrap();
        let mut hash = 14695981039346656037u64;
        for _ in 0..50025 {
            hash = (hash ^ u64::from(s.byte().unwrap())).wrapping_mul(1099511628211);
        }
        assert_eq!(hash, 0xd239cf79ab46d26a); // Independent Python byte probe, not learned from Rust output.
        assert_eq!(s.byte().unwrap_err().kind, ErrorKind::UnexpectedEof);
        let source = Source::open(
            std::path::Path::new("tests/fixtures/v7/blocks.gdx"),
            OpenOptions::default(),
        )
        .unwrap();
        let mut stream = Stream::new(source, 194, h.offsets[0], true, h.order, "records").unwrap();
        for _ in 0..32765 {
            stream.byte().unwrap();
        }
        assert_eq!(
            stream.array::<8>().unwrap(),
            [0x0a, 0xbc, 0xe7, 0xa5, 0xb2, 0xa4, 0xdb, 0xfc]
        );
    }
    #[cfg(feature = "compression")]
    #[test]
    fn damaged_checksum_is_rejected() {
        let source = Source::open(
            std::path::Path::new("tests/fixtures/invalid/damaged-zlib-checksum.gdx"),
            OpenOptions::default(),
        )
        .unwrap();
        let h = crate::format::Header::read(&source).unwrap();
        let (start, end) = h.section(0, source.length).unwrap();
        let mut s = Stream::new(source, start, end, true, h.order, "symbols").unwrap();
        assert_eq!(
            s.marker(b"_SYMB_").unwrap_err().kind,
            ErrorKind::InvalidCompression
        );
    }
    #[cfg(feature = "compression")]
    #[test]
    fn output_larger_than_32k_is_rejected_without_growth() {
        struct Memory(Vec<u8>);
        impl crate::source::ReadAt for Memory {
            fn read_at(&self, at: u64, out: &mut [u8]) -> std::io::Result<usize> {
                let at = usize::try_from(at).map_err(std::io::Error::other)?;
                let rest = self.0.get(at..).unwrap_or(&[]);
                let n = rest.len().min(out.len());
                out[..n].copy_from_slice(&rest[..n]);
                Ok(n)
            }
            fn unchanged(&self) -> std::io::Result<bool> {
                Ok(true)
            }
        }
        // Independently generated zlib stream of 32,769 zero bytes.
        let payload = &[
            120, 156, 237, 193, 1, 1, 0, 0, 0, 128, 144, 254, 175, 238, 8, 10, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 104, 128, 1, 0,
            1,
        ];
        let mut data = vec![1, 0, payload.len() as u8];
        data.extend_from_slice(payload);
        let len = data.len() as u64;
        let source = Source::testing(Box::new(Memory(data)), len);
        let mut s = Stream::new(Arc::clone(&source), 0, len, true, Order::Little, "bomb").unwrap();
        assert_eq!(s.byte().unwrap_err().kind, ErrorKind::InvalidCompression);
        assert!(source.budget.statistics().peak_bytes < 160 * 1024);
    }
    #[cfg(not(feature = "compression"))]
    #[test]
    fn unavailable_compression_is_explicit() {
        let source = Source::open(
            std::path::Path::new("tests/fixtures/v7/blocks.gdx"),
            OpenOptions::default(),
        )
        .unwrap();
        let end = source.length;
        assert_eq!(
            Stream::new(source, 194, end, true, Order::Little, "records")
                .err()
                .unwrap()
                .kind,
            ErrorKind::CompressionUnavailable
        );
    }
}
