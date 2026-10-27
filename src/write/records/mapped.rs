use super::*;
/// Caller-ID rows externally sorted back to original raw-ID order for GDX output.
pub struct MappedSymbolWriter<'a> {
    inner: UnsortedSymbolWriter<'a>,
    keys: Buffer<WriterUelId>,
    _state: Reservation,
}
impl Writer {
    pub fn begin_mapped_symbol(&mut self, id: WriterSymbolId) -> Result<MappedSymbolWriter<'_>> {
        self.open()?;
        let prepared = (|| {
            if !self.metadata.as_ref().is_some_and(Metadata::has_map) {
                return Err(Error::new(
                    ErrorKind::InvalidWriteState,
                    "register a caller map first",
                ));
            }
            let state = self.budget.reserve(CONTROL)?;
            let keys = Buffer::new(&self.budget, 20)?;
            Ok((state, keys))
        })();
        let (state, keys) = prepared.map_err(|e| self.fail(e))?;
        let inner = self.begin_unsorted_symbol(id, None)?;
        Ok(MappedSymbolWriter {
            inner,
            keys,
            _state: state,
        })
    }
}
impl MappedSymbolWriter<'_> {
    pub fn write_f64(&mut self, keys: &[MappedUelId], values: &[f64]) -> Result<()> {
        let inner = self
            .inner
            .inner
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "mapped writer closed"))?;
        let converted = inner.writer.convert_values(values);
        let typed = converted.map_err(|e| self.inner.reject(e))?;
        self.write(keys, &typed[..values.len()])
    }
    pub fn write(&mut self, keys: &[MappedUelId], values: &[Value]) -> Result<()> {
        let result =
            (|| {
                let inner = self.inner.inner.as_mut().ok_or_else(|| {
                    Error::new(ErrorKind::InvalidWriteState, "mapped writer closed")
                })?;
                inner.writer.open()?;
                if keys.len() != inner.dim {
                    return Err(Error::new(
                        ErrorKind::InvalidDimension,
                        "mapped key arity mismatch",
                    ));
                }
                self.keys.data.clear();
                let m =
                    inner.writer.metadata.as_mut().ok_or_else(|| {
                        Error::new(ErrorKind::InvalidWriteState, "metadata closed")
                    })?;
                for key in keys {
                    let raw = m.map_lookup(key.get(), true)?.ok_or_else(|| {
                        Error::new(ErrorKind::InvalidReference, "mapped UEL is not registered")
                    })?;
                    self.keys.push(m.bound_uel(raw)?)?;
                }
                self.inner.write(&self.keys.data, values)
            })();
        result.map_err(|e| self.inner.reject(e))
    }
    pub fn statistics(&self) -> SpillStatistics {
        self.inner.statistics()
    }
    pub fn finish(self) -> Result<UnsortedWriteStatistics> {
        self.inner.finish()
    }
}
