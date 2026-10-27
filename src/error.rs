use std::{fmt, io, sync::Arc};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    Io,
    UnexpectedEof,
    UnsupportedVersion,
    UnsupportedByteOrder,
    CompressionUnavailable,
    InvalidHeader,
    InvalidOffset,
    InvalidCount,
    InvalidDimension,
    InvalidType,
    InvalidKeyTag,
    InvalidValueTag,
    InvalidCompression,
    InvalidReference,
    InvalidAcronymId,
    AliasCycle,
    SourceMismatch,
    SourceChanged,
    InvalidOptions,
    InvalidProjection,
    InvalidSelection,
    BatchTooSmall,
    BudgetTooSmall,
    AllocationFailed,
    TemporarySpaceExceeded,
    FileLimitExceeded,
    InvalidSpill,
    InvalidWriteState,
    OutputLimitExceeded,
    DataErrorLimitExceeded,
}

#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub source_id: Option<u64>,
    pub symbol_id: Option<u32>,
    pub section: Option<&'static str>,
    pub physical_offset: Option<u64>,
    pub decoded_offset: Option<u32>,
    message: &'static str,
    cause: Option<Arc<io::Error>>,
    cleanup: Option<Arc<io::Error>>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            source_id: None,
            symbol_id: None,
            section: None,
            physical_offset: None,
            decoded_offset: None,
            cause: None,
            cleanup: None,
        }
    }
    /// Secondary cleanup failure; the original operation kind/cause stays intact.
    pub fn cleanup_error(&self) -> Option<&io::Error> {
        self.cleanup.as_deref()
    }
    pub(crate) fn with_cleanup(mut self, error: io::Error) -> Self {
        self.cleanup = Some(Arc::new(error));
        self
    }
    pub(crate) fn io(cause: io::Error) -> Self {
        let mut error = Self::new(ErrorKind::Io, "file I/O failed");
        error.cause = Some(Arc::new(cause));
        error
    }
    pub(crate) fn at(
        mut self,
        source: u64,
        section: &'static str,
        physical: u64,
        decoded: Option<u32>,
    ) -> Self {
        self.source_id = Some(source);
        self.section = Some(section);
        self.physical_offset = Some(physical);
        self.decoded_offset = decoded;
        self
    }
    pub(crate) fn symbol(mut self, id: u32) -> Self {
        self.symbol_id = Some(id);
        self
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(at) = self.physical_offset {
            write!(f, " at byte {at}")?;
        }
        if let Some(cause) = &self.cause {
            write!(f, ": {cause}")?;
        }
        if self.cleanup.is_some() {
            write!(f, " (scratch cleanup also failed)")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl From<io::Error> for Error {
    fn from(cause: io::Error) -> Self {
        Self::io(cause)
    }
}
