//! Private temporary-run sorting; not a GDX writer or persistent index.
mod run;
#[cfg(test)]
mod tests;
use crate::{
    budget::{Buffer, Reservation},
    model::Bytes,
    source::Source,
    *,
};
use run::{RunReader, RunWriter};
use std::{
    fs, io,
    mem::size_of,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const CONTROL: usize = 16 * 1024;
const IO_BYTES: usize = 32768;
#[derive(Clone, Debug)]
pub struct SpillOptions {
    pub directory: PathBuf,
    pub run_bytes: usize,
    pub maximum_temporary_bytes: u64,
    pub maximum_open_files: usize,
    pub merge_fan_in: usize,
}
impl SpillOptions {
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: directory.as_ref().to_path_buf(),
            run_bytes: 256 * 1024,
            maximum_temporary_bytes: 512 * 1024 * 1024,
            maximum_open_files: 9,
            merge_fan_in: 8,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.directory.as_os_str().len() > 4096
            || !(2..=64).contains(&self.merge_fan_in)
            || self.maximum_open_files <= self.merge_fan_in
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid scratch path or merge/handle limits",
            ));
        }
        if self.run_bytes < size_of::<Record>() {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "run buffer cannot hold one record",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpillStatistics {
    pub temporary_bytes: u64,
    pub peak_temporary_bytes: u64,
    pub open_files: u64,
    pub peak_open_files: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub input_records: u64,
    pub returned_records: u64,
    pub published_runs: u64,
    pub merge_passes: u64,
    /// True only after validated EOF and successful cleanup; cancellation is false.
    pub complete: bool,
}
struct Metrics {
    used: AtomicU64,
    peak: AtomicU64,
    handles: AtomicU64,
    peak_handles: AtomicU64,
    read: AtomicU64,
    written: AtomicU64,
    input: AtomicU64,
    returned: AtomicU64,
    runs: AtomicU64,
    passes: AtomicU64,
    complete: AtomicBool,
    disk_limit: u64,
    handle_limit: u64,
}
fn add(counter: &AtomicU64, n: u64) -> Result<()> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(n))
        .map(|_| ())
        .map_err(|_| Error::new(ErrorKind::InvalidCount, "spill counter overflow"))
}
impl Metrics {
    fn new(options: &SpillOptions) -> Result<Arc<Self>> {
        let handle_limit = u64::try_from(options.maximum_open_files)
            .map_err(|_| Error::new(ErrorKind::InvalidOptions, "handle limit overflow"))?;
        Ok(Arc::new(Self {
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            handles: AtomicU64::new(0),
            peak_handles: AtomicU64::new(0),
            read: AtomicU64::new(0),
            written: AtomicU64::new(0),
            input: AtomicU64::new(0),
            returned: AtomicU64::new(0),
            runs: AtomicU64::new(0),
            passes: AtomicU64::new(0),
            complete: AtomicBool::new(false),
            disk_limit: options.maximum_temporary_bytes,
            handle_limit,
        }))
    }
    fn snapshot(&self) -> SpillStatistics {
        SpillStatistics {
            temporary_bytes: self.used.load(Ordering::Relaxed),
            peak_temporary_bytes: self.peak.load(Ordering::Relaxed),
            open_files: self.handles.load(Ordering::Relaxed),
            peak_open_files: self.peak_handles.load(Ordering::Relaxed),
            bytes_read: self.read.load(Ordering::Relaxed),
            bytes_written: self.written.load(Ordering::Relaxed),
            input_records: self.input.load(Ordering::Relaxed),
            returned_records: self.returned.load(Ordering::Relaxed),
            published_runs: self.runs.load(Ordering::Relaxed),
            merge_passes: self.passes.load(Ordering::Relaxed),
            complete: self.complete.load(Ordering::Relaxed),
        }
    }
    fn reserve_bytes(&self, n: usize) -> Result<()> {
        let n = u64::try_from(n).map_err(|_| {
            Error::new(
                ErrorKind::TemporarySpaceExceeded,
                "spill byte layout overflow",
            )
        })?;
        let old = self
            .used
            .try_update(Ordering::AcqRel, Ordering::Relaxed, |v| {
                v.checked_add(n).filter(|v| *v <= self.disk_limit)
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::TemporarySpaceExceeded,
                    "temporary byte quota exceeded",
                )
            })?;
        self.peak.fetch_max(old + n, Ordering::Relaxed);
        Ok(())
    }
    fn handle(self: &Arc<Self>) -> Result<Handle> {
        let old = self
            .handles
            .try_update(Ordering::AcqRel, Ordering::Relaxed, |v| {
                v.checked_add(1).filter(|v| *v <= self.handle_limit)
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::FileLimitExceeded,
                    "open run handle quota exceeded",
                )
            })?;
        self.peak_handles.fetch_max(old + 1, Ordering::Relaxed);
        Ok(Handle(Arc::clone(self)))
    }
}
struct Handle(Arc<Metrics>);
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.handles.fetch_sub(1, Ordering::AcqRel);
    }
}
struct OpenRun {
    file: fs::File,
    _handle: Handle,
}
#[derive(Clone)]
struct Record {
    label: Bytes,
    id: u64,
}
impl Record {
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        self.label
            .as_slice()
            .cmp(other.label.as_slice())
            .then(self.id.cmp(&other.id))
    }
}
struct Session {
    source: Arc<Source>,
    directory: PathBuf,
    metrics: Arc<Metrics>,
    next_id: u64,
    pending: Option<(u64, bool)>,
    fan_in: usize,
    cleaned: bool,
    _paths: Reservation,
    _control: Reservation,
}
impl Session {
    fn new(source: Arc<Source>, options: &SpillOptions) -> Result<Self> {
        let control = source.budget.reserve(CONTROL)?;
        let path_bytes = options
            .directory
            .as_os_str()
            .len()
            .checked_mul(8)
            .and_then(|v| v.checked_add(64 * 1024))
            .ok_or_else(|| Error::new(ErrorKind::BudgetTooSmall, "scratch path layout overflow"))?;
        let paths = source.budget.reserve(path_bytes)?;
        let parent = fs::canonicalize(&options.directory).map_err(Error::io)?;
        let parent_metadata = fs::metadata(&parent).map_err(Error::io)?;
        if parent.as_os_str().len() > 4096
            || parent.capacity() > 16 * 1024
            || !parent_metadata.is_dir()
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "scratch parent must be an existing bounded directory",
            ));
        }
        let metrics = Metrics::new(options)?;
        for _ in 0..64 {
            let sequence = next_sequence()?;
            let directory = parent.join(format!(
                "gdxrs-spill-{:08x}-{sequence:016x}",
                std::process::id()
            ));
            if directory.capacity() > 16 * 1024 {
                return Err(Error::new(
                    ErrorKind::BudgetTooSmall,
                    "scratch path capacity exceeds allowance",
                ));
            }
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        source,
                        directory,
                        metrics,
                        next_id: 0,
                        pending: None,
                        fan_in: options.merge_fan_in,
                        cleaned: false,
                        _paths: paths,
                        _control: control,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Error::io(e)),
            }
        }
        Err(Error::io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "scratch namespace collisions",
        )))
    }
    fn path(&self, id: u64, partial: bool) -> PathBuf {
        self.directory.join(format!(
            "r-{id:016x}.{}",
            if partial { "part" } else { "run" }
        ))
    }
    fn decorate(&self, mut e: Error) -> Error {
        e.source_id = Some(self.source.id);
        if e.section.is_none() {
            e.section = Some("spill");
        }
        e
    }
    fn remove_run(&self, id: u64) -> Result<()> {
        let path = self.path(id, false);
        let bytes = fs::metadata(&path).map_err(Error::io)?.len();
        event(Event::Remove).map_err(Error::io)?;
        fs::remove_file(path).map_err(Error::io)?;
        self.metrics
            .used
            .try_update(Ordering::AcqRel, Ordering::Relaxed, |v| {
                v.checked_sub(bytes)
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::InvalidSpill,
                    "temporary byte accounting mismatch",
                )
            })?;
        Ok(())
    }
    fn cleanup(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        let mut first = None;
        for id in 0..self.next_id {
            for partial in [true, false] {
                if !partial && self.pending == Some((id, false)) {
                    continue;
                }
                let result =
                    event(Event::Remove).and_then(|_| fs::remove_file(self.path(id, partial)));
                if let Err(e) = result
                    && e.kind() != io::ErrorKind::NotFound
                    && first.is_none()
                {
                    first = Some(e);
                }
            }
        }
        let result = event(Event::Remove).and_then(|_| fs::remove_dir(&self.directory));
        if let Err(e) = result
            && e.kind() != io::ErrorKind::NotFound
            && first.is_none()
        {
            first = Some(e);
        }
        if let Some(e) = first {
            return Err(e);
        }
        self.pending = None;
        self.cleaned = true;
        self.metrics.used.store(0, Ordering::Release);
        Ok(())
    }
    fn fail(&mut self, e: Error) -> Error {
        let mut e = self.decorate(e);
        if let Err(cleanup) = self.cleanup() {
            e = e.with_cleanup(cleanup);
        }
        e
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn next_sequence() -> Result<u64> {
    #[cfg(test)]
    if let Some(id) = tests::namespace_override() {
        return Ok(id);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| Error::new(ErrorKind::InvalidCount, "scratch sequence exhausted"))
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Create,
    Read,
    Write,
    Flush,
    Publish,
    Remove,
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
fn io_chunk(length: usize, write: bool) -> usize {
    #[cfg(test)]
    return tests::io_chunk(length, write);
    #[cfg(not(test))]
    {
        let _ = write;
        length
    }
}

pub struct SpillSorter {
    records: Option<Buffer<Record>>,
    error: Option<Error>,
    cancelled: bool,
    _state: Reservation,
    session: Session,
}
impl Reader {
    /// Create an exclusive scratch sort whose buffers share this reader's quota.
    ///
    /// ```no_run
    /// use gdxrs::{Reader, OpenOptions, SpillOptions};
    /// # fn example() -> gdxrs::Result<()> {
    /// let reader = Reader::open("model.gdx", OpenOptions::default())?;
    /// let limits = SpillOptions::new(std::env::temp_dir());
    /// let mut sorter = reader.spill_sort(&limits)?;
    /// sorter.push(b"region-b", 2)?;
    /// sorter.push(b"region-a", 1)?;
    /// let mut sorted = sorter.finish()?;
    /// while let Some(entry) = sorted.next()? {
    ///     println!("{:?}: {}", entry.label, entry.id);
    /// }
    /// assert!(sorted.statistics().complete);
    /// # Ok(()) }
    /// ```
    pub fn spill_sort(&self, options: &SpillOptions) -> Result<SpillSorter> {
        options.validate()?;
        if size_of::<SpillSorter>() > CONTROL || size_of::<SpillCursor>() > CONTROL {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "spill fixed state exceeds allowance",
            ));
        }
        let state = self.source.budget.reserve(CONTROL)?;
        let records = Buffer::new(&self.source.budget, options.run_bytes / size_of::<Record>())?;
        let session = Session::new(Arc::clone(&self.source), options)?;
        Ok(SpillSorter {
            records: Some(records),
            error: None,
            cancelled: false,
            _state: state,
            session,
        })
    }
}
impl SpillSorter {
    pub fn directory(&self) -> &Path {
        &self.session.directory
    }
    pub fn statistics(&self) -> SpillStatistics {
        self.session.metrics.snapshot()
    }
    fn fail(&mut self, e: Error) -> Error {
        self.records = None;
        let e = self.session.fail(e);
        self.error = Some(e.clone());
        e
    }
    pub fn push(&mut self, label: &[u8], id: u64) -> Result<()> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.cancelled {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "sorter was cancelled",
            ));
        }
        let result = (|| {
            let label = Bytes::from_slice(label).map_err(|_| {
                Error::new(ErrorKind::InvalidOptions, "spill label exceeds 255 bytes")
            })?;
            if self
                .records
                .as_ref()
                .is_some_and(|v| v.data.len() == v.data.capacity())
            {
                self.flush()?;
            }
            let buffer = self
                .records
                .as_mut()
                .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "sorter has no run buffer"))?;
            add(&self.session.metrics.input, 1)?;
            buffer.push(Record { label, id })
        })();
        result.map_err(|e| self.fail(e))
    }
    fn flush(&mut self) -> Result<()> {
        let records = self
            .records
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "sorter has no run buffer"))?;
        if records.data.is_empty() {
            return Ok(());
        }
        records.data.sort_unstable_by(Record::compare);
        let mut writer = RunWriter::new(&mut self.session)?;
        for record in &records.data {
            writer.record(record, &self.session)?;
        }
        writer.finish(&mut self.session)?;
        records.data.clear();
        Ok(())
    }
    fn merge(&mut self) -> Result<Option<u64>> {
        self.flush()?;
        self.records = None;
        let (mut start, mut count) = (0, self.session.next_id);
        while count > 1 {
            add(&self.session.metrics.passes, 1)?;
            let output_start = self.session.next_id;
            let mut consumed = 0;
            while consumed < count {
                let number = (count - consumed).min(self.session.fan_in as u64) as usize;
                let mut slots = Buffer::new(&self.session.source.budget, number)?;
                for n in 0..number {
                    let mut reader = RunReader::open(&self.session, start + consumed + n as u64)?;
                    let head = reader.advance()?;
                    slots.push(Slot { reader, head })?;
                }
                let mut writer = RunWriter::new(&mut self.session)?;
                loop {
                    let best = slots
                        .data
                        .iter()
                        .enumerate()
                        .filter_map(|(i, s)| s.head.as_ref().map(|r| (i, r)))
                        .min_by(|a, b| a.1.compare(b.1))
                        .map(|(i, _)| i);
                    let Some(best) = best else {
                        break;
                    };
                    let slot = &mut slots.data[best];
                    let head = slot
                        .head
                        .as_ref()
                        .ok_or_else(|| Error::new(ErrorKind::InvalidSpill, "missing merge head"))?;
                    writer.record(head, &self.session)?;
                    slot.head = slot.reader.advance()?;
                }
                writer.finish(&mut self.session)?;
                drop(slots);
                for n in 0..number {
                    self.session.remove_run(start + consumed + n as u64)?;
                }
                consumed += number as u64;
            }
            start = output_start;
            count = self.session.next_id - output_start;
        }
        Ok((count != 0).then_some(start))
    }
    /// Finish all run writes/merges before returning a readable result.
    pub fn finish(mut self) -> Result<SpillCursor> {
        if let Some(e) = self.error.take() {
            return Err(self.session.fail(e));
        }
        if self.cancelled {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "sorter was cancelled",
            ));
        }
        let result = self
            .merge()
            .and_then(|id| id.map(|id| RunReader::open(&self.session, id)).transpose());
        match result {
            Ok(reader) => Ok(SpillCursor {
                reader,
                last: Record {
                    label: Bytes::default(),
                    id: 0,
                },
                done: false,
                error: None,
                _state: self._state,
                session: self.session,
            }),
            Err(e) => Err(self.fail(e)),
        }
    }
    /// Cancel and report cleanup errors. May be called again to retry cleanup.
    pub fn cancel(&mut self) -> Result<SpillStatistics> {
        self.records = None;
        self.cancelled = true;
        self.session
            .cleanup()
            .map_err(|e| self.session.decorate(Error::io(e)))?;
        Ok(self.statistics())
    }
}
struct Slot {
    reader: RunReader,
    head: Option<Record>,
}
pub struct SpillEntry<'a> {
    pub label: &'a [u8],
    pub id: u64,
}
/// One complete borrowed scratch record at a time.
///
/// ```compile_fail,E0499
/// # use gdxrs::SpillCursor;
/// # fn example(cursor: &mut SpillCursor) -> gdxrs::Result<()> {
/// let first = cursor.next()?.unwrap();
/// let second = cursor.next()?;
/// println!("{:?}", first.label);
/// # Ok(()) }
/// ```
pub struct SpillCursor {
    reader: Option<RunReader>,
    last: Record,
    done: bool,
    error: Option<Error>,
    _state: Reservation,
    session: Session,
}
impl SpillCursor {
    pub fn directory(&self) -> &Path {
        &self.session.directory
    }
    pub fn statistics(&self) -> SpillStatistics {
        self.session.metrics.snapshot()
    }
    fn fail(&mut self, e: Error) -> Error {
        self.reader = None;
        let e = self.session.fail(e);
        self.error = Some(e.clone());
        e
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<SpillEntry<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.done {
            return Ok(None);
        }
        let result = match &mut self.reader {
            Some(reader) => reader.advance(),
            None => Ok(None),
        };
        match result {
            Ok(Some(record)) => {
                if let Err(e) = add(&self.session.metrics.returned, 1) {
                    return Err(self.fail(e));
                }
                self.last = record;
                Ok(Some(SpillEntry {
                    label: self.last.label.as_slice(),
                    id: self.last.id,
                }))
            }
            Ok(None) => {
                self.reader = None;
                if self.session.metrics.returned.load(Ordering::Relaxed)
                    != self.session.metrics.input.load(Ordering::Relaxed)
                {
                    return Err(self.fail(Error::new(
                        ErrorKind::InvalidSpill,
                        "spill result count differs from accepted input",
                    )));
                }
                match self.session.cleanup() {
                    Ok(()) => {
                        self.done = true;
                        self.session.metrics.complete.store(true, Ordering::Release);
                        Ok(None)
                    }
                    Err(e) => Err(self.fail(Error::io(e))),
                }
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    /// Explicitly close/cancel a partial result; retryable if cleanup fails.
    pub fn close(&mut self) -> Result<SpillStatistics> {
        self.reader = None;
        self.done = true;
        if let Err(e) = self.session.cleanup() {
            let e = self.session.decorate(Error::io(e));
            self.error = Some(e.clone());
            return Err(e);
        }
        Ok(self.statistics())
    }
}

/// Fully verified fixed-frame IDs; private to source-bound selection evaluation.
pub(crate) struct DiskIds {
    file: Option<OpenRun>,
    count: u64,
    session: Session,
    _state: Reservation,
}
impl SpillCursor {
    pub(crate) fn into_ids(mut self) -> Result<DiskIds> {
        let result = (|| {
            if size_of::<DiskIds>() > CONTROL {
                return Err(Error::new(
                    ErrorKind::BudgetTooSmall,
                    "membership controls exceed allowance",
                ));
            }
            let mut count = 0u64;
            if let Some(reader) = &mut self.reader {
                while let Some(record) = reader.advance()? {
                    if !record.label.as_slice().is_empty()
                        || record.id == 0
                        || record.id > u64::from(self.session.source.counts()?.uels)
                    {
                        return Err(Error::new(
                            ErrorKind::InvalidSpill,
                            "invalid membership ID frame",
                        ));
                    }
                    count = count.checked_add(1).ok_or_else(|| {
                        Error::new(ErrorKind::InvalidCount, "membership count overflow")
                    })?;
                }
            }
            if count != self.session.metrics.input.load(Ordering::Relaxed) {
                return Err(Error::new(
                    ErrorKind::InvalidSpill,
                    "membership input count mismatch",
                ));
            }
            Ok(count)
        })();
        match result {
            Ok(count) => Ok(DiskIds {
                file: self.reader.take().map(RunReader::into_file),
                count,
                session: self.session,
                _state: self._state,
            }),
            Err(e) => Err(self.fail(e)),
        }
    }
}
impl DiskIds {
    pub(crate) fn directory(&self) -> &Path {
        &self.session.directory
    }
    pub(crate) fn statistics(&self) -> SpillStatistics {
        self.session.metrics.snapshot()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub(crate) fn contains(&self, id: UelId) -> Result<bool> {
        let mut low = 0;
        let mut high = self.count;
        while low < high {
            let mid = low + (high - low) / 2;
            let at = mid
                .checked_mul(10)
                .and_then(|v| v.checked_add(8))
                .ok_or_else(|| Error::new(ErrorKind::InvalidSpill, "membership offset overflow"))?;
            let file = &self
                .file
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "membership is closed"))?
                .file;
            let mut frame = [0u8; 10];
            let mut done = 0;
            while done < frame.len() {
                event(Event::Read).map_err(Error::io)?;
                let length = io_chunk(frame.len() - done, false);
                #[cfg(unix)]
                let result = {
                    use std::os::unix::fs::FileExt;
                    file.read_at(&mut frame[done..done + length], at + done as u64)
                };
                #[cfg(windows)]
                let result = {
                    use std::os::windows::fs::FileExt;
                    file.seek_read(&mut frame[done..done + length], at + done as u64)
                };
                #[cfg(not(any(unix, windows)))]
                let result: io::Result<usize> = Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "positioned membership reads unavailable",
                ));
                match result {
                    Ok(0) => {
                        return Err(Error::new(
                            ErrorKind::UnexpectedEof,
                            "truncated membership frame",
                        ));
                    }
                    Ok(n) => {
                        add(&self.session.metrics.read, n as u64)?;
                        done += n;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(Error::io(e)),
                }
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&frame[2..]);
            let found = u64::from_le_bytes(bytes);
            if frame[..2] != [0, 0] || found == 0 || found > i32::MAX as u64 {
                return Err(Error::new(
                    ErrorKind::InvalidSpill,
                    "invalid membership frame",
                ));
            }
            match found.cmp(&u64::from(id.get())) {
                std::cmp::Ordering::Equal => return Ok(true),
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
            }
        }
        Ok(false)
    }
    pub(crate) fn close(&mut self) -> Result<()> {
        self.file = None;
        self.session.cleanup().map_err(Error::io)
    }
}
impl SpillCursor {
    pub(crate) fn within_limits(&self, limits: &SpillOptions) -> bool {
        self.session.metrics.disk_limit == limits.maximum_temporary_bytes
            && self.session.metrics.handle_limit == limits.maximum_open_files as u64
            && self.session.fan_in == limits.merge_fan_in
    }
}
