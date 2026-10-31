//! Explicit, source-verified per-symbol sidecars. No automatic query planner.
use crate::{
    budget::{Buffer, Reservation},
    records::{Decoder, DecoderPosition},
    *,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
mod wire;
use wire::*;
mod generate;
mod query;
mod store;
pub(crate) use query::Plan;
pub use query::{IndexScanMode, IndexedScanStatistics};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexPublication {
    CreateNew,
    Replace,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexPolicy {
    Off,
    UseIfValid,
}
#[derive(Clone, Copy, Debug)]
pub struct IndexOptions {
    pub record_stride: u64,
    pub decoded_byte_stride: u64,
    /// Aggregate temporary/published index extent; directory nodes count too.
    pub maximum_index_bytes: u64,
    /// Includes the already-open GDX source and one sidecar handle.
    pub maximum_open_files: usize,
    /// One page is cached in this initial implementation. Larger caps are not allocated.
    pub page_cache_bytes: usize,
    pub publication: IndexPublication,
}
impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            record_stride: 65536,
            decoded_byte_stride: 1024 * 1024,
            maximum_index_bytes: 4 * 1024 * 1024 * 1024,
            maximum_open_files: 2,
            page_cache_bytes: PAGE,
            publication: IndexPublication::CreateNew,
        }
    }
}
impl IndexOptions {
    fn validate(&self) -> Result<()> {
        if self.record_stride == 0
            || self.decoded_byte_stride == 0
            || self.maximum_index_bytes < 2 * PAGE as u64
            || self.maximum_open_files < 2
            || self.page_cache_bytes < PAGE
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid index resources or strides",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct IndexStatistics {
    pub source_bytes_hashed: u64,
    pub rows_decoded: u64,
    pub intervals: u64,
    pub directory_pages: u64,
    pub index_bytes: u64,
    pub pages_read: u64,
    pub pages_written: u64,
    pub cache_hits: u64,
    pub digest_time: Duration,
    pub replay_time: Duration,
    pub published: bool,
    pub complete: bool,
}
#[derive(Debug)]
pub enum IndexStatus {
    Off,
    Missing,
    Rejected(Error),
    Ready,
}
pub struct IndexOpen {
    pub status: IndexStatus,
    pub index: Option<CheckpointIndex>,
}
impl IndexOpen {
    fn off() -> Self {
        Self {
            status: IndexStatus::Off,
            index: None,
        }
    }
    fn missing() -> Self {
        Self {
            status: IndexStatus::Missing,
            index: None,
        }
    }
    fn rejected(error: Error) -> Self {
        Self {
            status: IndexStatus::Rejected(error),
            index: None,
        }
    }
    fn ready(index: CheckpointIndex) -> Self {
        Self {
            status: IndexStatus::Ready,
            index: Some(index),
        }
    }
}

pub struct IndexBuilder {
    reader: Reader,
    symbol: SymbolHandle,
    options: IndexOptions,
    destination: PathBuf,
    directory: PathBuf,
    data: PathBuf,
    owned_directory: bool,
    owned_data: bool,
    file: Option<File>,
    statistics: IndexStatistics,
    error: Option<Error>,
    aborted: bool,
    _state: Reservation,
}
impl Reader {
    /// Explicit build owner. No temporary files are created until finish, so
    /// initialization/build failures retain an owner for fallible abort/retry.
    pub fn index_builder(
        &self,
        symbol: &SymbolHandle,
        path: impl AsRef<Path>,
        options: IndexOptions,
    ) -> Result<IndexBuilder> {
        self.check_handle(symbol)?;
        options.validate()?;
        let state = self
            .source
            .budget
            .reserve(std::mem::size_of::<IndexBuilder>() + 64 * 1024)?;
        let path = path.as_ref();
        if path.as_os_str().len() > 4096 {
            return Err(Error::new(ErrorKind::InvalidOptions, "index path too long"));
        }
        let parent = fs::canonicalize(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .map_err(Error::io)?;
        let name = path
            .file_name()
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "index needs a file name"))?;
        let destination = parent.join(name);
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| invalid())?;
        let directory = parent.join(format!(".gdxrs-index-{}-{sequence}", std::process::id()));
        let data = directory.join("data");
        if destination.as_os_str().len() > 4096 || data.as_os_str().len() > 4096 {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "canonical index path too long",
            ));
        }
        Ok(IndexBuilder {
            reader: self.clone(),
            symbol: symbol.clone(),
            options,
            destination,
            directory,
            data,
            owned_directory: false,
            owned_data: false,
            file: None,
            statistics: IndexStatistics::default(),
            error: None,
            aborted: false,
            _state: state,
        })
    }
    /// Never builds. Strict reuse hashes the source and regenerates canonical
    /// checkpoints/summaries once, before exposing a validated session.
    pub fn open_index(
        &self,
        symbol: &SymbolHandle,
        path: impl AsRef<Path>,
        options: IndexOptions,
        policy: IndexPolicy,
    ) -> Result<IndexOpen> {
        if policy == IndexPolicy::Off {
            return Ok(IndexOpen::off());
        }
        self.check_handle(symbol)?;
        options.validate()?;
        CheckpointIndex::open(self, symbol, path.as_ref(), options)
    }
}
impl IndexBuilder {
    pub fn statistics(&self) -> IndexStatistics {
        self.statistics
    }
    fn validate_destination(&self) -> Result<()> {
        if self.options.publication == IndexPublication::Replace {
            match File::open(&self.destination) {
                Ok(mut file) => {
                    if !file.metadata().map_err(Error::io)?.is_file() {
                        return Err(invalid());
                    }
                    let mut magic = [0; 8];
                    file.read_exact(&mut magic).map_err(Error::io)?;
                    if &magic != b"GDXSIDX1" {
                        return Err(Error::new(
                            ErrorKind::InvalidOptions,
                            "replacement target is not a checkpoint sidecar",
                        ));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(e)),
            }
        }
        Ok(())
    }
    fn cleanup(&mut self) -> io::Result<()> {
        self.file = None;
        if self.owned_data {
            store::event(store::Event::Remove)?;
            fs::remove_file(&self.data)?;
            self.owned_data = false;
        }
        if self.owned_directory {
            store::event(store::Event::Remove)?;
            fs::remove_dir(&self.directory)?;
            self.owned_directory = false;
        }
        Ok(())
    }
    pub fn abort(&mut self) -> Result<()> {
        self.aborted = true;
        self.cleanup().map_err(Error::io)
    }
    pub fn finish(&mut self) -> Result<IndexStatistics> {
        self.finish_with(|_| Ok(()))
    }
    /// Called after each streamed page. Returning an error cancels publication;
    /// explicit abort retains retryable cleanup ownership if removal fails.
    pub fn finish_with<F: FnMut(IndexStatistics) -> Result<()>>(
        &mut self,
        mut progress: F,
    ) -> Result<IndexStatistics> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.aborted {
            return Err(Error::new(
                ErrorKind::InvalidWriteState,
                "index builder aborted",
            ));
        }
        if self.statistics.complete {
            return Ok(self.statistics);
        }
        let result = self.build(&mut progress);
        match result {
            Ok(()) => Ok(self.statistics),
            Err(mut error) => {
                if let Err(cleanup) = self.cleanup() {
                    error = error.with_cleanup(cleanup);
                }
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    fn build<F: FnMut(IndexStatistics) -> Result<()>>(&mut self, progress: &mut F) -> Result<()> {
        self.reader.source.check_unchanged()?;
        self.validate_destination()?;
        store::event(store::Event::CreateDirectory)?;
        fs::create_dir(&self.directory).map_err(Error::io)?;
        self.owned_directory = true;
        store::event(store::Event::Create)?;
        self.file = Some(
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&self.data)
                .map_err(Error::io)?,
        );
        self.owned_data = true;
        let file = self.file.as_mut().ok_or_else(invalid)?;
        store::write(file, &[0; PAGE])?;
        self.statistics.index_bytes = PAGE as u64;
        let time = Instant::now();
        let digest = source_digest(&self.reader)?;
        self.statistics.digest_time = time.elapsed();
        self.statistics.source_bytes_hashed = self.reader.source.length;
        let time = Instant::now();
        let reader = self.reader.clone();
        let symbol = self.symbol.clone();
        let options = self.options;
        let (root, initial, rows) =
            generate::generate(&reader, &symbol, &options, |offset, page| {
                let file = self.file.as_mut().ok_or_else(invalid)?;
                store::seek(file, offset)?;
                store::write(file, page)?;
                self.statistics.index_bytes =
                    offset.checked_add(PAGE as u64).ok_or_else(invalid)?;
                self.statistics.pages_written += 1;
                if page[8] == 1 {
                    self.statistics.rows_decoded = num(page, 40);
                    self.statistics.intervals += 1;
                } else {
                    self.statistics.directory_pages += 1;
                }
                progress(self.statistics)?;
                reader.source.check_unchanged()
            })?;
        self.statistics.replay_time = time.elapsed();
        self.statistics.rows_decoded = rows;
        let header = header(&self.reader, &initial, digest, &self.options, root)?;
        let file = self.file.as_mut().ok_or_else(invalid)?;
        store::seek(file, 0)?;
        store::write(file, &header)?;
        store::event(store::Event::Sync)?;
        file.flush().map_err(Error::io)?;
        file.sync_all().map_err(Error::io)?;
        self.file = None;
        self.reader.source.check_unchanged()?;
        self.validate_destination()?;
        store::event(store::Event::Publish)?;
        match self.options.publication {
            IndexPublication::CreateNew => fs::hard_link(&self.data, &self.destination),
            IndexPublication::Replace => fs::rename(&self.data, &self.destination),
        }
        .map_err(Error::io)?;
        self.statistics.published = true;
        if self.options.publication == IndexPublication::Replace {
            self.owned_data = false;
        }
        self.cleanup().map_err(Error::io)?;
        self.statistics.complete = true;
        Ok(())
    }
}
impl Drop for IndexBuilder {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub struct CheckpointIndex {
    reader: Reader,
    symbol: SymbolHandle,
    file: File,
    root: Root,
    cache: Buffer<u8>,
    cache_offset: Option<u64>,
    error: Option<Error>,
    statistics: IndexStatistics,
    _state: Reservation,
}
pub struct IndexInterval {
    pub start: u64,
    pub end: u64,
    pub checkpoint: RecordCheckpoint,
    minimum: [u32; 20],
    maximum: [u32; 20],
    dimension: u8,
    _state: Reservation,
}
impl IndexInterval {
    pub fn key_bounds(&self, dimension: u8) -> Option<(UelId, UelId)> {
        if dimension >= self.dimension || self.start == self.end {
            return None;
        }
        Some((
            UelId::new(self.minimum[dimension as usize]).ok()?,
            UelId::new(self.maximum[dimension as usize]).ok()?,
        ))
    }
}
impl CheckpointIndex {
    pub fn statistics(&self) -> IndexStatistics {
        self.statistics
    }
    pub fn interval_count(&self) -> u64 {
        self.root.intervals
    }
    fn open(
        reader: &Reader,
        symbol: &SymbolHandle,
        path: &Path,
        options: IndexOptions,
    ) -> Result<IndexOpen> {
        let state = reader
            .source
            .budget
            .reserve(std::mem::size_of::<Self>() + 16 * 1024)?;
        if path.as_os_str().len() > 4096 {
            return Err(Error::new(ErrorKind::InvalidOptions, "index path too long"));
        }
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(IndexOpen::missing()),
            Err(e) => return Err(Error::io(e)),
        };
        let length = file.metadata().map_err(Error::io)?.len();
        if !file.metadata().map_err(Error::io)?.is_file()
            || length < 2 * PAGE as u64
            || length > options.maximum_index_bytes
            || !length.is_multiple_of(PAGE as u64)
        {
            return Ok(IndexOpen::rejected(invalid()));
        }
        let mut session = Self {
            reader: reader.clone(),
            symbol: symbol.clone(),
            file,
            root: Root {
                entry: Entry::default(),
                depth: 0,
                intervals: 0,
                directory_pages: 0,
                bytes: length,
            },
            cache: Buffer::zeroed(&reader.source.budget, PAGE)?,
            cache_offset: None,
            error: None,
            statistics: IndexStatistics::default(),
            _state: state,
        };
        let result = session.validate(options);
        match result {
            Ok(()) => Ok(IndexOpen::ready(session)),
            Err(error)
                if error.kind == ErrorKind::InvalidIndex
                    || error.kind == ErrorKind::SourceMismatch =>
            {
                Ok(IndexOpen::rejected(error))
            }
            Err(error) => Err(error),
        }
    }
    fn read_page(&mut self, offset: u64) -> Result<Page> {
        if !offset.is_multiple_of(PAGE as u64)
            || offset
                .checked_add(PAGE as u64)
                .is_none_or(|n| n > self.root.bytes)
        {
            return Err(invalid());
        }
        if self.cache_offset == Some(offset) {
            self.statistics.cache_hits = self
                .statistics
                .cache_hits
                .checked_add(1)
                .ok_or_else(invalid)?;
            let mut page = [0; PAGE];
            page.copy_from_slice(&self.cache.data);
            return Ok(page);
        }
        self.cache_offset = None;
        store::seek(&mut self.file, offset)?;
        store::read(&mut self.file, &mut self.cache.data)?;
        self.statistics.pages_read = self
            .statistics
            .pages_read
            .checked_add(1)
            .ok_or_else(invalid)?;
        self.cache_offset = Some(offset);
        let mut page = [0; PAGE];
        page.copy_from_slice(&self.cache.data);
        Ok(page)
    }
    fn validate(&mut self, options: IndexOptions) -> Result<()> {
        let actual = self.read_page(0)?;
        if &actual[..8] != b"GDXSIDX1"
            || actual[8..12] != 1u32.to_le_bytes()
            || actual[12..16] != (PAGE as u32).to_le_bytes()
            || num(&actual, 112) != options.record_stride
            || num(&actual, 120) != options.decoded_byte_stride
            || num(&actual, 640) != self.root.bytes
        {
            return Err(invalid());
        }
        let time = Instant::now();
        let digest = source_digest(&self.reader)?;
        self.statistics.digest_time = time.elapsed();
        self.statistics.source_bytes_hashed = self.reader.source.length;
        if num(&actual, 16) != self.reader.source.length || actual[24..56] != digest {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "sidecar source digest mismatch",
            ));
        }
        let time = Instant::now();
        let reader = self.reader.clone();
        let symbol = self.symbol.clone();
        let (root, initial, rows) =
            generate::generate(&reader, &symbol, &options, |offset, page| {
                if self.read_page(offset)? != *page {
                    return Err(invalid());
                }
                Ok(())
            })?;
        if root.bytes != self.root.bytes
            || header(&reader, &initial, digest, &options, root)? != actual
        {
            return Err(invalid());
        }
        self.root = root;
        self.statistics.replay_time = time.elapsed();
        self.statistics.rows_decoded = rows;
        self.statistics.intervals = root.intervals;
        self.statistics.directory_pages = root.directory_pages;
        self.statistics.index_bytes = root.bytes;
        self.statistics.complete = true;
        Ok(())
    }
    /// Read one authenticated directory path and data page. No source digest or
    /// full-directory scan is repeated within the validated immutable session.
    pub fn interval(&mut self, index: u64) -> Result<IndexInterval> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let result = self.load_interval(index);
        result.map_err(|mut error| {
            error.source_id = Some(self.reader.source.id);
            error.symbol_id = Some(self.symbol.id().get());
            if error.kind != ErrorKind::InvalidReference && error.kind != ErrorKind::BudgetTooSmall
            {
                self.cache_offset = None;
                self.error = Some(error.clone());
            }
            error
        })
    }
    fn load_interval(&mut self, index: u64) -> Result<IndexInterval> {
        self.reader.source.check_unchanged()?;
        if index >= self.root.intervals {
            return Err(Error::new(
                ErrorKind::InvalidReference,
                "index interval outside directory",
            ));
        }
        let mut entry = self.root.entry;
        let mut depth = self.root.depth;
        loop {
            let page = self.read_page(entry.offset)?;
            verify(&page, &entry.digest)?;
            if &page[..8] != b"GDXPAGE1"
                || page[9] != depth
                || num(&page, 16) != entry.first
                || num(&page, 24) != entry.end
            {
                return Err(invalid());
            }
            if depth == 0 {
                if page[8] != 1 || entry.first != index || entry.end != index + 1 {
                    return Err(invalid());
                }
                let start = num(&page, 32);
                let end = num(&page, 40);
                if end < start || end > self.statistics.rows_decoded {
                    return Err(invalid());
                }
                let mut minimum = [0; 20];
                let mut maximum = [0; 20];
                for d in 0..20 {
                    let mut b = [0; 4];
                    b.copy_from_slice(&page[48 + 4 * d..52 + 4 * d]);
                    minimum[d] = u32::from_le_bytes(b);
                    b.copy_from_slice(&page[128 + 4 * d..132 + 4 * d]);
                    maximum[d] = u32::from_le_bytes(b);
                }
                let mut bytes = [0; 512];
                bytes.copy_from_slice(&page[208..720]);
                let position = DecoderPosition::from_index_bytes(bytes, self.reader.source.id)?;
                if position.row_ordinal() != start {
                    return Err(invalid());
                }
                let dimension = position.dimension();
                let uels = self.reader.source.counts()?.uels;
                for d in 0..20 {
                    if d < usize::from(dimension) && end > start {
                        if minimum[d] == 0 || minimum[d] > maximum[d] || maximum[d] > uels {
                            return Err(invalid());
                        }
                    } else if minimum[d] != 0 || maximum[d] != 0 {
                        return Err(invalid());
                    }
                }
                let state = self
                    .reader
                    .source
                    .budget
                    .reserve(std::mem::size_of::<IndexInterval>())?;
                let checkpoint = RecordCheckpoint::verified(
                    Arc::clone(&self.reader.source),
                    self.symbol.id(),
                    position,
                )?;
                self.reader.source.check_unchanged()?;
                return Ok(IndexInterval {
                    start,
                    end,
                    checkpoint,
                    minimum,
                    maximum,
                    dimension,
                    _state: state,
                });
            }
            if page[8] != 2 || page[10] == 0 || page[10] as usize > FAN {
                return Err(invalid());
            }
            let mut found = None;
            for n in 0..page[10] as usize {
                let child = Entry::read(&page, 32 + n * 56);
                if child.offset >= entry.offset || child.first >= child.end {
                    return Err(invalid());
                }
                if child.first <= index && index < child.end {
                    if found.is_some() {
                        return Err(invalid());
                    }
                    found = Some(child);
                }
            }
            entry = found.ok_or_else(invalid)?;
            depth -= 1;
        }
    }
}

#[cfg(test)]
mod tests;
