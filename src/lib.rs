//! Independent, bounded-memory GDX v7 reader.

#![forbid(unsafe_code)]

mod budget;
mod error;
mod format;
mod model;
mod source;
pub use budget::BudgetStatistics;
pub use error::{Error, ErrorKind, Result};
pub use model::{
    AcronymId, KeyProjection, OpenOptions, ScanOptions, SpecialValue, SymbolId, SymbolType, UelId,
    Value, ValueField, ValueProjection,
};
pub use source::IoStatistics;
