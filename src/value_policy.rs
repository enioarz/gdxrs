//! Explicit numeric adapters. Typed scans and typed writes never consult these maps.
use crate::*;
/// Conversion of a typed value into a single floating-point number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatConversion {
    Exact,
    AllowLoss,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IeeeWritePolicy {
    #[default]
    Preserve,
    NormalizeNative,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadCollisions {
    Reject,
    AllowLoss,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AcronymReadPolicy {
    #[default]
    Preserve,
    ToNotAvailable,
}
const SPECIALS: [SpecialValue; 5] = [
    SpecialValue::Undefined,
    SpecialValue::NotAvailable,
    SpecialValue::PositiveInfinity,
    SpecialValue::NegativeInfinity,
    SpecialValue::Epsilon,
];
pub(crate) fn special_index(s: SpecialValue) -> usize {
    match s {
        SpecialValue::Undefined => 0,
        SpecialValue::NotAvailable => 1,
        SpecialValue::PositiveInfinity => 2,
        SpecialValue::NegativeInfinity => 3,
        SpecialValue::Epsilon => 4,
    }
}
/// Exact bit sentinels, in UNDEF, NA, +INF, -INF, EPS order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecialValueMap {
    bits: [u64; 5],
}
impl Default for SpecialValueMap {
    fn default() -> Self {
        Self {
            bits: [
                (1.0e300f64).to_bits(),
                (2.0e300f64).to_bits(),
                (3.0e300f64).to_bits(),
                (4.0e300f64).to_bits(),
                (5.0e300f64).to_bits(),
            ],
        }
    }
}
impl SpecialValueMap {
    pub fn new(bits: [u64; 5]) -> Result<Self> {
        for i in 0..5 {
            if bits[i + 1..].contains(&bits[i]) {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "duplicate special sentinel bits",
                ));
            }
        }
        Ok(Self { bits })
    }
    pub fn bits(self) -> [u64; 5] {
        self.bits
    }
    pub fn sentinel(self, special: SpecialValue) -> u64 {
        self.bits[special_index(special)]
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteValuePolicy {
    specials: SpecialValueMap,
    ieee: IeeeWritePolicy,
}
impl WriteValuePolicy {
    pub fn new(specials: SpecialValueMap, ieee: IeeeWritePolicy) -> Self {
        Self { specials, ieee }
    }
    pub fn special_values(self) -> SpecialValueMap {
        self.specials
    }
    pub fn ieee_policy(self) -> IeeeWritePolicy {
        self.ieee
    }
    /// Match sentinels first, by bits (including NaN payloads and signed zero).
    /// Optional native normalization explicitly loses IEEE NaN/infinity/zero identity.
    pub fn decode_bits(self, bits: u64) -> Result<Value> {
        if let Some(i) = self.specials.bits.iter().position(|b| *b == bits) {
            return Ok(Value::Special(SPECIALS[i]));
        }
        let number = f64::from_bits(bits);
        if self.ieee == IeeeWritePolicy::NormalizeNative {
            if number.is_nan() {
                return Ok(Value::Special(SpecialValue::NotAvailable));
            }
            if number.is_infinite() {
                return Ok(Value::Special(if number.is_sign_negative() {
                    SpecialValue::NegativeInfinity
                } else {
                    SpecialValue::PositiveInfinity
                }));
            }
            if number == 0.0 {
                return Ok(Value::ZERO);
            }
        }
        Value::ordinary(bits)
    }
    pub(crate) fn values(self, values: &[f64]) -> Result<[Value; 5]> {
        if values.len() > 5 {
            return Err(Error::new(
                ErrorKind::InvalidDimension,
                "at most five value fields",
            ));
        }
        let mut out = [Value::ZERO; 5];
        for (to, from) in out.iter_mut().zip(values) {
            *to = self.decode_bits(from.to_bits())?;
        }
        Ok(out)
    }
}
impl AcronymId {
    /// Canonical GDX numeric encoding. Reject IDs whose product overflows f64.
    pub fn value(self) -> Result<Value> {
        let bits = (10.0e300 * f64::from(self.get())).to_bits();
        let value = Value::ordinary(bits)?;
        if !matches!(value,Value::Acronym{id,..} if id==self) {
            return Err(Error::new(
                ErrorKind::InvalidAcronymId,
                "acronym ID cannot be encoded as a finite value",
            ));
        }
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcronymMapping {
    pub original: AcronymId,
    pub mapped: AcronymId,
    pub automatic: bool,
}
/// Original identity plus the numeric projection. Reading never changes Value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectedValue {
    pub original: Value,
    pub acronym_mapping: Option<AcronymMapping>,
    pub(crate) bits: u64,
}
impl ProjectedValue {
    pub fn to_f64(self, conversion: FloatConversion) -> Result<f64> {
        if conversion == FloatConversion::Exact && self.original != Value::NumberBits(self.bits) {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "numeric conversion would discard typed identity",
            ));
        }
        Ok(f64::from_bits(self.bits))
    }
}
