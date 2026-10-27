//! Borrowed, quota-controlled data diagnostics. No implicit error accumulation.
use crate::model::Bytes;
use crate::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataOrigin {
    RawWrite,
    MappedWrite,
    StringWrite,
    MappedRead,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataErrorKind {
    DomainViolation { dimensions: u32 },
    Duplicate,
    MissingMapping { dimensions: u32 },
}
#[derive(Clone, Copy, Debug)]
pub struct ErrorLimits {
    pub maximum_records: u64,
    pub maximum_bytes: u64,
}
impl Default for ErrorLimits {
    fn default() -> Self {
        Self {
            maximum_records: 1024,
            maximum_bytes: 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DataErrorStatistics {
    pub errors: u64,
    pub reported: u64,
    pub bytes: u64,
}
/// One rejected row. Keys and values retain original identities and bits.
/// Callback order may differ from ingestion; ordinals always refer to input.
///
/// ```compile_fail,E0521
/// use gdxrs::{DeferredRecordWriter, ErrorLimits, Result};
/// fn retain(rows: DeferredRecordWriter<'_>) -> Result<()> {
///     let mut saved = None;
///     rows.finish_reporting(ErrorLimits::default(), |row| {
///         saved = Some(row); // A callback cannot retain borrowed row storage.
///         Ok(())
///     })?;
///     Ok(())
/// }
/// ```
pub struct DataErrorRecord<'a> {
    pub session_id: u64,
    pub symbol: SymbolId,
    pub input_ordinal: u64,
    pub symbol_ordinal: u32,
    pub origin: DataOrigin,
    pub kind: DataErrorKind,
    pub raw_keys: &'a [UelId],
    pub values: &'a [Value],
    pub(crate) mapped: &'a [MappedUelId],
    pub(crate) mapped_mask: u32,
    pub(crate) labels: Option<&'a [Bytes]>,
}
impl DataErrorRecord<'_> {
    pub fn mapped_key(&self, dimension: usize) -> Option<MappedUelId> {
        if dimension >= 20 || self.mapped_mask & (1 << dimension) == 0 {
            return None;
        }
        self.mapped.get(dimension).copied()
    }
    pub fn label(&self, dimension: usize) -> Option<&[u8]> {
        self.labels?.get(dimension).map(Bytes::as_slice)
    }
    /// Logical callback payload bytes, independent of caller serialization.
    pub fn logical_bytes(&self) -> Result<u64> {
        let mut values = [0; 65];
        let value_bytes = crate::write::pack_values(self.values, &mut values)?;
        // session + symbol + ordinals + origin/kind + domain/mapped masks
        let mut bytes = 8
            + 4
            + 8
            + 4
            + 1
            + 1
            + 4
            + 4
            + self.raw_keys.len() as u64 * 4
            + u64::from(self.mapped_mask.count_ones()) * 4
            + value_bytes as u64;
        if let Some(labels) = self.labels {
            for label in labels {
                bytes = bytes
                    .checked_add(1 + label.as_slice().len() as u64)
                    .ok_or_else(limit_error)?;
            }
        }
        Ok(bytes)
    }
}
fn limit_error() -> Error {
    Error::new(
        ErrorKind::DataErrorLimitExceeded,
        "data-error output quota exceeded",
    )
}
pub(crate) fn report<F>(
    record: DataErrorRecord<'_>,
    limits: ErrorLimits,
    statistics: &mut DataErrorStatistics,
    callback: &mut F,
) -> Result<()>
where
    F: for<'a> FnMut(DataErrorRecord<'a>) -> Result<()>,
{
    statistics.errors = statistics.errors.checked_add(1).ok_or_else(limit_error)?;
    let bytes = record.logical_bytes()?;
    let reported = statistics
        .reported
        .checked_add(1)
        .filter(|n| *n <= limits.maximum_records)
        .ok_or_else(limit_error)?;
    let total = statistics
        .bytes
        .checked_add(bytes)
        .filter(|n| *n <= limits.maximum_bytes)
        .ok_or_else(limit_error)?;
    callback(record)?;
    statistics.reported = reported;
    statistics.bytes = total;
    Ok(())
}
