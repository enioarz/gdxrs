use crate::{
    Error, ErrorKind, Result, SpecialValue, SymbolType, UelId, Value, budget::Reservation,
    metadata::SymbolRecord, reader::Reader, source::Source, stream::Stream,
};
use std::{mem::size_of, sync::Arc};

pub(crate) struct Record {
    pub dimension: u8,
    pub fields: usize,
    pub keys: [Option<UelId>; 20],
    pub values: [Value; 5],
}
impl Record {
    pub fn key(&self, d: usize) -> Option<UelId> {
        self.keys.get(d).copied().flatten()
    }
}
pub(crate) struct Decoder {
    source: Arc<Source>,
    record: SymbolRecord,
    requested: u32,
    stream: Option<Stream>,
    started: bool,
    done: bool,
    error: Option<Error>,
    previous: [i64; 20],
    minimum: [i32; 20],
    maximum: [i32; 20],
    widths: [u8; 20],
    emitted: u64,
    row: Record,
    _state: Reservation,
}
impl Decoder {
    pub fn new(reader: &Reader, requested: &SymbolRecord) -> Result<Self> {
        if size_of::<Self>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "decoder fixed state exceeds reservation",
            ));
        }
        let state = reader.source.budget.reserve(16 * 1024)?;
        let record = reader.resolve(requested)?;
        let row = Record {
            dimension: record.dimension,
            fields: record.kind.fields(),
            keys: [None; 20],
            values: [Value::ZERO; 5],
        };
        Ok(Self {
            source: Arc::clone(&reader.source),
            record,
            requested: requested.id,
            stream: None,
            started: false,
            done: false,
            error: None,
            previous: [-256; 20],
            minimum: [0; 20],
            maximum: [0; 20],
            widths: [0; 20],
            emitted: 0,
            row,
            _state: state,
        })
    }
    fn start(&mut self) -> Result<()> {
        if self.record.id == 0 {
            self.started = true;
            return Ok(());
        }
        let h = self.source.header()?;
        let mut s = Stream::new(
            Arc::clone(&self.source),
            self.record.position,
            h.offsets[0],
            self.record.compressed,
            h.order,
            "records",
        )?;
        s.marker(b"_DATA_")?;
        if s.byte()? != self.record.dimension {
            return Err(s.error(
                ErrorKind::InvalidDimension,
                "data/catalog dimensions disagree",
            ));
        }
        if s.i32()? < -1 {
            return Err(s.error(ErrorKind::InvalidCount, "invalid record-count sentinel"));
        }
        for d in 0..usize::from(self.record.dimension) {
            let low = s.i32()?;
            let high = s.i32()?;
            let range = i64::from(high) - i64::from(low) + 1;
            if self.record.count != 0 && range <= 0 {
                return Err(s.error(
                    ErrorKind::InvalidReference,
                    "nonempty key bounds are invalid",
                ));
            }
            self.minimum[d] = low;
            self.maximum[d] = high;
            self.widths[d] = if (1..=255).contains(&range) {
                1
            } else if (256..=65535).contains(&range) {
                2
            } else {
                4
            };
        }
        self.started = true;
        self.stream = Some(s);
        Ok(())
    }
    fn advance(&mut self) -> Result<bool> {
        if self.done {
            return Ok(false);
        }
        if !self.started {
            self.start()?;
        }
        if self.record.id == 0 {
            let count = u64::from(self.source.counts()?.uels);
            if self.emitted == count {
                self.done = true;
                return Ok(false);
            }
            self.emitted += 1;
            let id = u32::try_from(self.emitted)
                .map_err(|_| Error::new(ErrorKind::InvalidReference, "universe ID overflow"))?;
            self.row.keys[0] = Some(UelId::new(id)?);
            self.row.values[0] = Value::ZERO;
            return Ok(true);
        }
        let s = self
            .stream
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "missing record stream"))?;
        let tag = s.byte()?;
        if tag == 255 {
            if self.emitted != u64::from(self.record.count) {
                return Err(s.error(ErrorKind::InvalidCount, "premature record terminator"));
            }
            self.done = true;
            if self.record.dimension == 0 && self.record.count == 0 {
                self.row.values = defaults(self.record.kind, self.record.user);
                self.emitted = 1;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.emitted >= u64::from(self.record.count) {
            return Err(s.error(ErrorKind::InvalidCount, "data exceeds catalog record count"));
        }
        let dim = usize::from(self.record.dimension);
        let old = self.previous;
        if dim == 0 {
            if tag != 1 {
                return Err(s.error(ErrorKind::InvalidKeyTag, "invalid scalar record tag"));
            }
        } else if tag > 0 && usize::from(tag) <= dim {
            for d in usize::from(tag) - 1..dim {
                let relative = match self.widths[d] {
                    1 => i64::from(s.byte()?),
                    2 => i64::from(s.u16()?),
                    4 => i64::from(s.i32()?),
                    _ => return Err(s.error(ErrorKind::InvalidKeyTag, "invalid key width")),
                };
                self.previous[d] = relative + i64::from(self.minimum[d]);
            }
        } else if usize::from(tag) > dim {
            self.previous[dim - 1] = self.previous[dim - 1]
                .checked_add(i64::from(tag) - dim as i64)
                .ok_or_else(|| s.error(ErrorKind::InvalidReference, "key delta overflow"))?;
        } else {
            return Err(s.error(ErrorKind::InvalidKeyTag, "invalid first-changed dimension"));
        }
        for d in 0..dim {
            let key = self.previous[d];
            if key < 1
                || key > i64::from(self.source.counts()?.uels)
                || key < i64::from(self.minimum[d])
                || key > i64::from(self.maximum[d])
            {
                return Err(s.error(
                    ErrorKind::InvalidReference,
                    "decoded UEL ID outside bounds/table",
                ));
            }
            self.row.keys[d] = Some(UelId::new(u32::try_from(key).map_err(|_| {
                s.error(ErrorKind::InvalidReference, "UEL conversion overflow")
            })?)?);
        }
        if dim > 0 && self.emitted > 0 && self.previous[..dim] <= old[..dim] {
            return Err(s.error(
                ErrorKind::InvalidKeyTag,
                "record keys are not strictly increasing",
            ));
        }
        for field in self.row.values.iter_mut().take(self.row.fields) {
            *field = decode_value(s)?;
        }
        self.emitted += 1;
        Ok(true)
    }
    pub fn next(&mut self) -> Result<Option<&Record>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        match self.advance() {
            Ok(true) => {
                if self.done {
                    self.stream = None;
                }
                Ok(Some(&self.row))
            }
            Ok(false) => {
                self.stream = None;
                Ok(None)
            }
            Err(e) => {
                let mut error = e.symbol(self.requested);
                error.source_id = Some(self.source.id);
                self.stream = None;
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
}
fn decode_value(s: &mut Stream) -> Result<Value> {
    Ok(match s.byte()? {
        0 => Value::Special(SpecialValue::Undefined),
        1 => Value::Special(SpecialValue::NotAvailable),
        2 => Value::Special(SpecialValue::PositiveInfinity),
        3 => Value::Special(SpecialValue::NegativeInfinity),
        4 => Value::Special(SpecialValue::Epsilon),
        5 => Value::ZERO,
        6 => Value::NumberBits(1f64.to_bits()),
        7 => Value::NumberBits((-1f64).to_bits()),
        8 => Value::NumberBits(0.5f64.to_bits()),
        9 => Value::NumberBits(2f64.to_bits()),
        10 => Value::ordinary(s.bits()?).map_err(|e| s.context(e))?,
        _ => return Err(s.error(ErrorKind::InvalidValueTag, "unknown numeric tag")),
    })
}
fn defaults(kind: SymbolType, user: i32) -> [Value; 5] {
    let zero = Value::ZERO;
    let one = Value::NumberBits(1f64.to_bits());
    let pos = Value::Special(SpecialValue::PositiveInfinity);
    let neg = Value::Special(SpecialValue::NegativeInfinity);
    let (low, high) = match kind {
        SymbolType::Variable => match user {
            1 => (zero, one),
            2 | 3 | 6 | 7 => (zero, pos),
            4 => (neg, zero),
            5 => (neg, pos),
            8 | 9 => (one, pos),
            _ => (zero, zero),
        },
        SymbolType::Equation => match user {
            54 | 58 => (zero, pos),
            55 => (neg, zero),
            56 => (neg, pos),
            _ => (zero, zero),
        },
        _ => (zero, zero),
    };
    [zero, zero, low, high, one]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;
    fn value(v: Value) -> String {
        match v {
            Value::NumberBits(b) => format!("f:{b:016x}"),
            Value::Acronym { id, .. } => format!("a:{}", id.get()),
            Value::Special(v) => format!(
                "s:{}",
                match v {
                    SpecialValue::Undefined => "undef",
                    SpecialValue::NotAvailable => "na",
                    SpecialValue::PositiveInfinity => "pinf",
                    SpecialValue::NegativeInfinity => "minf",
                    SpecialValue::Epsilon => "eps",
                }
            ),
        }
    }
    #[test]
    fn every_public_record_matches_authored_stream() {
        let mut fixtures = 0;
        let mut total = 0;
        for entry in std::fs::read_dir("tests/fixtures/v7").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "gdx") {
                continue;
            }
            let reader = match Reader::open(&path, crate::OpenOptions::default()) {
                Ok(v) => v,
                Err(e)
                    if !cfg!(feature = "compression")
                        && e.kind == ErrorKind::CompressionUnavailable =>
                {
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            let expected = std::fs::read_to_string(path.with_extension("expected.tsv")).unwrap();
            let expected: String = expected
                .lines()
                .filter(|s| s.starts_with("R\t") || s.starts_with("E\t"))
                .map(|s| format!("{s}\n"))
                .collect();
            let mut output = String::new();
            for n in 0..=reader.information().unwrap().symbol_count {
                let handle = reader.symbol(crate::SymbolId::new(n).unwrap()).unwrap();
                let mut decoder = Decoder::new(&reader, &handle.inner.record).unwrap();
                let mut rows = 0;
                while let Some(row) = decoder.next().unwrap() {
                    write!(&mut output, "R\t{n}\t").unwrap();
                    for d in 0..usize::from(row.dimension) {
                        write!(
                            &mut output,
                            "{}{}",
                            if d == 0 { "" } else { "," },
                            row.key(d).unwrap().get()
                        )
                        .unwrap();
                    }
                    output.push('\t');
                    for f in 0..row.fields {
                        write!(
                            &mut output,
                            "{}{}",
                            if f == 0 { "" } else { "," },
                            value(row.values[f])
                        )
                        .unwrap();
                    }
                    output.push('\n');
                    rows += 1;
                }
                writeln!(&mut output, "E\t{n}\t{rows}").unwrap();
                total += rows;
            }
            assert_eq!(output, expected, "{}", path.display());
            fixtures += 1;
        }
        assert_eq!(fixtures, if cfg!(feature = "compression") { 10 } else { 8 });
        if cfg!(feature = "compression") {
            assert_eq!(total, 141354);
        }
    }
    #[test]
    fn invalid_record_tags_dimensions_and_keys_are_terminal() {
        for (name, kind) in [
            ("bad-key-tag", ErrorKind::InvalidKeyTag),
            ("bad-value-tag", ErrorKind::InvalidValueTag),
            ("bad-dimension", ErrorKind::InvalidDimension),
        ] {
            let reader = Reader::open(
                format!("tests/fixtures/invalid/{name}.gdx"),
                crate::OpenOptions::default(),
            )
            .unwrap();
            let h = reader.symbol(crate::SymbolId::new(1).unwrap()).unwrap();
            let mut d = Decoder::new(&reader, &h.inner.record).unwrap();
            assert_eq!(d.next().err().unwrap().kind, kind);
            assert_eq!(d.next().err().unwrap().kind, kind);
        }
    }
    struct BytesSource(Vec<u8>);
    impl crate::source::ReadAt for BytesSource {
        fn read_at(&self, at: u64, out: &mut [u8]) -> std::io::Result<usize> {
            let rest = self.0.get(at as usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len());
            out[..n].copy_from_slice(&rest[..n]);
            Ok(n)
        }
        fn unchanged(&self) -> std::io::Result<bool> {
            Ok(true)
        }
    }
    fn bytes_reader(bytes: Vec<u8>) -> Reader {
        let len = bytes.len() as u64;
        Reader::from_source(Source::testing(Box::new(BytesSource(bytes)), len)).unwrap()
    }
    #[test]
    fn corrupt_counts_bounds_and_order_reject_without_panics() {
        let original = std::fs::read("tests/fixtures/v7/optional-domains.gdx").unwrap();
        let reader = bytes_reader(original.clone());
        let h = reader.symbol(crate::SymbolId::new(1).unwrap()).unwrap();
        let position = h.inner.record.position as usize;
        for count in [0, 2] {
            let mut d = Decoder::new(&reader, &h.inner.record).unwrap();
            d.record.count = count;
            let error = loop {
                match d.next() {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("accepted count mismatch"),
                    Err(e) => break e,
                }
            };
            assert_eq!(error.kind, ErrorKind::InvalidCount);
        }
        for (offset, bytes) in [
            (position + 21, 2i32.to_le_bytes()),
            (position + 12, i32::MIN.to_le_bytes()),
        ] {
            let mut corrupt = original.clone();
            corrupt[offset..offset + 4].copy_from_slice(&bytes);
            let reader = bytes_reader(corrupt);
            let h = reader.symbol(crate::SymbolId::new(1).unwrap()).unwrap();
            let mut d = Decoder::new(&reader, &h.inner.record).unwrap();
            assert_eq!(d.next().err().unwrap().kind, ErrorKind::InvalidReference);
        }
        let original = std::fs::read("tests/fixtures/v7/key-widths.gdx").unwrap();
        let reader = bytes_reader(original.clone());
        let h = reader.find_symbol(b"w65535").unwrap().unwrap();
        let mut corrupt = original;
        let position = h.inner.record.position as usize;
        // Last explicitly encoded suffix becomes key 1 after key 254.
        corrupt[position + 53..position + 55].copy_from_slice(&0u16.to_le_bytes());
        let reader = bytes_reader(corrupt);
        let h = reader.find_symbol(b"w65535").unwrap().unwrap();
        let mut d = Decoder::new(&reader, &h.inner.record).unwrap();
        let error = loop {
            match d.next() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("accepted nonmonotone keys"),
                Err(e) => break e,
            }
        };
        assert_eq!(error.kind, ErrorKind::InvalidKeyTag);
        assert_eq!(d.next().err().unwrap().kind, error.kind);
    }
    #[test]
    fn explicit_conversion_retains_typed_identity() {
        assert_eq!(
            Value::ordinary(f64::NAN.to_bits()).unwrap(),
            Value::NumberBits(f64::NAN.to_bits())
        );
        assert!(
            matches!(Value::ordinary((7.0_f64*10.0e300).to_bits()).unwrap(),Value::Acronym {id,..} if id.get()==7)
        );
        assert_eq!(
            defaults(SymbolType::Variable, 5)[2],
            Value::Special(SpecialValue::NegativeInfinity)
        );
    }
}
