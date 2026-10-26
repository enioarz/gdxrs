//! Independent, bounded-memory GDX v7 reader.

#![forbid(unsafe_code)]

mod budget;
mod error;
mod format;
mod labels;
mod large;
mod metadata;
mod model;
mod reader;
mod records;
mod scan;
mod selection;
mod source;
mod spill;
mod stream;
mod write;
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

pub use labels::{
    LabelLookupStatistics, LabelMatches, ResolvedLabel, SMALL_LABEL_LOOKUP_BYTES, UelLabelCursor,
    UelLabelStatistics,
};

pub use spill::{SpillCursor, SpillEntry, SpillOptions, SpillSorter, SpillStatistics};

pub use large::{
    DomainId, LargeLabelCursor, LargeLabelLookup, LargeResolvedLabel, UniqueDomainCursor,
};

pub use write::{
    KeyBounds, MetadataOptions, MetadataStatistics, SymbolReference, SymbolWriter,
    UnsortedSymbolWriter, UnsortedWriteStatistics, WriteCompression, WriteDomain, WriteOptions,
    WriteStatistics, WriteStatus, WriteSymbol, Writer, WriterSymbolId, WriterUelId,
};
