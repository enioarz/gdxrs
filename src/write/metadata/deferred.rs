//! Deferred phase ownership and dependency checks use the existing journal.
use super::*;
impl Metadata {
    pub(in crate::write) fn deferred_begin(&mut self) -> Result<Arc<std::sync::Mutex<Store>>> {
        self.mutable()?;
        if self.owned[5] || self.string_store.is_some() || self.options.maximum_open_files < 4 {
            return Err(invalid(
                "deferred records require one phase and seven available scratch handles",
            ));
        }
        let store = Store::create(&self.path(5), &self.budget)?;
        self.owned[5] = true;
        let shared = Arc::new(std::sync::Mutex::new(store));
        self.deferred_store = Some(Arc::clone(&shared));
        self.stats.peak_open_files = self.stats.peak_open_files.max(self.statistics().open_files);
        self.options.maximum_open_files -= 1;
        self.options.merge_fan_in = self
            .options
            .merge_fan_in
            .min(self.options.maximum_open_files - 1);
        Ok(shared)
    }
    pub(in crate::write) fn deferred_done(&mut self, bytes: u64) -> Result<()> {
        self.stats.peak_open_files = self.statistics().peak_open_files;
        if let Some(store) = self.deferred_store.take() {
            store.lock().map_err(|_| corrupt())?.close();
        }
        event(Event::MetadataRemove).map_err(Error::io)?;
        fs::remove_file(self.path(5)).map_err(Error::io)?;
        self.owned[5] = false;
        self.used = self.used.checked_sub(bytes).ok_or_else(corrupt)?;
        self.options.maximum_open_files += 1;
        Ok(())
    }
    pub(in crate::write) fn records_completed(&mut self, id: WriterSymbolId) -> Result<bool> {
        let id = self.check(id)?;
        Ok(self.record_info(id)?.is_some())
    }
    pub(in crate::write) fn records_ready(&mut self, id: WriterSymbolId) -> Result<bool> {
        self.freeze()?;
        let id = self.check(id)?;
        let s = self.lookup(id)?;
        if self.mode(id)? == Some(1) {
            for axis in 0..s.dim {
                let (_, r) = self.domain(id, axis)?;
                let target = self.set_target(&r)?;
                let direct = self.resolve_ref(&r)?;
                if target != 0
                    && !(target == id && direct == id && s.kind == SymbolType::Set && s.dim == 1)
                    && self.record_info(target)?.is_none()
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub(in crate::write) fn deferred_shape(
        &mut self,
        id: WriterSymbolId,
    ) -> Result<(SymbolType, usize)> {
        let id = self.check(id)?;
        let s = self.lookup(id)?;
        if s.kind == SymbolType::Alias {
            return Err(invalid("aliases have no own records"));
        }
        Ok((s.kind, usize::from(s.dim)))
    }
    pub(in crate::write) fn existing_label(&mut self, label: &[u8]) -> Result<WriterUelId> {
        let n = self
            .find_string(1, label, false)?
            .ok_or_else(|| invalid("register string keys before deferred phase"))?;
        self.bound_uel(n)
    }
    pub(in crate::write) fn uel_label(&mut self, id: UelId) -> Result<Bytes> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 1 {
                let mut p = Parse::new(f.payload());
                if p.u32()? == id.get() {
                    let label = p.string()?;
                    p.done()?;
                    return Ok(label);
                }
            }
        }
        Err(corrupt())
    }
    pub(in crate::write) fn unwritten_set(&mut self, n: u32) -> Result<Option<WriterSymbolId>> {
        let s = self.lookup(n)?;
        if s.kind == SymbolType::Set && s.dim == 1 && self.record_info(n)?.is_none() {
            Ok(Some(self.string_symbol(n)?))
        } else {
            Ok(None)
        }
    }
}
