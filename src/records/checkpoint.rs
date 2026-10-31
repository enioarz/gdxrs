//! Complete in-session restart state. Persistent/untrusted import is not exposed.
use super::*;
use crate::{format::Order, stream::Checkpoint};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    source: u64,
    requested: u32,
    resolved: u32,
    position: u64,
    end: u64,
    count: u32,
    dimension: u8,
    kind: SymbolType,
    user: i32,
    compressed: bool,
    order: Order,
}
#[derive(Clone, Copy)]
pub(crate) struct DecoderPosition {
    identity: Identity,
    previous: [i64; 20],
    minimum: [i32; 20],
    maximum: [i32; 20],
    widths: [u8; 20],
    emitted: u64,
    started: bool,
    done: bool,
    stream: Option<Checkpoint>,
}
impl DecoderPosition {
    pub(crate) fn row_ordinal(&self) -> u64 {
        self.emitted
    }
}
impl Decoder {
    fn identity(&self) -> Result<Identity> {
        let header = self.source.header()?;
        Ok(Identity {
            source: self.source.id,
            requested: self.requested,
            resolved: self.record.id,
            position: self.record.position,
            end: header.offsets[0],
            count: self.record.count,
            dimension: self.record.dimension,
            kind: self.record.kind,
            user: self.record.user,
            compressed: self.record.compressed,
            order: header.order,
        })
    }
    /// Used only after successful next/checkpoint calls, including confirmed EOF.
    pub(crate) fn position(&self) -> Result<DecoderPosition> {
        Ok(DecoderPosition {
            identity: self.identity()?,
            previous: self.previous,
            minimum: self.minimum,
            maximum: self.maximum,
            widths: self.widths,
            emitted: self.emitted,
            started: self.started,
            done: self.done,
            stream: self.stream.as_ref().map(Stream::checkpoint),
        })
    }
    pub(crate) fn checkpoint(&mut self) -> Result<DecoderPosition> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let result = (|| {
            self.source.check_unchanged()?;
            if !self.started {
                self.start()?;
            }
            let position = self.position()?;
            self.source.check_unchanged()?;
            Ok(position)
        })();
        result.map_err(|error| self.fail(error))
    }
    pub(crate) fn resume(
        reader: &Reader,
        requested: &SymbolRecord,
        point: &DecoderPosition,
    ) -> Result<Self> {
        let result = (|| {
            reader.source.check_unchanged()?;
            let mut decoder = Self::new(reader, requested)?;
            if decoder.identity()? != point.identity {
                return Err(Error::new(
                    ErrorKind::SourceMismatch,
                    "checkpoint identity mismatch",
                ));
            }
            let invalid =
                || Error::new(ErrorKind::InvalidOffset, "invalid record checkpoint state");
            let logical_count = if decoder.record.id == 0 {
                u64::from(reader.source.counts()?.uels)
            } else if decoder.record.dimension == 0 {
                1
            } else {
                u64::from(decoder.record.count)
            };
            if !point.started
                || point.emitted > logical_count
                || point.done && point.emitted != logical_count
            {
                return Err(invalid());
            }
            // Revalidate the small preamble, never the record prefix. Drop its
            // buffers before constructing the target block's independent stream.
            decoder.start()?;
            if decoder.minimum != point.minimum
                || decoder.maximum != point.maximum
                || decoder.widths != point.widths
            {
                return Err(invalid());
            }
            decoder.stream = None;
            let dim = usize::from(decoder.record.dimension);
            if decoder.record.id != 0 && point.emitted > 0 && decoder.record.count > 0 {
                for d in 0..dim {
                    let key = point.previous[d];
                    if key < 1
                        || key > i64::from(reader.source.counts()?.uels)
                        || key < i64::from(point.minimum[d])
                        || key > i64::from(point.maximum[d])
                    {
                        return Err(invalid());
                    }
                }
            } else if point.previous != [-256; 20] {
                return Err(invalid());
            }
            if point.previous[dim..] != [-256; 20][dim..] {
                return Err(invalid());
            }
            if point.done || decoder.record.id == 0 {
                if point.stream.is_some() {
                    return Err(invalid());
                }
            } else {
                let stream = point.stream.ok_or_else(invalid)?;
                if stream.physical < decoder.record.position
                    || stream.physical >= point.identity.end
                {
                    return Err(invalid());
                }
                decoder.stream = Some(Stream::resume(
                    Arc::clone(&reader.source),
                    stream,
                    point.identity.end,
                    decoder.record.compressed,
                    point.identity.order,
                    "records",
                )?);
            }
            decoder.previous = point.previous;
            decoder.emitted = point.emitted;
            decoder.done = point.done;
            reader.source.check_unchanged()?;
            Ok(decoder)
        })();
        result.map_err(|mut error| {
            error.source_id = Some(reader.source.id);
            error.symbol_id = Some(requested.id);
            error
        })
    }
}

#[cfg(test)]
mod tests;
