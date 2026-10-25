//! Independent, bounded-memory GDX v7 reader.

#![forbid(unsafe_code)]

mod budget;
mod error;
mod format;
mod metadata;
mod model;
mod reader;
mod records;
mod scan;
mod selection;
mod source;
mod stream;
pub use budget::BudgetStatistics;
pub use error::{Error, ErrorKind, Result};
pub use metadata::{
    AcronymCursor, AcronymView, CommentCursor, CommentView, DomainCursor, DomainMode, DomainView,
    SetTextCursor, SetTextView, SymbolCursor, SymbolHandle, SymbolMetadata, UelCursor, UelEntry,
};
pub use model::{
    AcronymId, KeyProjection, OpenOptions, ScanOptions, SpecialValue, SymbolId, SymbolType, UelId,
    Value, ValueField, ValueProjection,
};
pub use reader::{FileInformation, Reader};
pub use source::IoStatistics;

pub use scan::{BatchView, RecordView, ScanCursor, ScanStatistics};
pub use selection::{DimensionSelection, KeySelection, Selection, UelRange};
