//! Bounded v7 metadata registration and sorted raw record creation.
mod encoder;
mod metadata;
mod records;
mod store;
use encoder::Encoder;
pub use encoder::WriteCompression;
use metadata::Metadata;
pub use metadata::{
    MetadataOptions, MetadataStatistics, SymbolReference, WriteDomain, WriteSymbol, WriterSymbolId,
    WriterUelId,
};
pub use records::{KeyBounds, SymbolWriter};
#[cfg(test)]
mod codec_tests;
#[cfg(test)]
mod tests;
use crate::{
    budget::{Budget, Buffer, Reservation},
    *,
};
use std::{
    fs,
    io::{self, Seek, SeekFrom, Write},
    mem::size_of,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
const CONTROL: usize = 16 * 1024;
const OUTPUT_BUFFER: usize = 32768;
const EMPTY_BYTES: u64 = 197;

#[derive(Clone, Debug)]
pub struct WriteOptions {
    pub memory_budget_bytes: usize,
    /// Highest admitted output extent, including pending buffered bytes.
    pub maximum_output_bytes: u64,
    pub compression: WriteCompression,
}
impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            memory_budget_bytes: 64 * 1024 * 1024,
            maximum_output_bytes: i64::MAX as u64,
            compression: WriteCompression::None,
        }
    }
}
impl WriteOptions {
    fn validate(&self) -> Result<()> {
        self.compression.validate()?;
        if self.memory_budget_bytes < 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "writer root budget below one MiB",
            ));
        }
        if self.maximum_output_bytes > i64::MAX as u64 {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "output cap exceeds v7 offset range",
            ));
        }
        if self.maximum_output_bytes < EMPTY_BYTES {
            return Err(Error::new(
                ErrorKind::OutputLimitExceeded,
                "output cap cannot hold empty v7 file",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteStatus {
    Open,
    Failed,
    Aborted,
    Complete,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteStatistics {
    pub status: WriteStatus,
    pub output_bytes: u64,
    /// Actual bytes written, including major-index backpatches.
    pub bytes_written: u64,
    /// May be true on Failed if publication succeeded but temp cleanup failed.
    pub published: bool,
}
pub struct Writer {
    owner: u64,
    symbol_active: bool,
    metadata: Option<Metadata>,
    metadata_statistics: MetadataStatistics,
    encoder: Option<Encoder>,
    compression: WriteCompression,
    buffer: Option<Buffer<u8>>,
    file: Option<fs::File>,
    destination: PathBuf,
    temporary: PathBuf,
    owned: bool,
    budget: Arc<Budget>,
    limit: u64,
    position: u64,
    physical: u64,
    index: u64,
    statistics: WriteStatistics,
    error: Option<Error>,
    _paths: Reservation,
    _control: Reservation,
}
impl Writer {
    /// Create an exclusive v7 output transaction, with optional zlib framing.
    ///
    /// ```no_run
    /// use gdxrs::{Writer, WriteOptions, WriteStatus};
    /// # fn example() -> gdxrs::Result<()> {
    /// let mut writer = Writer::create("new.gdx", WriteOptions::default())?;
    /// let result = writer.finish()?;
    /// assert_eq!(result.status, WriteStatus::Complete);
    /// # Ok(()) }
    /// ```
    pub fn create(path: impl AsRef<Path>, options: WriteOptions) -> Result<Self> {
        options.validate()?;
        Self::with_budget(
            path.as_ref(),
            &options,
            Budget::new(options.memory_budget_bytes),
        )
    }
    fn with_budget(path: &Path, options: &WriteOptions, budget: Arc<Budget>) -> Result<Self> {
        options.validate()?;
        if size_of::<Self>() > CONTROL {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "writer controls exceed allowance",
            ));
        }
        let length = path.as_os_str().len();
        if length > 4096 {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "writer path exceeds bounded length",
            ));
        }
        let filename = path
            .file_name()
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "destination must name a file"))?;
        let control = budget.reserve(CONTROL)?;
        let paths = budget.reserve(
            length
                .checked_mul(8)
                .and_then(|n| n.checked_add(64 * 1024))
                .ok_or_else(|| {
                    Error::new(ErrorKind::BudgetTooSmall, "writer path layout overflow")
                })?,
        )?;
        let owner = next_owner()?;
        let buffer = Buffer::new(&budget, OUTPUT_BUFFER)?;
        let encoder = Encoder::new(&budget, options.compression)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent).map_err(Error::io)?;
        let metadata = fs::metadata(&parent).map_err(Error::io)?;
        if !metadata.is_dir() || parent.as_os_str().len() > 4096 || parent.capacity() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "destination parent must be an existing bounded directory",
            ));
        }
        let destination = parent.join(filename);
        if destination.as_os_str().len() > 4096 || destination.capacity() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "canonical destination exceeds path allowance",
            ));
        }
        match fs::symlink_metadata(&destination) {
            Ok(_) => {
                return Err(Error::io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination already exists",
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(e)),
        }
        for _ in 0..64 {
            let sequence = next_sequence()?;
            let temporary = parent.join(format!(
                ".gdxrs-write-{:08x}-{sequence:016x}.part",
                std::process::id()
            ));
            if temporary.as_os_str().len() > 4096 || temporary.capacity() > 16 * 1024 {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "temporary output path exceeds allowance",
                ));
            }
            let mut open = fs::OpenOptions::new();
            open.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                open.mode(0o600);
            }
            event(Event::Create).map_err(Error::io)?;
            match open.open(&temporary) {
                Ok(file) => {
                    let mut writer = Self {
                        owner,
                        symbol_active: false,
                        metadata: None,
                        metadata_statistics: MetadataStatistics::default(),
                        encoder: Some(encoder),
                        compression: options.compression,
                        buffer: Some(buffer),
                        file: Some(file),
                        destination,
                        temporary,
                        owned: true,
                        budget,
                        limit: options.maximum_output_bytes,
                        position: 0,
                        physical: 0,
                        index: 0,
                        statistics: WriteStatistics {
                            status: WriteStatus::Open,
                            output_bytes: 0,
                            bytes_written: 0,
                            published: false,
                        },
                        error: None,
                        _paths: paths,
                        _control: control,
                    };
                    if let Err(e) = writer.header() {
                        return Err(writer.fail(e));
                    }
                    return Ok(writer);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        Err(Error::io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary output namespace collisions",
        )))
    }
    pub fn statistics(&self) -> WriteStatistics {
        self.statistics
    }
    pub fn budget_statistics(&self) -> BudgetStatistics {
        self.budget.statistics()
    }
    pub fn temporary_path(&self) -> &Path {
        &self.temporary
    }
    fn decorate(&self, mut error: Error) -> Error {
        if error.section.is_none() {
            error.section = Some("writer output");
        }
        if error.physical_offset.is_none() {
            error.physical_offset = Some(self.physical);
        }
        error
    }
    fn cleanup(&mut self) -> io::Result<()> {
        self.file = None;
        self.buffer = None;
        self.encoder = None;
        let mut secondary = None;
        if let Some(m) = &mut self.metadata {
            if let Err(e) = m.cleanup() {
                secondary = Some(e);
            }
            self.metadata_statistics = m.statistics();
        }
        if secondary.is_none() {
            self.metadata = None;
        }
        if self.owned {
            match event(Event::Remove).and_then(|_| fs::remove_file(&self.temporary)) {
                Ok(()) => self.owned = false,
                Err(e) if e.kind() == io::ErrorKind::NotFound => self.owned = false,
                Err(e) => {
                    if secondary.is_none() {
                        secondary = Some(e);
                    }
                }
            }
        }
        if let Some(e) = secondary {
            Err(e)
        } else {
            Ok(())
        }
    }
    fn fail(&mut self, error: Error) -> Error {
        self.statistics.status = WriteStatus::Failed;
        let mut error = self.decorate(error);
        if let Err(cleanup) = self.cleanup() {
            error = error.with_cleanup(cleanup);
        }
        self.error = Some(error.clone());
        error
    }
    fn bytes(&mut self, mut bytes: &[u8]) -> Result<()> {
        let end = self
            .position
            .checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::OutputLimitExceeded,
                    "writer output limit exceeded",
                )
            })?;
        self.statistics.output_bytes = self.statistics.output_bytes.max(end);
        while !bytes.is_empty() {
            let buffer = self.buffer.as_mut().ok_or_else(|| {
                Error::new(ErrorKind::InvalidWriteState, "writer buffer is closed")
            })?;
            if buffer.data.len() == buffer.data.capacity() {
                self.flush()?;
                continue;
            }
            let count = (buffer.data.capacity() - buffer.data.len()).min(bytes.len());
            buffer.data.extend_from_slice(&bytes[..count]);
            self.position += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }
    fn string(&mut self, bytes: &[u8]) -> Result<()> {
        let length = u8::try_from(bytes.len()).map_err(|_| {
            Error::new(ErrorKind::InvalidOptions, "writer string exceeds 255 bytes")
        })?;
        self.bytes(&[length])?;
        self.bytes(bytes)
    }
    fn flush(&mut self) -> Result<()> {
        let buffer = self
            .buffer
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "writer buffer is closed"))?;
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "writer file is closed"))?;
        let mut done = 0;
        while done < buffer.data.len() {
            let length = io_chunk(buffer.data.len() - done);
            let result =
                event(Event::Write).and_then(|_| file.write(&buffer.data[done..done + length]));
            match result {
                Ok(0) => {
                    return Err(Error::io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "writer returned zero",
                    )));
                }
                Ok(n) => {
                    self.physical = self.physical.checked_add(n as u64).ok_or_else(|| {
                        Error::new(
                            ErrorKind::InvalidOffset,
                            "writer physical position overflow",
                        )
                    })?;
                    self.statistics.bytes_written = self
                        .statistics
                        .bytes_written
                        .checked_add(n as u64)
                        .ok_or_else(|| {
                            Error::new(ErrorKind::InvalidCount, "writer byte counter overflow")
                        })?;
                    done += n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        buffer.data.clear();
        Ok(())
    }
    fn seek(&mut self, position: u64) -> Result<()> {
        if position > self.statistics.output_bytes {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                "writer patch position outside admitted extent",
            ));
        }
        self.flush()?;
        event(Event::Seek).map_err(Error::io)?;
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "writer file is closed"))?;
        file.seek(SeekFrom::Start(position)).map_err(Error::io)?;
        self.physical = position;
        self.position = position;
        Ok(())
    }
    fn header(&mut self) -> Result<()> {
        self.bytes(&[
            2, 0x34, 0x12, 4, 0x78, 0x56, 0x34, 0x12, 8, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 9,
            0x40, 123,
        ])?;
        self.string(b"GAMSGDX")?;
        self.bytes(&7i32.to_le_bytes())?;
        self.bytes(&i32::from(self.compression == WriteCompression::Zlib).to_le_bytes())?;
        self.string(b"gdxrs")?;
        self.string(b"gdxrs")?;
        self.index = self.position;
        self.bytes(&[0; 80])
    }
    fn encoded(&mut self, f: impl FnOnce(&mut Encoder, &mut Self) -> Result<()>) -> Result<()> {
        let mut encoder = self
            .encoder
            .take()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "section encoder is closed"))?;
        let result = f(&mut encoder, self);
        self.encoder = Some(encoder);
        result
    }
    fn section_string(&mut self, value: &[u8]) -> Result<()> {
        self.encoded(|e, w| e.string(value, |b| w.bytes(b)))
    }
    fn section_i32(&mut self, value: i32) -> Result<()> {
        self.encoded(|e, w| e.i32(value, |b| w.bytes(b)))
    }
    fn section_end(&mut self) -> Result<()> {
        self.encoded(|e, w| e.end(|b| w.bytes(b)))
    }
    fn finalize(&mut self) -> Result<()> {
        if self.symbol_active {
            return Err(Error::new(
                ErrorKind::InvalidWriteState,
                "symbol is unfinished",
            ));
        }
        let offsets = if let Some(mut metadata) = self.metadata.take() {
            let result = metadata.sections(self);
            self.metadata_statistics = metadata.statistics();
            self.metadata = Some(metadata);
            result?
        } else {
            let symbols = self.position;
            self.section_string(b"_SYMB_")?;
            self.section_i32(0)?;
            self.section_string(b"_SYMB_")?;
            self.section_end()?;
            let texts = self.position;
            self.section_string(b"_SETT_")?;
            self.section_i32(1)?;
            self.section_string(b"")?;
            self.section_string(b"_SETT_")?;
            self.section_end()?;
            let uels = self.position;
            self.section_string(b"_UEL_")?;
            self.section_i32(0)?;
            self.section_string(b"_UEL_")?;
            self.section_end()?;
            let acronyms = self.position;
            self.section_string(b"_ACRO_")?;
            self.section_i32(0)?;
            self.section_string(b"_ACRO_")?;
            self.section_end()?;
            [symbols, uels, texts, acronyms, symbols, 0]
        };
        let mut patch = [0u8; 52];
        patch[..4].copy_from_slice(&19510624i32.to_le_bytes());
        for (n, position) in offsets.iter().enumerate() {
            let start = 4 + n * 8;
            patch[start..start + 8].copy_from_slice(&position.to_le_bytes());
        }
        self.seek(self.index)?;
        self.bytes(&patch)?;
        self.flush()?;
        event(Event::Sync).map_err(Error::io)?;
        self.file
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "writer file is closed"))?
            .sync_all()
            .map_err(Error::io)?;
        self.file = None;
        event(Event::Publish).map_err(Error::io)?;
        fs::hard_link(&self.temporary, &self.destination).map_err(Error::io)?;
        self.statistics.published = true;
        self.cleanup().map_err(Error::io)?;
        self.statistics.status = WriteStatus::Complete;
        Ok(())
    }
    /// Finalize, sync, close and publish without overwriting. Errors are terminal.
    pub fn finish(&mut self) -> Result<WriteStatistics> {
        match self.statistics.status {
            WriteStatus::Complete => return Ok(self.statistics),
            WriteStatus::Failed => {
                return Err(self
                    .error
                    .clone()
                    .unwrap_or_else(|| Error::new(ErrorKind::InvalidWriteState, "failed writer")));
            }
            WriteStatus::Aborted => {
                return Err(Error::new(
                    ErrorKind::InvalidWriteState,
                    "writer was aborted",
                ));
            }
            WriteStatus::Open => {}
        }
        match self.finalize() {
            Ok(()) => Ok(self.statistics),
            Err(e) => Err(self.fail(e)),
        }
    }
    /// Close and remove owned temporary output; retryable after cleanup failure.
    pub fn abort(&mut self) -> Result<WriteStatistics> {
        if self.statistics.status == WriteStatus::Complete {
            return Ok(self.statistics);
        }
        if self.statistics.status != WriteStatus::Failed {
            self.statistics.status = WriteStatus::Aborted;
        }
        self.cleanup().map_err(|e| self.decorate(Error::io(e)))?;
        Ok(self.statistics)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
impl Reader {
    pub fn write_options(&self) -> WriteOptions {
        WriteOptions {
            memory_budget_bytes: self.source.budget.statistics().limit_bytes,
            ..WriteOptions::default()
        }
    }
    pub fn create_writer(&self, path: impl AsRef<Path>, options: WriteOptions) -> Result<Writer> {
        options.validate()?;
        if options.memory_budget_bytes != self.source.budget.statistics().limit_bytes {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "shared writer options must match reader root cap",
            ));
        }
        Writer::with_budget(path.as_ref(), &options, Arc::clone(&self.source.budget))
    }
}
fn next_sequence() -> Result<u64> {
    #[cfg(test)]
    if let Some(n) = tests::sequence_override() {
        return Ok(n);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| {
            Error::new(
                ErrorKind::InvalidCount,
                "writer namespace sequence exhausted",
            )
        })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Create,
    Write,
    Seek,
    Sync,
    Publish,
    Remove,
    MetadataCreate,
    MetadataRead,
    MetadataWrite,
    MetadataSeek,
    MetadataSync,
    MetadataRemove,
}
fn event(kind: Event) -> io::Result<()> {
    #[cfg(test)]
    return tests::event(kind);
    #[cfg(not(test))]
    {
        let _ = kind;
        Ok(())
    }
}
fn io_chunk(length: usize) -> usize {
    #[cfg(test)]
    return tests::io_chunk(length);
    #[cfg(not(test))]
    length
}

fn next_owner() -> Result<u64> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .map_err(|_| Error::new(ErrorKind::InvalidCount, "writer identity exhausted"))
}
