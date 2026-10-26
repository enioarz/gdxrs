//! Streaming sorted raw records; fixed state only.
use super::*;
mod unsorted;
pub use unsorted::{UnsortedSymbolWriter, UnsortedWriteStatistics};

#[derive(Clone, Copy)]
pub(super) struct DomainMembership {
    pub target: u32,
    pub count: u32,
    pub start: u64,
}
pub(super) type Domains = [Option<DomainMembership>; 20];
pub(super) struct RecordSetup {
    pub kind: SymbolType,
    pub dimension: u8,
    pub domains: Domains,
    pub members: u64,
}

/// Inclusive bounds. Omitted bounds use 0..i32::MAX on each axis.
#[derive(Clone, Copy, Debug)]
pub struct KeyBounds {
    pub minimum: i32,
    pub maximum: i32,
}

/// Exclusive borrowing record encoder. Must be explicitly finished.
pub struct SymbolWriter<'a> {
    writer: &'a mut Writer,
    id: WriterSymbolId,
    kind: SymbolType,
    dim: usize,
    bounds: [KeyBounds; 20],
    widths: [u8; 20],
    previous: [u32; 20],
    domains: Domains,
    count: u32,
    position: u64,
    members: u64,
    finished: bool,
    _state: Reservation,
}
impl Writer {
    /// Freeze registration and start one non-alias symbol. Domain sets must
    /// already be completed. Each symbol can be written at most once.
    pub fn begin_symbol(
        &mut self,
        id: WriterSymbolId,
        bounds: Option<&[KeyBounds]>,
    ) -> Result<SymbolWriter<'_>> {
        self.open()?;
        let prepared = (|| {
            if self.symbol_active {
                return Err(Error::new(
                    ErrorKind::InvalidWriteState,
                    "symbol is unfinished",
                ));
            }
            let state = self.budget.reserve(16 * 1024)?;
            if size_of::<SymbolWriter<'_>>() > 16 * 1024 {
                return Err(Error::new(
                    ErrorKind::BudgetTooSmall,
                    "symbol controls exceed allowance",
                ));
            }
            let RecordSetup {
                kind,
                dimension: dim,
                domains,
                members,
            } = self.metadata_call(|m| m.start_records(id))?;
            let dim = usize::from(dim);
            let mut selected = [KeyBounds {
                minimum: 0,
                maximum: i32::MAX,
            }; 20];
            if let Some(bounds) = bounds {
                if bounds.len() != dim {
                    return Err(Error::new(
                        ErrorKind::InvalidDimension,
                        "bounds dimension mismatch",
                    ));
                }
                for (d, bound) in bounds.iter().enumerate() {
                    if bound.minimum < 0 || bound.maximum < bound.minimum {
                        return Err(Error::new(
                            ErrorKind::InvalidOptions,
                            "invalid inclusive key bounds",
                        ));
                    }
                    selected[d] = *bound;
                }
            }
            let mut widths = [0; 20];
            let position = self.position;
            self.section_string(b"_DATA_")?;
            self.encoded(|e, w| e.bytes(&[dim as u8], |b| w.bytes(b)))?;
            self.section_i32(-1)?;
            for d in 0..dim {
                let n = i64::from(selected[d].maximum) - i64::from(selected[d].minimum) + 1;
                widths[d] = if n <= 255 {
                    1
                } else if n <= 65535 {
                    2
                } else {
                    4
                };
                self.section_i32(selected[d].minimum)?;
                self.section_i32(selected[d].maximum)?;
            }
            Ok((
                state, kind, dim, domains, members, selected, widths, position,
            ))
        })();
        match prepared {
            Ok((state, kind, dim, domains, members, bounds, widths, position)) => {
                self.symbol_active = true;
                Ok(SymbolWriter {
                    writer: self,
                    id,
                    kind,
                    dim,
                    bounds,
                    widths,
                    domains,
                    members,
                    position,
                    count: 0,
                    previous: [0; 20],
                    finished: false,
                    _state: state,
                })
            }
            Err(e) => Err(self.fail(e)),
        }
    }
}
impl SymbolWriter<'_> {
    fn emit(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.encoded(|e, w| e.bytes(bytes, |b| w.bytes(b)))
    }
    /// Encode one strictly increasing tuple and exactly the kind's value fields.
    /// Any failure terminates the entire file transaction.
    pub fn write(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<()> {
        let result = self.row(keys, values);
        result.map_err(|e| self.writer.fail(e))
    }
    fn validate_input(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<[u32; 20]> {
        self.writer.open()?;
        if keys.len() != self.dim || values.len() != self.kind.fields() {
            return Err(Error::new(
                ErrorKind::InvalidDimension,
                "record arity mismatch",
            ));
        }
        let mut current = [0u32; 20];
        for (d, key) in keys.iter().enumerate() {
            let key = key.get();
            if key < self.bounds[d].minimum as u32 || key > self.bounds[d].maximum as u32 {
                return Err(Error::new(
                    ErrorKind::InvalidReference,
                    "key outside supplied bounds",
                ));
            }
            current[d] = key;
        }
        self.writer
            .metadata
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata is closed"))?
            .validate_record(&self.domains, keys, values, self.kind)?;
        Ok(current)
    }
    fn row(&mut self, keys: &[WriterUelId], values: &[Value]) -> Result<()> {
        let current = self.validate_input(keys, values)?;
        if self.count == i32::MAX as u32 {
            return Err(Error::new(
                ErrorKind::InvalidCount,
                "record count exceeds v7 range",
            ));
        }
        if self.count > 0 && current[..self.dim] <= self.previous[..self.dim] {
            return Err(Error::new(
                ErrorKind::InvalidKeyTag,
                "keys are duplicate or out of order",
            ));
        }
        let mut packet = RowPacket {
            bytes: [0; 126],
            length: 0,
        };
        if self.dim == 0 {
            packet.bytes(&[1])?;
        } else {
            let changed = if self.count == 0 {
                0
            } else {
                (0..self.dim)
                    .find(|d| current[*d] != self.previous[*d])
                    .ok_or_else(|| Error::new(ErrorKind::InvalidKeyTag, "unchanged key"))?
            };
            let delta = i64::from(current[self.dim - 1]) - i64::from(self.previous[self.dim - 1]);
            if self.count > 0
                && changed == self.dim - 1
                && delta > 0
                && delta + (self.dim as i64) < 255
            {
                packet.bytes(&[(delta + self.dim as i64) as u8])?;
            } else {
                packet.bytes(&[(changed + 1) as u8])?;
                for (d, key) in current.iter().enumerate().take(self.dim).skip(changed) {
                    let relative = *key - self.bounds[d].minimum as u32;
                    packet.bytes(&relative.to_le_bytes()[..usize::from(self.widths[d])])?;
                }
            }
        }
        for value in values {
            packet.value(*value)?;
        }
        self.emit(&packet.bytes[..packet.length])?;
        if self.kind == SymbolType::Set && self.dim == 1 {
            self.writer
                .metadata
                .as_mut()
                .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata is closed"))?
                .member(self.id, keys[0])?;
        }
        self.previous = current;
        self.count += 1;
        Ok(())
    }
    /// Close the data section and retain checked catalog/domain information.
    pub fn finish(mut self) -> Result<u32> {
        let result = (|| {
            self.writer.open()?;
            self.emit(&[255])?;
            self.writer.section_end()?;
            self.writer.metadata_call(|m| {
                m.complete_records(self.id, self.position, self.count, self.members)
            })?;
            self.writer.symbol_active = false;
            self.finished = true;
            Ok(self.count)
        })();
        result.map_err(|e| self.writer.fail(e))
    }
}
struct RowPacket {
    bytes: [u8; 126],
    length: usize,
}
impl RowPacket {
    fn bytes(&mut self, value: &[u8]) -> Result<()> {
        let end = self
            .length
            .checked_add(value.len())
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "record packet overflow"))?;
        self.bytes
            .get_mut(self.length..end)
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "record exceeds v7 packet bound"))?
            .copy_from_slice(value);
        self.length = end;
        Ok(())
    }
    fn value(&mut self, value: Value) -> Result<()> {
        let bits = match value {
            Value::Special(s) => {
                return self.bytes(&[match s {
                    SpecialValue::Undefined => 0,
                    SpecialValue::NotAvailable => 1,
                    SpecialValue::PositiveInfinity => 2,
                    SpecialValue::NegativeInfinity => 3,
                    SpecialValue::Epsilon => 4,
                }]);
            }
            Value::NumberBits(bits) => bits,
            Value::Acronym { encoded_bits, .. } => encoded_bits,
        };
        let common = [0.0f64, 1.0, -1.0, 0.5, 2.0];
        if let Some(index) = common.iter().position(|n| n.to_bits() == bits) {
            self.bytes(&[5 + index as u8])
        } else {
            self.bytes(&[10])?;
            self.bytes(&bits.to_le_bytes())
        }
    }
}
impl Drop for SymbolWriter<'_> {
    fn drop(&mut self) {
        if !self.finished && self.writer.statistics.status == WriteStatus::Open {
            self.writer.fail(Error::new(
                ErrorKind::InvalidWriteState,
                "symbol writer abandoned",
            ));
        }
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
