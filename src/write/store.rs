//! Private checksummed journal storage, with bounded buffers and explicit extent.
use super::*;
use std::io::Read;
const FRAME: usize = 1024;
const INITIAL: u64 = 14695981039346656037;
fn hash(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    h
}
pub(super) struct Frame {
    pub tag: u8,
    pub bytes: [u8; FRAME],
    pub len: usize,
}
impl Frame {
    pub fn payload(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
pub(super) struct Store {
    file: Option<fs::File>,
    buffer: Buffer<u8>,
    pub length: u64,
}
impl Store {
    pub fn create(path: &Path, budget: &Arc<Budget>) -> Result<Self> {
        let buffer = Buffer::new(budget, 32768)?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        event(Event::MetadataCreate).map_err(Error::io)?;
        let file = options.open(path).map_err(Error::io)?;
        Ok(Self {
            file: Some(file),
            buffer,
            length: 0,
        })
    }
    fn file(&mut self) -> Result<&mut fs::File> {
        self.file
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata file closed"))
    }
    pub fn flush(&mut self) -> Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata file closed"))?;
        if self.buffer.data.is_empty() {
            return Ok(());
        }
        event(Event::MetadataSeek).map_err(Error::io)?;
        file.seek(SeekFrom::Start(self.length - self.buffer.data.len() as u64))
            .map_err(Error::io)?;
        let mut done = 0;
        while done < self.buffer.data.len() {
            let n = io_chunk(self.buffer.data.len() - done);
            match event(Event::MetadataWrite)
                .and_then(|_| file.write(&self.buffer.data[done..done + n]))
            {
                Ok(0) => {
                    return Err(Error::io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "metadata zero write",
                    )));
                }
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        self.buffer.data.clear();
        Ok(())
    }
    pub fn append(
        &mut self,
        tag: u8,
        payload: &[u8],
        used: &mut u64,
        peak: &mut u64,
        limit: u64,
    ) -> Result<()> {
        if !(1..=8).contains(&tag) || payload.len() > FRAME {
            return Err(Error::new(
                ErrorKind::InvalidSpill,
                "invalid metadata frame",
            ));
        }
        let size = 11 + payload.len() as u64;
        let end = self.length.checked_add(size).ok_or_else(|| {
            Error::new(
                ErrorKind::TemporarySpaceExceeded,
                "metadata extent overflow",
            )
        })?;
        let next = used
            .checked_add(size)
            .filter(|v| *v <= limit)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::TemporarySpaceExceeded,
                    "metadata scratch quota exceeded",
                )
            })?;
        let mut frame = [0u8; FRAME + 11];
        frame[0] = tag;
        frame[1..3].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        frame[3..3 + payload.len()].copy_from_slice(payload);
        let digest = hash(
            hash(INITIAL, &self.length.to_le_bytes()),
            &frame[..3 + payload.len()],
        );
        frame[3 + payload.len()..11 + payload.len()].copy_from_slice(&digest.to_le_bytes());
        if self.buffer.data.len() + size as usize > 32768 {
            self.flush()?;
        }
        *used = next;
        *peak = (*peak).max(next);
        self.buffer.data.extend_from_slice(&frame[..size as usize]);
        self.length = end;
        Ok(())
    }
    pub fn read_at(&mut self, at: u64, bytes: &mut [u8]) -> Result<()> {
        self.flush()?;
        if at
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > self.length)
        {
            return Err(Error::new(
                ErrorKind::InvalidSpill,
                "metadata read outside extent",
            ));
        }
        event(Event::MetadataSeek).map_err(Error::io)?;
        let length = self.length;
        let file = self.file()?;
        if file.metadata().map_err(Error::io)?.len() != length {
            return Err(Error::new(
                ErrorKind::InvalidSpill,
                "metadata extent changed",
            ));
        }
        // Reborrow after checking the recorded high-water mark.
        let file = self.file()?;
        file.seek(SeekFrom::Start(at)).map_err(Error::io)?;
        event(Event::MetadataRead).map_err(Error::io)?;
        file.read_exact(bytes).map_err(Error::io)
    }
    pub fn next(&mut self, position: &mut u64) -> Result<Option<Frame>> {
        if *position == self.length {
            self.read_at(*position, &mut [])?;
            return Ok(None);
        }
        let mut header = [0u8; 3];
        self.read_at(*position, &mut header)?;
        let len = usize::from(u16::from_le_bytes([header[1], header[2]]));
        if !(1..=8).contains(&header[0]) || len > FRAME {
            return Err(Error::new(
                ErrorKind::InvalidSpill,
                "corrupt metadata frame header",
            ));
        }
        let mut f = Frame {
            tag: header[0],
            bytes: [0; FRAME],
            len,
        };
        self.read_at(*position + 3, &mut f.bytes[..len])?;
        let mut digest = [0; 8];
        self.read_at(*position + 3 + len as u64, &mut digest)?;
        if u64::from_le_bytes(digest)
            != hash(
                hash(hash(INITIAL, &position.to_le_bytes()), &header),
                f.payload(),
            )
        {
            return Err(Error::new(
                ErrorKind::InvalidSpill,
                "metadata checksum mismatch",
            ));
        }
        *position = position
            .checked_add(11 + len as u64)
            .ok_or_else(|| Error::new(ErrorKind::InvalidSpill, "metadata frame overflow"))?;
        Ok(Some(f))
    }
    pub fn sync(&mut self) -> Result<()> {
        self.flush()?;
        event(Event::MetadataSync).map_err(Error::io)?;
        self.file()?.sync_all().map_err(Error::io)
    }
}
impl Store {
    pub fn integer(&mut self, index: u32) -> Result<i64> {
        if index == 0 {
            return Err(Error::new(ErrorKind::InvalidSpill, "zero ordinal index"));
        }
        let mut at = u64::from(index - 1) * 19;
        let f = self
            .next(&mut at)?
            .ok_or_else(|| Error::new(ErrorKind::InvalidSpill, "missing ordinal index"))?;
        if f.tag != 8 || f.len != 8 {
            return Err(Error::new(ErrorKind::InvalidSpill, "invalid ordinal frame"));
        }
        let mut b = [0; 8];
        b.copy_from_slice(f.payload());
        Ok(i64::from_le_bytes(b))
    }
    pub fn set_integer(&mut self, index: u32, value: i64) -> Result<()> {
        self.integer(index)?;
        let at = u64::from(index - 1) * 19;
        let mut b = [0u8; 19];
        b[..3].copy_from_slice(&[8, 8, 0]);
        b[3..11].copy_from_slice(&value.to_le_bytes());
        let h = hash(hash(INITIAL, &at.to_le_bytes()), &b[..11]);
        b[11..].copy_from_slice(&h.to_le_bytes());
        event(Event::MetadataSeek).map_err(Error::io)?;
        let file = self.file()?;
        file.seek(SeekFrom::Start(at)).map_err(Error::io)?;
        event(Event::MetadataWrite).map_err(Error::io)?;
        file.write_all(&b).map_err(Error::io)
    }
}
