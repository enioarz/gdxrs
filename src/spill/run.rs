use super::*;
use std::io::{Read, Write};
const MAGIC: &[u8; 8] = b"GDXRUN01";
const END: &[u8; 8] = b"GDXEND01";
const INITIAL_HASH: u64 = 14695981039346656037;
fn hash(mut state: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        state = (state ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    state
}

pub(super) struct RunWriter {
    file: OpenRun,
    buffer: Buffer<u8>,
    id: u64,
    count: u64,
    hash: u64,
}
impl RunWriter {
    pub fn new(session: &mut Session) -> Result<Self> {
        let buffer = Buffer::new(&session.source.budget, IO_BYTES)?;
        let id = session.next_id;
        let next = id
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "run ID overflow"))?;
        let handle = session.metrics.handle()?;
        event(Event::Create).map_err(Error::io)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(session.path(id, true)).map_err(Error::io)?;
        session.next_id = next;
        session.pending = Some((id, false));
        let mut writer = Self {
            file: OpenRun {
                file,
                _handle: handle,
            },
            buffer,
            id,
            count: 0,
            hash: INITIAL_HASH,
        };
        writer.write(MAGIC, session)?;
        Ok(writer)
    }
    fn flush(&mut self, session: &Session) -> Result<()> {
        let mut rest = self.buffer.data.as_slice();
        while !rest.is_empty() {
            let n = io_chunk(rest.len(), true);
            match event(Event::Write).and_then(|_| self.file.file.write(&rest[..n])) {
                Ok(0) => {
                    return Err(Error::io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "spill write returned zero",
                    )));
                }
                Ok(n) => {
                    add(&session.metrics.written, n as u64)?;
                    rest = &rest[n..];
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        self.buffer.data.clear();
        Ok(())
    }
    fn write(&mut self, mut bytes: &[u8], session: &Session) -> Result<()> {
        session.metrics.reserve_bytes(bytes.len())?;
        while !bytes.is_empty() {
            if self.buffer.data.len() == self.buffer.data.capacity() {
                self.flush(session)?;
            }
            let count = (self.buffer.data.capacity() - self.buffer.data.len()).min(bytes.len());
            // Exact helper capacity is reserved; this extension cannot grow.
            self.buffer.data.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
        }
        Ok(())
    }
    pub fn record(&mut self, record: &Record, session: &Session) -> Result<()> {
        let label = record.label.as_slice();
        let length = label.len();
        let mut frame = [0u8; 265];
        frame[..2].copy_from_slice(&(length as u16).to_le_bytes());
        frame[2..10].copy_from_slice(&record.id.to_le_bytes());
        frame[10..10 + length].copy_from_slice(label);
        self.write(&frame[..10 + length], session)?;
        self.hash = hash(self.hash, &frame[..10 + length]);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "run record count overflow"))?;
        Ok(())
    }
    pub fn finish(mut self, session: &mut Session) -> Result<()> {
        let mut footer = [0u8; 26];
        footer[..2].copy_from_slice(&u16::MAX.to_le_bytes());
        footer[2..10].copy_from_slice(&self.count.to_le_bytes());
        footer[10..18].copy_from_slice(&self.hash.to_le_bytes());
        footer[18..].copy_from_slice(END);
        self.write(&footer, session)?;
        self.flush(session)?;
        event(Event::Flush).map_err(Error::io)?;
        self.file.file.sync_all().map_err(Error::io)?;
        let id = self.id;
        drop(self);
        event(Event::Publish).map_err(Error::io)?;
        fs::hard_link(session.path(id, true), session.path(id, false)).map_err(Error::io)?;
        session.pending = Some((id, true));
        event(Event::Remove).map_err(Error::io)?;
        fs::remove_file(session.path(id, true)).map_err(Error::io)?;
        session.pending = None;
        add(&session.metrics.runs, 1)?;
        Ok(())
    }
}

pub(super) struct RunReader {
    file: OpenRun,
    buffer: Buffer<u8>,
    index: usize,
    valid: usize,
    position: u64,
    length: u64,
    count: u64,
    hash: u64,
    previous: Option<Record>,
    done: bool,
    metrics: Arc<Metrics>,
    source: u64,
}
impl RunReader {
    pub fn open(session: &Session, id: u64) -> Result<Self> {
        let buffer = Buffer::zeroed(&session.source.budget, IO_BYTES)?;
        let handle = session.metrics.handle()?;
        event(Event::Create).map_err(Error::io)?;
        let file = fs::File::open(session.path(id, false)).map_err(Error::io)?;
        let length = file.metadata().map_err(Error::io)?.len();
        let mut reader = Self {
            file: OpenRun {
                file,
                _handle: handle,
            },
            buffer,
            index: 0,
            valid: 0,
            position: 0,
            length,
            count: 0,
            hash: INITIAL_HASH,
            previous: None,
            done: false,
            metrics: Arc::clone(&session.metrics),
            source: session.source.id,
        };
        if reader.array::<8>()? != *MAGIC {
            return Err(reader.error(ErrorKind::InvalidSpill, "invalid spill header"));
        }
        Ok(reader)
    }
    fn error(&self, kind: ErrorKind, message: &'static str) -> Error {
        Error::new(kind, message).at(self.source, "spill run", self.position, None)
    }
    fn read(&mut self, mut bytes: &mut [u8]) -> Result<()> {
        while !bytes.is_empty() {
            if self.index == self.valid {
                let n = io_chunk(self.buffer.data.len(), false);
                let got = loop {
                    match event(Event::Read)
                        .and_then(|_| self.file.file.read(&mut self.buffer.data[..n]))
                    {
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        other => break other,
                    }
                }
                .map_err(|e| Error::io(e).at(self.source, "spill run", self.position, None))?;
                if got == 0 {
                    return Err(self.error(ErrorKind::UnexpectedEof, "truncated spill run"));
                }
                add(&self.metrics.read, got as u64)?;
                self.index = 0;
                self.valid = got;
            }
            let n = bytes.len().min(self.valid - self.index);
            bytes[..n].copy_from_slice(&self.buffer.data[self.index..self.index + n]);
            self.index += n;
            self.position = self
                .position
                .checked_add(n as u64)
                .ok_or_else(|| self.error(ErrorKind::InvalidOffset, "spill position overflow"))?;
            bytes = &mut bytes[n..];
        }
        Ok(())
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut bytes = [0u8; N];
        self.read(&mut bytes)?;
        Ok(bytes)
    }
    pub fn advance(&mut self) -> Result<Option<Record>> {
        if self.done {
            return Ok(None);
        }
        let length = self.array::<2>()?;
        let length_value = u16::from_le_bytes(length);
        if length_value == u16::MAX {
            let count = u64::from_le_bytes(self.array()?);
            let digest = u64::from_le_bytes(self.array()?);
            let end = self.array::<8>()?;
            if count != self.count
                || digest != self.hash
                || end != *END
                || self.position != self.length
            {
                return Err(self.error(
                    ErrorKind::InvalidSpill,
                    "spill footer count checksum or length mismatch",
                ));
            }
            self.done = true;
            return Ok(None);
        }
        if length_value > 255 {
            return Err(self.error(ErrorKind::InvalidSpill, "invalid spill label length"));
        }
        let id = self.array::<8>()?;
        let mut bytes = [0u8; 255];
        let n = usize::from(length_value);
        self.read(&mut bytes[..n])?;
        self.hash = hash(hash(hash(self.hash, &length), &id), &bytes[..n]);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| self.error(ErrorKind::InvalidCount, "spill count overflow"))?;
        let record = Record {
            label: Bytes::from_slice(&bytes[..n])?,
            id: u64::from_le_bytes(id),
        };
        if self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.compare(&record).is_gt())
        {
            return Err(self.error(ErrorKind::InvalidSpill, "spill run is not sorted"));
        }
        self.previous = Some(record.clone());
        Ok(Some(record))
    }
}
