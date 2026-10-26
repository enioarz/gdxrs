//! Private checksummed journal storage, with bounded buffers and explicit extent.
use super::*;
use std::io::Read;
const FRAME: usize = 1024;
// Whole 19-byte ordinal frames; variable journal frames may cross pages.
const PAGE: usize = 19 * 1724;
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
    cache_start: u64,
    cache_len: usize,
    cache_dirty: bool,
    stamp: Option<std::time::SystemTime>,
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
            cache_start: 0,
            cache_len: 0,
            cache_dirty: false,
            stamp: None,
        })
    }
    fn file(&mut self) -> Result<&mut fs::File> {
        self.file
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata file closed"))
    }
    fn flush_append(&mut self) -> Result<()> {
        if self.cache_len != 0 {
            return Ok(());
        }
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
        self.cache_len = 0;
        self.stamp = Some(
            file.metadata()
                .map_err(Error::io)?
                .modified()
                .map_err(Error::io)?,
        );
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
        if self.cache_dirty {
            self.validate_snapshot()?;
            self.flush_cache()?;
        }
        if self.cache_len != 0 {
            self.cache_len = 0;
            self.buffer.data.clear();
        }
        if !(1..=9).contains(&tag) || payload.len() > FRAME {
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
    fn invalid_snapshot() -> Error {
        Error::new(
            ErrorKind::InvalidSpill,
            "metadata extent or cached snapshot changed",
        )
    }
    fn validate_snapshot(&mut self) -> Result<()> {
        self.flush_append()?;
        let m = self.file()?.metadata().map_err(Error::io)?;
        if m.len() != self.length {
            return Err(Self::invalid_snapshot());
        }
        let stamp = m.modified().map_err(Error::io)?;
        if self.stamp != Some(stamp) {
            if self.cache_dirty {
                return Err(Self::invalid_snapshot());
            }
            self.cache_len = 0;
            self.buffer.data.clear();
            self.stamp = Some(stamp);
        }
        Ok(())
    }
    fn flush_cache(&mut self) -> Result<()> {
        if !self.cache_dirty {
            return Ok(());
        }
        event(Event::MetadataSeek).map_err(Error::io)?;
        let file = self.file.as_mut().ok_or_else(Self::invalid_snapshot)?;
        file.seek(SeekFrom::Start(self.cache_start))
            .map_err(Error::io)?;
        let mut done = 0;
        while done < self.cache_len {
            let length = io_chunk(self.cache_len - done);
            match event(Event::MetadataWrite)
                .and_then(|_| file.write(&self.buffer.data[done..done + length]))
            {
                Ok(0) => {
                    return Err(Error::io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "metadata cache zero write",
                    )));
                }
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        self.stamp = Some(
            file.metadata()
                .map_err(Error::io)?
                .modified()
                .map_err(Error::io)?,
        );
        self.cache_dirty = false;
        Ok(())
    }
    pub fn flush(&mut self) -> Result<()> {
        self.flush_append()?;
        self.flush_cache()
    }
    fn read_at(&mut self, mut at: u64, mut bytes: &mut [u8]) -> Result<()> {
        if at
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > self.length)
        {
            return Err(Self::invalid_snapshot());
        }
        while !bytes.is_empty() {
            if at < self.cache_start || at >= self.cache_start + self.cache_len as u64 {
                self.flush_cache()?;
                let start = at / (PAGE as u64) * (PAGE as u64);
                let len = (self.length - start).min(PAGE as u64) as usize;
                // Reuse the admitted append buffer after flushing; no new allocation.
                self.buffer.data.resize(PAGE, 0);
                event(Event::MetadataSeek).map_err(Error::io)?;
                let file = self.file.as_mut().ok_or_else(Self::invalid_snapshot)?;
                file.seek(SeekFrom::Start(start)).map_err(Error::io)?;
                event(Event::MetadataRead).map_err(Error::io)?;
                file.read_exact(&mut self.buffer.data[..len])
                    .map_err(Error::io)?;
                self.cache_start = start;
                self.cache_len = len;
            }
            let start = (at - self.cache_start) as usize;
            let n = bytes.len().min(self.cache_len - start);
            bytes[..n].copy_from_slice(&self.buffer.data[start..start + n]);
            at += n as u64;
            bytes = &mut bytes[n..];
        }
        Ok(())
    }
    pub fn next(&mut self, position: &mut u64) -> Result<Option<Frame>> {
        self.validate_snapshot()?;
        event(Event::MetadataRead).map_err(Error::io)?;
        if *position == self.length {
            self.read_at(*position, &mut [])?;
            return Ok(None);
        }
        let mut header = [0u8; 3];
        self.read_at(*position, &mut header)?;
        let len = usize::from(u16::from_le_bytes([header[1], header[2]]));
        if !(1..=9).contains(&header[0]) || len > FRAME {
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
        let offset = (at - self.cache_start) as usize;
        // PAGE is a multiple of 19: integer() brought this entire frame into cache.
        self.buffer
            .data
            .get_mut(offset..offset + 19)
            .ok_or_else(Self::invalid_snapshot)?
            .copy_from_slice(&b);
        self.cache_dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::write::tests::{Parent, Reset, short_chunks};
    #[test]
    fn pages_preserve_shuffled_ordinal_updates_crossing_frames_and_late_corruption() {
        let _reset = Reset::new();
        let p = Parent::new();
        let budget = Budget::new(1024 * 1024);
        let mut store = Store::create(&p.destination(), &budget).unwrap();
        let (mut used, mut peak) = (0, 0);
        for _ in 0..4096 {
            store
                .append(8, &0i64.to_le_bytes(), &mut used, &mut peak, 1024 * 1024)
                .unwrap();
        }
        short_chunks(7);
        for i in 0..4096 {
            let ordinal = (i * 37) % 4096 + 1;
            store.set_integer(ordinal, -i64::from(ordinal)).unwrap();
        }
        for i in 1..=4096 {
            assert_eq!(store.integer(i).unwrap(), -i64::from(i));
        }
        store.flush().unwrap();
        assert_eq!(store.integer(1).unwrap(), -1);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(p.destination())
            .unwrap();
        file.seek(SeekFrom::Start(3)).unwrap();
        file.write_all(&[0]).unwrap();
        drop(file);
        assert_eq!(store.integer(1).unwrap_err().kind, ErrorKind::InvalidSpill);
        drop(store);
        assert_eq!(budget.statistics().current_bytes, 0);
        fs::remove_file(p.destination()).unwrap();
        let mut store = Store::create(&p.destination(), &budget).unwrap();
        let (mut used, mut peak) = (0, 0);
        for n in 0..100u32 {
            let mut b = [b'x'; 1024];
            b[..4].copy_from_slice(&n.to_le_bytes());
            store
                .append(3, &b, &mut used, &mut peak, 1024 * 1024)
                .unwrap();
        }
        let mut at = 0;
        for n in 0..100u32 {
            let f = store.next(&mut at).unwrap().unwrap();
            assert_eq!(&f.payload()[..4], &n.to_le_bytes());
            assert_eq!(&f.payload()[4..], &[b'x'; 1020]);
        }
        assert!(store.next(&mut at).unwrap().is_none());
    }
}
