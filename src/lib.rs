//! Independent, bounded-memory GDX v7 reader.

#![forbid(unsafe_code)]

mod budget;
mod data_error;
mod error;
mod format;
mod labels;
mod large;
mod mapped;
pub use data_error::{
    DataErrorKind, DataErrorRecord, DataErrorStatistics, DataOrigin, ErrorLimits,
};
mod metadata;
mod metadata_operations;
pub use metadata_operations::{
    MetadataSession, MetadataVisitStatistics, SetTextInformation, UelInformation,
    UelMappingInformation,
};
mod model;
mod reader;
mod records;
mod scan;
mod selection;
mod source;
mod spill;
mod stream;
mod value_policy;
mod value_session;
mod write;
pub use budget::BudgetStatistics;
pub use error::{Error, ErrorKind, Result};
pub use mapped::{
    MappedCursor, MappedReadOptions, MappedRecord, MappedScanStatistics, MappedUelId, MappedUels,
    MissingMapping,
};
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
pub use value_policy::{
    AcronymMapping, AcronymReadPolicy, FloatConversion, IeeeWritePolicy, ProjectedValue,
    ReadCollisions, SpecialValueMap, WriteValuePolicy,
};
pub use value_session::{AcronymInformation, ValueSession};

pub use scan::{BatchView, RecordCheckpoint, RecordView, ScanCursor, ScanStatistics};
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
    AppendOptions, AppendPolicy, AppendStatistics, DeferredRecordWriter, KeyBounds,
    MappedSymbolWriter, MetadataOptions, MetadataStatistics, StringRecordWriter,
    StringWriteStatistics, SymbolReference, SymbolWriter, UnsortedSymbolWriter,
    UnsortedWriteStatistics, ValidationStatistics, WriteCompression, WriteDomain, WriteOptions,
    WriteStatistics, WriteStatus, WriteSymbol, Writer, WriterSymbolId, WriterUelId,
};
