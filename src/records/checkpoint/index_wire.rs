//! Canonical private sidecar bytes; caller must source-verify the index first.
use super::*;
pub(crate) const POSITION_BYTES: usize = 512;
struct Wire {
    bytes: [u8; POSITION_BYTES],
    at: usize,
}
impl Wire {
    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        let end = self.at.checked_add(bytes.len()).ok_or_else(invalid)?;
        self.bytes
            .get_mut(self.at..end)
            .ok_or_else(invalid)?
            .copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }
    fn get<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut b = [0; N];
        let end = self.at.checked_add(N).ok_or_else(invalid)?;
        b.copy_from_slice(self.bytes.get(self.at..end).ok_or_else(invalid)?);
        self.at = end;
        Ok(b)
    }
    fn flag(&mut self) -> Result<bool> {
        match self.get::<1>()?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid()),
        }
    }
}
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidIndex, "invalid checkpoint encoding")
}
impl DecoderPosition {
    pub(crate) fn index_bytes(&self) -> Result<[u8; POSITION_BYTES]> {
        let mut w = Wire {
            bytes: [0; POSITION_BYTES],
            at: 0,
        };
        let i = self.identity;
        for n in [i.requested, i.resolved, i.count] {
            w.put(&n.to_le_bytes())?;
        }
        for n in [i.position, i.end] {
            w.put(&n.to_le_bytes())?;
        }
        w.put(&[
            i.dimension,
            i.kind.code(),
            u8::from(i.compressed),
            u8::from(i.order == Order::Big),
        ])?;
        w.put(&i.user.to_le_bytes())?;
        for n in self.previous {
            w.put(&n.to_le_bytes())?;
        }
        for n in self.minimum.into_iter().chain(self.maximum) {
            w.put(&n.to_le_bytes())?;
        }
        w.put(&self.widths)?;
        w.put(&self.emitted.to_le_bytes())?;
        w.put(&[
            u8::from(self.started),
            u8::from(self.done),
            u8::from(self.stream.is_some()),
        ])?;
        let stream = self.stream.unwrap_or_default();
        w.put(&stream.physical.to_le_bytes())?;
        w.put(
            &u32::try_from(stream.decoded)
                .map_err(|_| invalid())?
                .to_le_bytes(),
        )?;
        w.put(&stream.logical.to_le_bytes())?;
        Ok(w.bytes)
    }
    pub(crate) fn from_index_bytes(bytes: [u8; POSITION_BYTES], source: u64) -> Result<Self> {
        let mut w = Wire { bytes, at: 0 };
        let requested = u32::from_le_bytes(w.get()?);
        let resolved = u32::from_le_bytes(w.get()?);
        let count = u32::from_le_bytes(w.get()?);
        let position = u64::from_le_bytes(w.get()?);
        let end = u64::from_le_bytes(w.get()?);
        let dimension = w.get::<1>()?[0];
        if dimension > 20 {
            return Err(invalid());
        }
        let kind = SymbolType::decode(w.get::<1>()?[0])?;
        let compressed = w.flag()?;
        let order = if w.flag()? { Order::Big } else { Order::Little };
        let user = i32::from_le_bytes(w.get()?);
        let mut previous = [0; 20];
        for n in &mut previous {
            *n = i64::from_le_bytes(w.get()?);
        }
        let mut minimum = [0; 20];
        let mut maximum = [0; 20];
        for n in minimum.iter_mut().chain(&mut maximum) {
            *n = i32::from_le_bytes(w.get()?);
        }
        let widths = w.get()?;
        let emitted = u64::from_le_bytes(w.get()?);
        let started = w.flag()?;
        let done = w.flag()?;
        let has_stream = w.flag()?;
        let stream = Checkpoint {
            physical: u64::from_le_bytes(w.get()?),
            decoded: u32::from_le_bytes(w.get()?) as usize,
            logical: u64::from_le_bytes(w.get()?),
        };
        if !has_stream && stream != Checkpoint::default() || w.bytes[w.at..].iter().any(|b| *b != 0)
        {
            return Err(invalid());
        }
        Ok(Self {
            identity: Identity {
                source,
                requested,
                resolved,
                position,
                end,
                count,
                dimension,
                kind,
                user,
                compressed,
                order,
            },
            previous,
            minimum,
            maximum,
            widths,
            emitted,
            started,
            done,
            stream: has_stream.then_some(stream),
        })
    }
    pub(crate) fn logical_bytes(&self) -> u64 {
        self.stream.map_or(0, |s| s.logical)
    }
    pub(crate) fn dimension(&self) -> u8 {
        self.identity.dimension
    }
}
