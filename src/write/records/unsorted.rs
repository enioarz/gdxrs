//! Complete bounded records sorted using the existing exact-byte run format.
use super::*;
const PAYLOAD: usize = 145;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsortedWriteStatistics {
    pub records: u32,
    pub spill: SpillStatistics,
}
/// Explicit external sorting. Finish is required; failed/abandoned writes cannot publish.
pub struct UnsortedSymbolWriter<'a> {
    inner: Option<SymbolWriter<'a>>,
    sorter: Option<SpillSorter>,
    keys: Buffer<WriterUelId>,
    input: u32,
    last_statistics: SpillStatistics,
    _state: Reservation,
}
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidSpill, "invalid unsorted record payload")
}
impl Writer {
    /// Freeze metadata and sort raw tuples under the configured MetadataOptions.
    /// Duplicate keys are terminal errors. Domain sets must already be complete.
    pub fn begin_unsorted_symbol(
        &mut self,
        id: WriterSymbolId,
        bounds: Option<&[KeyBounds]>,
    ) -> Result<UnsortedSymbolWriter<'_>> {
        let inner = self.begin_symbol(id, bounds)?;
        let initialized = (|| {
            if size_of::<UnsortedSymbolWriter<'_>>() > CONTROL {
                return Err(Error::new(
                    ErrorKind::BudgetTooSmall,
                    "unsorted controls exceed allowance",
                ));
            }
            let state = inner.writer.budget.reserve(CONTROL)?;
            let keys = Buffer::new(&inner.writer.budget, inner.dim)?;
            let sorter = inner.writer.metadata_call(Metadata::record_sorter)?;
            Ok((state, keys, sorter))
        })();
        match initialized {
            Ok((state, keys, sorter)) => Ok(UnsortedSymbolWriter {
                inner: Some(inner),
                sorter: Some(sorter),
                keys,
                input: 0,
                last_statistics: SpillStatistics::default(),
                _state: state,
            }),
            Err(e) => Err(inner.writer.fail(e)),
        }
    }
}
impl UnsortedSymbolWriter<'_> {
    pub fn statistics(&self) -> SpillStatistics {
        self.sorter
            .as_ref()
            .map_or(self.last_statistics, SpillSorter::statistics)
    }
    fn reject(&mut self, mut error: Error) -> Error {
        if let Some(mut sorter) = self.sorter.take() {
            let observe = sorter.observer();
            if let Err(cleanup) = sorter.cancel()
                && error.cleanup_error().is_none()
            {
                error = error.with_cleanup(io::Error::other(cleanup));
            }
            drop(sorter);
            self.last_statistics = observe();
            if let Some(inner) = self.inner.as_mut()
                && let Some(m) = inner.writer.metadata.as_mut()
            {
                m.record_sort_statistics(self.last_statistics);
            }
        }
        if let Some(inner) = self.inner.as_mut() {
            if inner.writer.statistics.status == WriteStatus::Open {
                return inner.writer.fail(error);
            }
            return inner.writer.error.clone().unwrap_or(error);
        }
        error
    }
    /// Accept one complete typed record. Raw ordering is established at finish.
    pub fn write(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<()> {
        let result = (|| {
            let inner = self.inner.as_mut().ok_or_else(invalid)?;
            inner.validate_input(keys, values)?;
            let next = self
                .input
                .checked_add(1)
                .filter(|n| *n <= i32::MAX as u32)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidCount,
                        "unsorted input count exceeds v7 range",
                    )
                })?;
            let mut packet = Packet {
                bytes: [0; PAYLOAD],
                length: 0,
            };
            for key in keys {
                packet.bytes(&key.get().to_be_bytes())?;
            }
            for value in values {
                packet.value(*value)?;
            }
            self.sorter
                .as_mut()
                .ok_or_else(invalid)?
                .push(&packet.bytes[..packet.length], u64::from(next))?;
            self.input = next;
            Ok(())
        })();
        result.map_err(|e| self.reject(e))
    }
    /// Merge/replay, verify sorted EOF and cleanup, then close the symbol section.
    pub fn finish(mut self) -> Result<UnsortedWriteStatistics> {
        let Some(mut inner) = self.inner.take() else {
            return Err(invalid());
        };
        inner.writer.open()?;
        let Some(sorter) = self.sorter.take() else {
            return Err(inner.writer.fail(invalid()));
        };
        let observe = sorter.observer();
        let result = (|| {
            let mut cursor = sorter.finish()?;
            let replay = (|| {
                let mut records = 0u32;
                while let Some(entry) = cursor.next()? {
                    if entry.id == 0 || entry.id > u64::from(self.input) {
                        return Err(invalid());
                    }
                    let values = decode(entry.label, &mut self.keys, &inner)?;
                    inner.row(&self.keys.data, &values[..inner.kind.fields()])?;
                    records = records.checked_add(1).ok_or_else(invalid)?;
                }
                Ok(records)
            })();
            // Close explicitly even after replay fails: Drop cannot report cleanup.
            let closed = cursor.close();
            let records = match (replay, closed) {
                (Err(mut error), cleanup) => {
                    if let Err(cleanup) = cleanup
                        && error.cleanup_error().is_none()
                    {
                        error = error.with_cleanup(io::Error::other(cleanup));
                    }
                    return Err(error);
                }
                (Ok(_), Err(error)) => return Err(error),
                (Ok(records), Ok(_)) => records,
            };
            let statistics = cursor.statistics();
            if !statistics.complete
                || records != self.input
                || statistics.input_records != u64::from(self.input)
            {
                return Err(invalid());
            }
            Ok((records, statistics))
        })();
        if let Some(m) = inner.writer.metadata.as_mut() {
            m.record_sort_statistics(observe());
        }
        match result {
            Err(e) => Err(inner.writer.fail(e)),
            Ok((records, spill)) => {
                if inner.count != records {
                    return Err(inner.writer.fail(invalid()));
                }
                inner.finish()?;
                Ok(UnsortedWriteStatistics { records, spill })
            }
        }
    }
}
impl Drop for UnsortedSymbolWriter<'_> {
    fn drop(&mut self) {
        if self
            .inner
            .as_ref()
            .is_some_and(|i| i.writer.statistics.status == WriteStatus::Open)
        {
            self.reject(Error::new(
                ErrorKind::InvalidWriteState,
                "unsorted symbol abandoned",
            ));
        }
    }
}
struct Packet {
    bytes: [u8; PAYLOAD],
    length: usize,
}
impl Packet {
    fn bytes(&mut self, value: &[u8]) -> Result<()> {
        let end = self.length.checked_add(value.len()).ok_or_else(invalid)?;
        self.bytes
            .get_mut(self.length..end)
            .ok_or_else(invalid)?
            .copy_from_slice(value);
        self.length = end;
        Ok(())
    }
    fn value(&mut self, value: Value) -> Result<()> {
        match value {
            Value::NumberBits(bits) => {
                self.bytes(&[0])?;
                self.bytes(&bits.to_le_bytes())
            }
            Value::Special(s) => self.bytes(&[match s {
                SpecialValue::Undefined => 1,
                SpecialValue::NotAvailable => 2,
                SpecialValue::PositiveInfinity => 3,
                SpecialValue::NegativeInfinity => 4,
                SpecialValue::Epsilon => 5,
            }]),
            Value::Acronym { id, encoded_bits } => {
                self.bytes(&[6])?;
                self.bytes(&id.get().to_le_bytes())?;
                self.bytes(&encoded_bits.to_le_bytes())
            }
        }
    }
}
fn decode(
    bytes: &[u8],
    keys: &mut Buffer<WriterUelId>,
    inner: &SymbolWriter<'_>,
) -> Result<[Value; 5]> {
    fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8]> {
        let end = at.checked_add(n).ok_or_else(invalid)?;
        let out = bytes.get(*at..end).ok_or_else(invalid)?;
        *at = end;
        Ok(out)
    }
    if bytes.len() > PAYLOAD {
        return Err(invalid());
    }
    let mut at = 0;
    keys.data.clear();
    for _ in 0..inner.dim {
        let mut b = [0; 4];
        b.copy_from_slice(take(bytes, &mut at, 4)?);
        let id = inner
            .writer
            .metadata
            .as_ref()
            .ok_or_else(invalid)?
            .bound_uel(u32::from_be_bytes(b))?;
        keys.push(id)?;
    }
    let mut values = [Value::ZERO; 5];
    for value in values.iter_mut().take(inner.kind.fields()) {
        *value = match take(bytes, &mut at, 1)?[0] {
            0 => {
                let mut b = [0; 8];
                b.copy_from_slice(take(bytes, &mut at, 8)?);
                Value::NumberBits(u64::from_le_bytes(b))
            }
            tag @ 1..=5 => Value::Special(match tag {
                1 => SpecialValue::Undefined,
                2 => SpecialValue::NotAvailable,
                3 => SpecialValue::PositiveInfinity,
                4 => SpecialValue::NegativeInfinity,
                _ => SpecialValue::Epsilon,
            }),
            6 => {
                let mut id = [0; 4];
                id.copy_from_slice(take(bytes, &mut at, 4)?);
                let mut b = [0; 8];
                b.copy_from_slice(take(bytes, &mut at, 8)?);
                Value::Acronym {
                    id: AcronymId::new(u32::from_le_bytes(id))?,
                    encoded_bits: u64::from_le_bytes(b),
                }
            }
            _ => return Err(invalid()),
        };
    }
    if at != bytes.len() {
        return Err(invalid());
    }
    Ok(values)
}

#[cfg(test)]
#[path = "unsorted_tests.rs"]
mod tests;
