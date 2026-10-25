use crate::{
    Error, ErrorKind, OpenOptions, Result,
    budget::{Budget, Reservation},
    format::Header,
};
use std::{
    fs::File,
    io,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

pub(crate) trait ReadAt: Send + Sync {
    fn read_at(&self, at: u64, output: &mut [u8]) -> io::Result<usize>;
    fn unchanged(&self) -> io::Result<bool>;
}
struct FileSource {
    file: File,
    len: u64,
    modified: Option<SystemTime>,
}
impl ReadAt for FileSource {
    fn read_at(&self, at: u64, output: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            std::os::unix::fs::FileExt::read_at(&self.file, output, at)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::FileExt::seek_read(&self.file, output, at)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (at, output);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "positioned reads unavailable",
            ))
        }
    }
    fn unchanged(&self) -> io::Result<bool> {
        let now = self.file.metadata()?;
        Ok(now.len() == self.len && now.modified().ok() == self.modified)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct IoStatistics {
    pub read_calls: u64,
    pub bytes_read: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct Counts {
    pub symbols: u32,
    pub uels: u32,
}
pub(crate) struct Source {
    backend: Box<dyn ReadAt>,
    pub length: u64,
    pub id: u64,
    pub budget: Arc<Budget>,
    pub options: OpenOptions,
    pub header: OnceLock<Header>,
    pub counts: OnceLock<Counts>,
    calls: AtomicU64,
    bytes: AtomicU64,
    _root: Reservation,
}
impl Source {
    pub fn open(path: &Path, options: OpenOptions) -> Result<Arc<Self>> {
        options.validate()?;
        let budget = Budget::new(options.memory_budget_bytes);
        let root = budget.reserve(16 * 1024)?;
        let scratch = path
            .as_os_str()
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(32 * 1024))
            .ok_or_else(|| {
                Error::new(ErrorKind::BudgetTooSmall, "path conversion layout overflow")
            })?;
        let temporary = budget.reserve(scratch)?;
        let file = File::open(path).map_err(Error::io)?;
        let meta = file.metadata().map_err(Error::io)?;
        if !meta.is_file() {
            return Err(Error::new(
                ErrorKind::InvalidHeader,
                "source must be a regular file",
            ));
        }
        let length = meta.len();
        let backend = FileSource {
            file,
            len: length,
            modified: meta.modified().ok(),
        };
        drop(temporary);
        Self::new(Box::new(backend), length, options, budget, root)
    }
    fn new(
        backend: Box<dyn ReadAt>,
        length: u64,
        options: OpenOptions,
        budget: Arc<Budget>,
        root: Reservation,
    ) -> Result<Arc<Self>> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| {
                Error::new(ErrorKind::InvalidOptions, "source identity space exhausted")
            })?;
        Ok(Arc::new(Self {
            backend,
            length,
            id,
            budget,
            options,
            header: OnceLock::new(),
            counts: OnceLock::new(),
            calls: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            _root: root,
        }))
    }
    pub fn check_unchanged(&self) -> Result<()> {
        let error = |e: Error| e.at(self.id, "physical source", 0, None);
        if !self.backend.unchanged().map_err(|e| error(Error::io(e)))? {
            return Err(error(Error::new(
                ErrorKind::SourceChanged,
                "file changed after open",
            )));
        }
        Ok(())
    }
    pub fn read_exact(&self, mut at: u64, mut out: &mut [u8]) -> Result<()> {
        let error = |e: Error, offset| e.at(self.id, "physical source", offset, None);
        if !self
            .backend
            .unchanged()
            .map_err(|e| error(Error::io(e), at))?
        {
            return Err(error(
                Error::new(ErrorKind::SourceChanged, "file changed after open"),
                at,
            ));
        }
        let end = at
            .checked_add(
                u64::try_from(out.len())
                    .map_err(|_| Error::new(ErrorKind::InvalidOffset, "read size overflow"))?,
            )
            .ok_or_else(|| {
                error(
                    Error::new(ErrorKind::InvalidOffset, "read offset overflow"),
                    at,
                )
            })?;
        if end > self.length {
            return Err(error(
                Error::new(ErrorKind::UnexpectedEof, "read exceeds source length"),
                at,
            ));
        }
        while !out.is_empty() {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.backend.read_at(at, out) {
                Ok(0) => {
                    return Err(error(
                        Error::new(ErrorKind::UnexpectedEof, "short source read"),
                        at,
                    ));
                }
                Ok(n) if n <= out.len() => {
                    self.bytes.fetch_add(n as u64, Ordering::Relaxed);
                    at += n as u64;
                    out = &mut out[n..];
                }
                Ok(_) => {
                    return Err(error(
                        Error::new(ErrorKind::Io, "backend returned invalid read length"),
                        at,
                    ));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(error(Error::io(e), at)),
            }
        }
        Ok(())
    }
    pub fn header(&self) -> Result<&Header> {
        self.header
            .get()
            .ok_or_else(|| Error::new(ErrorKind::InvalidHeader, "header not initialized"))
    }
    pub fn counts(&self) -> Result<Counts> {
        self.counts
            .get()
            .copied()
            .ok_or_else(|| Error::new(ErrorKind::InvalidHeader, "counts not initialized"))
    }
    pub fn statistics(&self) -> IoStatistics {
        IoStatistics {
            read_calls: self.calls.load(Ordering::Relaxed),
            bytes_read: self.bytes.load(Ordering::Relaxed),
        }
    }
    #[cfg(test)]
    pub fn testing(backend: Box<dyn ReadAt>, length: u64) -> Arc<Self> {
        let options = OpenOptions::default();
        let budget = Budget::new(options.memory_budget_bytes);
        let root = budget.reserve(16384).unwrap();
        Self::new(backend, length, options, budget, root).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Short;
    impl ReadAt for Short {
        fn read_at(&self, at: u64, b: &mut [u8]) -> io::Result<usize> {
            if at == 3 {
                return Err(io::Error::other("injected"));
            }
            b[0] = at as u8;
            Ok(1)
        }
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    #[test]
    fn short_reads_and_io_causes_survive() {
        let source = Source::testing(Box::new(Short), 5);
        let mut b = [0; 3];
        source.read_exact(0, &mut b).unwrap();
        assert_eq!(b, [0, 1, 2]);
        assert_eq!(source.statistics().read_calls, 3);
        let error = source.read_exact(3, &mut b[..1]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Io);
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(
            source.read_exact(u64::MAX, &mut b).unwrap_err().kind,
            ErrorKind::InvalidOffset
        );
    }

    #[test]
    fn file_changes_and_options_fail_before_unbounded_work() {
        let path = std::path::Path::new(".work/m1-source-change-test.gdx");
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        std::io::Write::write_all(&mut file, b"small file").unwrap();
        let source = Source::open(path, OpenOptions::default()).unwrap();
        file.set_len(1).unwrap();
        assert_eq!(
            source.read_exact(0, &mut [0u8; 1]).unwrap_err().kind,
            ErrorKind::SourceChanged
        );
        drop(source);
        drop(file);
        std::fs::remove_file(path).unwrap();
        let options = OpenOptions {
            memory_budget_bytes: 0,
            ..OpenOptions::default()
        };
        assert_eq!(
            Source::open(std::path::Path::new("missing-file"), options)
                .err()
                .unwrap()
                .kind,
            ErrorKind::BudgetTooSmall
        );
        assert!(std::mem::size_of::<Source>() < 16 * 1024);
    }
}
