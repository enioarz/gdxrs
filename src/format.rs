use crate::{Error, ErrorKind, Result, model::Bytes, source::Source};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Order {
    Little,
    Big,
}
impl Order {
    pub fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes(b),
            Self::Big => u16::from_be_bytes(b),
        }
    }
    pub fn i32(self, b: [u8; 4]) -> i32 {
        match self {
            Self::Little => i32::from_le_bytes(b),
            Self::Big => i32::from_be_bytes(b),
        }
    }
    pub fn i64(self, b: [u8; 8]) -> i64 {
        match self {
            Self::Little => i64::from_le_bytes(b),
            Self::Big => i64::from_be_bytes(b),
        }
    }
    pub fn u64(self, b: [u8; 8]) -> u64 {
        match self {
            Self::Little => u64::from_le_bytes(b),
            Self::Big => u64::from_be_bytes(b),
        }
    }
}
pub(crate) struct Header {
    pub order: Order,
    pub compressed: bool,
    pub offsets: [u64; 6],
    pub data_start: u64,
    pub audit: Bytes,
    pub producer: Bytes,
}
impl Header {
    pub fn read(source: &Source) -> Result<Self> {
        let mut pos = 0;
        fn read<const N: usize>(source: &Source, pos: &mut u64) -> Result<[u8; N]> {
            let mut b = [0; N];
            source.read_exact(*pos, &mut b)?;
            *pos += N as u64;
            Ok(b)
        }
        fn string(source: &Source, pos: &mut u64) -> Result<Bytes> {
            let n = read::<1>(source, pos)?[0];
            let mut b = [0; 255];
            source.read_exact(*pos, &mut b[..usize::from(n)])?;
            *pos += u64::from(n);
            Bytes::from_slice(&b[..usize::from(n)])
        }
        let p = read::<17>(source, &mut pos)?;
        let little = [
            2, 0x34, 0x12, 4, 0x78, 0x56, 0x34, 0x12, 8, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 9,
            0x40,
        ];
        let big = [
            2, 0x12, 0x34, 4, 0x12, 0x34, 0x56, 0x78, 8, 0x40, 9, 0x21, 0xfb, 0x54, 0x44, 0x2d,
            0x18,
        ];
        let order = if p == little {
            Order::Little
        } else if p == big {
            Order::Big
        } else {
            return Err(Error::new(
                ErrorKind::UnsupportedByteOrder,
                "unsupported size/order markers",
            ));
        };
        if read::<1>(source, &mut pos)?[0] != 123
            || !string(source, &mut pos)?
                .as_slice()
                .eq_ignore_ascii_case(b"GAMSGDX")
        {
            return Err(Error::new(
                ErrorKind::InvalidHeader,
                "invalid GDX magic/identifier",
            ));
        }
        if order.i32(read(source, &mut pos)?) != 7 {
            return Err(Error::new(
                ErrorKind::UnsupportedVersion,
                "only GDX file version 7 is supported",
            ));
        }
        let compressed = match order.i32(read(source, &mut pos)?) {
            0 => false,
            1 => true,
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidHeader,
                    "invalid compression flag",
                ));
            }
        };
        let audit = string(source, &mut pos)?;
        let producer = string(source, &mut pos)?;
        let index = pos;
        if order.i32(read(source, &mut pos)?) != 19510624 {
            return Err(Error::new(
                ErrorKind::InvalidHeader,
                "invalid major index marker",
            ));
        }
        let data_start = index
            .checked_add(80)
            .ok_or_else(|| Error::new(ErrorKind::InvalidOffset, "header overflow"))?;
        if data_start > source.length {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "truncated index reservation",
            ));
        }
        let mut offsets = [0; 6];
        for (n, offset) in offsets.iter_mut().enumerate() {
            let value = order.i64(read(source, &mut pos)?);
            if n == 5 && value == 0 {
                continue;
            }
            *offset = u64::try_from(value)
                .map_err(|_| Error::new(ErrorKind::InvalidOffset, "negative section offset"))?;
            if *offset < data_start || *offset >= source.length {
                return Err(Error::new(
                    ErrorKind::InvalidOffset,
                    "section offset outside file",
                ));
            }
        }
        // Supported v7 writer layout: data, symbols, text, UELs, acronyms, optional domains.
        if !(offsets[0] < offsets[2] && offsets[2] < offsets[1] && offsets[1] < offsets[3])
            || offsets[4] != offsets[0]
            || (offsets[5] != 0 && offsets[5] <= offsets[3])
        {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                "overlapping or unsupported section layout",
            ));
        }
        Ok(Self {
            order,
            compressed,
            offsets,
            data_start,
            audit,
            producer,
        })
    }
    pub fn section(&self, index: usize, length: u64) -> Result<(u64, u64)> {
        let start = *self
            .offsets
            .get(index)
            .ok_or_else(|| Error::new(ErrorKind::InvalidOffset, "unknown section"))?;
        if start == 0 {
            return Err(Error::new(ErrorKind::InvalidOffset, "section absent"));
        }
        let end = self
            .offsets
            .iter()
            .copied()
            .filter(|v| *v > start)
            .min()
            .unwrap_or(length);
        Ok((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenOptions;
    #[test]
    fn all_valid_headers_and_version_offset_errors() {
        for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|v| v == "gdx") {
                let s = Source::open(&path, OpenOptions::default()).unwrap();
                let h = Header::read(&s).unwrap();
                assert!(s.statistics().bytes_read < h.data_start);
                assert_eq!(h.data_start, 194);
            }
        }
        for (name, kind) in [
            ("version-5-probe", ErrorKind::UnsupportedVersion),
            ("version-6-probe", ErrorKind::UnsupportedVersion),
            ("future-version-probe", ErrorKind::UnsupportedVersion),
            ("bad-offset", ErrorKind::InvalidOffset),
            ("truncated-header", ErrorKind::UnexpectedEof),
        ] {
            let s = Source::open(
                std::path::Path::new(&format!("tests/fixtures/invalid/{name}.gdx")),
                OpenOptions::default(),
            )
            .unwrap();
            assert_eq!(Header::read(&s).err().unwrap().kind, kind);
        }
    }
}
