//! Private append-only mapping journal, owned by an explicit reader policy session.
use super::*;
impl Metadata {
    pub(crate) fn value_extent(&self) -> u64 {
        self.journal.as_ref().map_or(0, |s| s.length)
    }
    pub(crate) fn value_find(
        &mut self,
        start: u64,
        id: AcronymId,
        inverse: bool,
    ) -> Result<Option<AcronymMapping>> {
        let mut at = start;
        let mut found = None;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag != 9 {
                return Err(corrupt());
            }
            let mut p = Parse::new(f.payload());
            let original = AcronymId::new(p.u32()?)?;
            let mapped = AcronymId::new(p.u32()?)?;
            let automatic = match p.u8()? {
                0 => false,
                1 => true,
                _ => return Err(corrupt()),
            };
            p.done()?;
            if (if inverse { mapped } else { original }) == id {
                if found.is_some() {
                    return Err(corrupt());
                }
                found = Some(AcronymMapping {
                    original,
                    mapped,
                    automatic,
                });
            }
        }
        Ok(found)
    }
    pub(crate) fn value_assign(&mut self, mapping: AcronymMapping) -> Result<()> {
        let mut p = Packet::new();
        p.u32(mapping.original.get())?;
        p.u32(mapping.mapped.get())?;
        p.u8(u8::from(mapping.automatic))?;
        self.append(9, &p)
    }
    pub(super) fn update_acronym(&mut self, id: AcronymId, n: &[u8], t: &[u8]) -> Result<()> {
        self.mutable()?;
        name(n)?;
        text(t)?;
        if self.find_string(2, n, true)?.is_some() || self.owned[1] || self.sort_directory.is_some()
        {
            return Err(invalid("acronym name collision or active scratch phase"));
        }
        let mut at = 0;
        let mut found = false;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 5 {
                let mut p = Parse::new(f.payload());
                let old = p.u32()?;
                let old_name = p.string()?;
                if old == id.get() {
                    if found {
                        return Err(corrupt());
                    }
                    found = true;
                } else if old_name.as_slice().eq_ignore_ascii_case(n) {
                    return Err(invalid("acronym name collision"));
                }
            }
        }
        if !found {
            return Err(invalid("unregistered acronym"));
        }
        let mut replacement = Packet::new();
        replacement.u32(id.get())?;
        replacement.string(n)?;
        replacement.string(t)?;
        self.rewrite(|f| {
            if f.tag == 5 && Parse::new(f.payload()).u32()? == id.get() {
                Ok(super::edit::Edit::replace(replacement.clone()))
            } else {
                Ok(super::edit::Edit::keep())
            }
        })
    }
}
impl Writer {
    /// Change a pre-registered acronym's name/text before metadata freezes. The
    /// numeric ID and registration order stay intact; rewrite shares scratch quotas.
    pub fn update_acronym(&mut self, id: AcronymId, name: &[u8], text: &[u8]) -> Result<()> {
        self.metadata_call(|m| m.update_acronym(id, name, text))
    }
}
