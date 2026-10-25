use crate::{Error, ErrorKind, Result};
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolId(u32);
impl SymbolId {
    pub const UNIVERSE: Self = Self(0);
    pub fn new(value: u32) -> Result<Self> {
        if value > i32::MAX as u32 {
            return Err(Error::new(
                ErrorKind::InvalidReference,
                "symbol ID exceeds format range",
            ));
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> u32 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UelId(NonZeroU32);
impl UelId {
    pub fn new(value: u32) -> Result<Self> {
        let value = NonZeroU32::new(value)
            .filter(|v| v.get() <= i32::MAX as u32)
            .ok_or_else(|| Error::new(ErrorKind::InvalidReference, "invalid UEL ID"))?;
        Ok(Self(value))
    }
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcronymId(NonZeroU32);
impl AcronymId {
    pub fn new(value: u32) -> Result<Self> {
        let value = NonZeroU32::new(value)
            .filter(|v| v.get() <= i32::MAX as u32)
            .ok_or_else(|| Error::new(ErrorKind::InvalidAcronymId, "invalid acronym ID"))?;
        Ok(Self(value))
    }
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolType {
    Set,
    Parameter,
    Variable,
    Equation,
    Alias,
}
impl SymbolType {
    pub const fn code(self) -> u8 {
        match self {
            Self::Set => 0,
            Self::Parameter => 1,
            Self::Variable => 2,
            Self::Equation => 3,
            Self::Alias => 4,
        }
    }
    pub(crate) fn decode(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Set),
            1 => Ok(Self::Parameter),
            2 => Ok(Self::Variable),
            3 => Ok(Self::Equation),
            4 => Ok(Self::Alias),
            _ => Err(Error::new(ErrorKind::InvalidType, "unknown symbol type")),
        }
    }
    pub(crate) fn fields(self) -> usize {
        if matches!(self, Self::Variable | Self::Equation) {
            5
        } else {
            1
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialValue {
    Undefined,
    NotAvailable,
    PositiveInfinity,
    NegativeInfinity,
    Epsilon,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    NumberBits(u64),
    Special(SpecialValue),
    Acronym { id: AcronymId, encoded_bits: u64 },
}
impl Value {
    pub const ZERO: Self = Self::NumberBits(0);
    pub fn number_bits(self) -> Option<u64> {
        if let Self::NumberBits(bits) = self {
            Some(bits)
        } else {
            None
        }
    }
    pub(crate) fn ordinary(bits: u64) -> Result<Self> {
        let number = f64::from_bits(bits);
        if number.is_finite() && number >= 10.0e300 {
            let id = (number / 10.0e300).round();
            if !(1.0..=f64::from(i32::MAX)).contains(&id) {
                return Err(Error::new(
                    ErrorKind::InvalidAcronymId,
                    "acronym value out of range",
                ));
            }
            Ok(Self::Acronym {
                id: AcronymId::new(id as u32)?,
                encoded_bits: bits,
            })
        } else {
            Ok(Self::NumberBits(bits))
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueField {
    Level,
    Marginal,
    Lower,
    Upper,
    Scale,
}
impl ValueField {
    pub const ALL: [Self; 5] = [
        Self::Level,
        Self::Marginal,
        Self::Lower,
        Self::Upper,
        Self::Scale,
    ];
    pub const fn index(self) -> usize {
        match self {
            Self::Level => 0,
            Self::Marginal => 1,
            Self::Lower => 2,
            Self::Upper => 3,
            Self::Scale => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub enum KeyProjection {
    #[default]
    All,
    Mask(u32),
}
#[derive(Clone, Copy, Debug, Default)]
pub enum ValueProjection {
    #[default]
    All,
    Mask(u8),
}
#[derive(Clone, Debug)]
pub struct OpenOptions {
    pub memory_budget_bytes: usize,
    pub metadata_cache_bytes: usize,
    pub maximum_batch_bytes: usize,
}
impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            memory_budget_bytes: 64 * 1024 * 1024,
            metadata_cache_bytes: 128 * 1024,
            maximum_batch_bytes: 64 * 1024,
        }
    }
}
impl OpenOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.memory_budget_bytes < 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "minimum reader budget is 1 MiB",
            ));
        }
        if self.metadata_cache_bytes > self.memory_budget_bytes
            || self.maximum_batch_bytes == 0
            || self.maximum_batch_bytes > self.memory_budget_bytes
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid cache/batch cap",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub key_projection: KeyProjection,
    pub value_projection: ValueProjection,
    pub limit: Option<u64>,
    pub batch_bytes: usize,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            key_projection: KeyProjection::All,
            value_projection: ValueProjection::All,
            limit: None,
            batch_bytes: 64 * 1024,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Bytes {
    data: [u8; 255],
    len: u8,
}
impl Default for Bytes {
    fn default() -> Self {
        Self {
            data: [0; 255],
            len: 0,
        }
    }
}
impl Bytes {
    pub const fn literal(value: &[u8]) -> Self {
        assert!(value.len() <= 255);
        let mut data = [0; 255];
        let mut n = 0;
        while n < value.len() {
            data[n] = value[n];
            n += 1;
        }
        Self {
            data,
            len: value.len() as u8,
        }
    }
    pub fn from_slice(value: &[u8]) -> Result<Self> {
        let len = u8::try_from(value.len())
            .map_err(|_| Error::new(ErrorKind::InvalidCount, "short string too long"))?;
        let mut out = Self {
            len,
            ..Self::default()
        };
        out.data[..value.len()].copy_from_slice(value);
        Ok(out)
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..usize::from(self.len)]
    }
}
