//! Per-reader value projections with quota-controlled, disk-backed acronym state.
use crate::*;
use crate::{budget::Reservation, model::Bytes, write::Metadata};
use std::sync::Arc;
/// Fixed-size metadata result; no borrowed cursor or full acronym table is retained.
#[derive(Clone)]
pub struct AcronymInformation {
    pub id: AcronymId,
    name: Bytes,
    text: Bytes,
}
impl AcronymInformation {
    pub fn name(&self) -> &[u8] {
        self.name.as_slice()
    }
    pub fn text(&self) -> &[u8] {
        self.text.as_slice()
    }
}
/// Explicit per-reader policy; raw typed scans remain unchanged.
///
/// ```no_run
/// use gdxrs::*;
/// # fn example() -> Result<()> {
/// let reader = Reader::open("input.gdx", OpenOptions::default())?;
/// let mut values = reader.value_session(&MetadataOptions::new("scratch"))?;
/// values.set_next_acronym_id(Some(AcronymId::new(100)?))?;
/// let projected = values.project(Value::Special(SpecialValue::Epsilon))?;
/// assert_eq!(projected.original, Value::Special(SpecialValue::Epsilon));
/// let explicitly_lossy = projected.to_f64(FloatConversion::AllowLoss)?;
/// values.close()?;
/// # let _ = explicitly_lossy;
/// # Ok(()) }
/// ```
pub struct ValueSession {
    reader: Reader,
    metadata: Metadata,
    initialized: bool,
    closed: bool,
    error: Option<Error>,
    internal: SpecialValueMap,
    read: [u64; 5],
    acronyms: AcronymReadPolicy,
    next: Option<AcronymId>,
    start: u64,
    _state: Reservation,
}
impl Reader {
    /// Explicit policy owner, returned before fallible journal initialization.
    /// Scratch options bound this separate job; all memory shares the reader root.
    pub fn value_session(&self, options: &MetadataOptions) -> Result<ValueSession> {
        if std::mem::size_of::<ValueSession>() + std::mem::size_of::<AcronymInformation>() > 16384 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "value controls exceed allowance",
            ));
        }
        let state = self.source.budget.reserve(16384)?;
        let metadata = Metadata::new(Arc::clone(&self.source.budget), self.source.id, options)?;
        let specials = SpecialValueMap::default();
        Ok(ValueSession {
            reader: self.clone(),
            metadata,
            initialized: false,
            closed: false,
            error: None,
            internal: specials,
            read: specials.bits(),
            acronyms: AcronymReadPolicy::Preserve,
            next: None,
            start: 0,
            _state: state,
        })
    }
}
impl ValueSession {
    fn ensure(&mut self) -> Result<()> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.closed {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "value session is closed",
            ));
        }
        self.reader
            .source
            .check_unchanged()
            .map_err(|e| self.fail(e))
    }
    fn journal(&mut self) -> Result<()> {
        if !self.initialized {
            self.metadata.initialize_journal()?;
            self.initialized = true;
        }
        Ok(())
    }
    fn fail(&mut self, mut e: Error) -> Error {
        self.closed = true;
        if let Err(cleanup) = self.metadata.cleanup()
            && e.cleanup_error().is_none()
        {
            e = e.with_cleanup(cleanup);
        }
        self.error = Some(e.clone());
        e
    }
    /// Scratch-owner metrics; the source acronym catalog remains streamed.
    pub fn statistics(&self) -> MetadataStatistics {
        self.metadata.statistics()
    }
    pub fn special_values(&self) -> SpecialValueMap {
        self.internal
    }
    pub fn read_special_values(&self) -> [u64; 5] {
        self.read
    }
    pub fn next_acronym_id(&self) -> Option<AcronymId> {
        self.next
    }
    pub fn set_special_values(&mut self, bits: [u64; 5]) -> Result<()> {
        self.ensure()?;
        let map = SpecialValueMap::new(bits).map_err(|e| self.fail(e))?;
        self.internal = map;
        self.read = map.bits();
        Ok(())
    }
    /// Duplicate read projections require an explicit AllowLoss selection. The
    /// original Value remains in every ProjectedValue even under this policy.
    pub fn set_read_special_values(
        &mut self,
        bits: [u64; 5],
        collisions: ReadCollisions,
    ) -> Result<()> {
        self.ensure()?;
        if collisions == ReadCollisions::Reject {
            SpecialValueMap::new(bits).map_err(|e| self.fail(e))?;
        }
        self.read = bits;
        Ok(())
    }
    pub fn reset_special_values(&mut self) -> Result<()> {
        self.set_special_values(SpecialValueMap::default().bits())
    }
    pub fn set_acronym_policy(&mut self, policy: AcronymReadPolicy) -> Result<()> {
        self.ensure()?;
        self.acronyms = policy;
        Ok(())
    }
    pub fn set_next_acronym_id(&mut self, next: Option<AcronymId>) -> Result<()> {
        self.ensure()?;
        if let Some(id) = next {
            id.value().map_err(|e| self.fail(e))?;
        }
        self.next = next;
        Ok(())
    }
    /// Start a new acronym epoch. Old journal bytes remain charged until close.
    /// Previously projected rows are never replayed or retroactively changed.
    pub fn reset_acronyms(&mut self) -> Result<()> {
        self.ensure()?;
        self.start = self.metadata.value_extent();
        self.next = None;
        self.acronyms = AcronymReadPolicy::Preserve;
        Ok(())
    }
    fn mapping(&mut self, id: AcronymId) -> Result<Option<AcronymMapping>> {
        self.journal()?;
        self.metadata.value_find(self.start, id, false)
    }
    pub fn acronym_mapping(&mut self, id: AcronymId) -> Result<Option<AcronymMapping>> {
        self.ensure()?;
        self.mapping(id).map_err(|e| self.fail(e))
    }
    fn assign(&mut self, m: AcronymMapping) -> Result<()> {
        self.journal()?;
        m.original.value()?;
        m.mapped.value()?;
        if let Some(old) = self.metadata.value_find(self.start, m.original, false)? {
            return if old.mapped == m.mapped {
                Ok(())
            } else {
                Err(Error::new(
                    ErrorKind::InvalidReference,
                    "acronym mapping is immutable within an epoch",
                ))
            };
        }
        if self
            .metadata
            .value_find(self.start, m.mapped, true)?
            .is_some()
        {
            return Err(Error::new(
                ErrorKind::InvalidReference,
                "acronym mapping collision",
            ));
        }
        self.metadata.value_assign(m)
    }
    pub fn set_acronym_mapping(&mut self, original: AcronymId, mapped: AcronymId) -> Result<()> {
        self.ensure()?;
        self.assign(AcronymMapping {
            original,
            mapped,
            automatic: false,
        })
        .map_err(|e| self.fail(e))
    }
    pub fn acronym_information(&mut self, id: AcronymId) -> Result<Option<AcronymInformation>> {
        self.ensure()?;
        let result = (|| {
            let mut cursor = self.reader.acronyms()?;
            let mut found = None;
            while let Some(v) = cursor.next()? {
                if v.id == id {
                    if found.is_some() {
                        return Err(Error::new(
                            ErrorKind::InvalidAcronymId,
                            "duplicate acronym metadata ID",
                        ));
                    }
                    found = Some(AcronymInformation {
                        id,
                        name: Bytes::from_slice(v.name)?,
                        text: Bytes::from_slice(v.text)?,
                    });
                }
            }
            Ok(found)
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn project(&mut self, value: Value) -> Result<ProjectedValue> {
        self.ensure()?;
        let result = (|| {
            let mut mapping = None;
            let bits = match value {
                Value::NumberBits(bits) => bits,
                Value::Special(s) => self.read[crate::value_policy::special_index(s)],
                Value::Acronym { id, encoded_bits } => {
                    if Value::ordinary(encoded_bits)? != value {
                        return Err(Error::new(
                            ErrorKind::InvalidAcronymId,
                            "inconsistent acronym encoding",
                        ));
                    }
                    if self.acronyms == AcronymReadPolicy::ToNotAvailable {
                        self.internal.sentinel(SpecialValue::NotAvailable)
                    } else {
                        let m = match self.mapping(id)? {
                            Some(m) => m,
                            None => {
                                let m = AcronymMapping {
                                    original: id,
                                    mapped: self.next.unwrap_or(id),
                                    automatic: self.next.is_some(),
                                };
                                self.assign(m)?;
                                if let Some(next) = self.next {
                                    self.next = Some(AcronymId::new(
                                        next.get().checked_add(1).ok_or_else(|| {
                                            Error::new(
                                                ErrorKind::InvalidAcronymId,
                                                "next acronym ID overflow",
                                            )
                                        })?,
                                    )?);
                                }
                                m
                            }
                        };
                        mapping = Some(m);
                        match m.mapped.value()? {
                            Value::Acronym { encoded_bits, .. } => encoded_bits,
                            _ => {
                                return Err(Error::new(
                                    ErrorKind::InvalidAcronymId,
                                    "invalid canonical acronym",
                                ));
                            }
                        }
                    }
                }
            };
            Ok(ProjectedValue {
                original: value,
                acronym_mapping: mapping,
                bits,
            })
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn close(&mut self) -> Result<()> {
        self.closed = true;
        self.metadata.cleanup().map_err(Error::from)
    }
    pub fn abort(&mut self) -> Result<()> {
        self.close()
    }
}
