//! ID-preserving UEL edits, session-only nodes and independent display domains.
use super::*;
impl Metadata {
    pub(crate) fn node(&mut self, id: u32) -> Result<i32> {
        let mut at = 0;
        let mut result = None;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 10 {
                let mut p = Parse::new(f.payload());
                let text = p.u32()?;
                let node = p.u32()? as i32;
                p.done()?;
                if node == 0 {
                    return Err(corrupt());
                }
                if text == id {
                    if result.is_some() {
                        return Err(corrupt());
                    }
                    result = Some(node);
                }
            }
        }
        Ok(result.unwrap_or(0))
    }
    pub(crate) fn assign_node(&mut self, id: u32, node: i32) -> Result<bool> {
        if self.node(id)? != 0 {
            return Ok(false);
        }
        if node != 0 {
            let mut p = Packet::new();
            p.u32(id)?;
            p.u32(node as u32)?;
            self.append(10, &p)?;
        }
        Ok(true)
    }
    pub(crate) fn overlay_label(&mut self, id: u32) -> Result<Option<Bytes>> {
        let mut at = 0;
        let mut result = None;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 1 {
                let mut p = Parse::new(f.payload());
                let raw = p.u32()?;
                let label = p.string()?;
                p.done()?;
                if raw == id {
                    result = Some(label);
                }
            }
        }
        Ok(result)
    }
    pub(crate) fn assign_label(&mut self, id: u32, label: &[u8]) -> Result<()> {
        super::validate_string_label(label)?;
        let mut p = Packet::new();
        p.u32(id)?;
        p.string(label)?;
        self.append(1, &p)
    }
    pub(super) fn rename_uel(&mut self, id: WriterUelId, new: &[u8]) -> Result<()> {
        let raw = self.validate_uel(id)?;
        if raw <= self.inherited_uels {
            return Err(invalid("cannot rename inherited UEL"));
        }
        label(new)?;
        if self.find_string(1, new, false)?.is_some() {
            return Err(invalid("UEL rename target already exists"));
        }
        let mut packet = Packet::new();
        packet.u32(raw)?;
        packet.string(new)?;
        self.rewrite(|f| {
            if f.tag == 1 && Parse::new(f.payload()).u32()? == raw {
                Ok(super::edit::Edit::replace(packet.clone()))
            } else {
                Ok(super::edit::Edit::keep())
            }
        })
    }
    fn validate_uel(&self, id: WriterUelId) -> Result<u32> {
        if id.owner != self.owner || id.get() > self.stats.uels {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "UEL belongs to another writer",
            ));
        }
        Ok(id.get())
    }
    pub(super) fn uel_information(&mut self, id: WriterUelId) -> Result<UelInformation> {
        let raw = self.validate_uel(id)?;
        let label = self.overlay_label(raw)?.ok_or_else(corrupt)?;
        let mapped = if self.map_ready && raw <= self.map_maximum {
            self.map_lookup(raw, false)?
                .map(|m| MappedUelId::new(i64::from(m)))
                .transpose()?
        } else {
            None
        };
        Ok(UelInformation {
            id: UelId::new(raw)?,
            mapped,
            label,
        })
    }
    pub(super) fn find_uel(&mut self, label: &[u8]) -> Result<Option<WriterUelId>> {
        self.find_string(1, label, false)?
            .map(|id| self.bound_uel(id))
            .transpose()
    }
    pub(super) fn text_information(&mut self, id: u32) -> Result<SetTextInformation> {
        if id >= self.stats.set_texts {
            return Err(invalid("set-text ID outside table"));
        }
        let text = if id == 0 {
            Bytes::default()
        } else {
            let mut at = 0;
            let mut found = None;
            while let Some(f) = self.j()?.next(&mut at)? {
                if f.tag == 4 {
                    let mut p = Parse::new(f.payload());
                    if p.u32()? == id {
                        found = Some(p.string()?);
                        p.done()?;
                        break;
                    }
                }
            }
            found.ok_or_else(corrupt)?
        };
        Ok(SetTextInformation {
            id,
            text,
            node: self.node(id)?,
        })
    }
    pub(crate) fn map_information(&mut self, total: u32) -> Result<UelMappingInformation> {
        let highest = if self.map_ready && self.map_entries != 0 {
            let ordinal = self.map_entries.checked_mul(2).ok_or_else(corrupt)?;
            let pair = self
                .mapped_store
                .as_mut()
                .ok_or_else(corrupt)?
                .integer(ordinal)?
                .to_le_bytes();
            let raw = u32::from_le_bytes(pair[4..].try_into().map_err(|_| corrupt())?);
            if raw == 0 || raw > self.map_maximum {
                return Err(corrupt());
            }
            Some(MappedUelId::new(i64::from(u32::from_le_bytes(
                pair[..4].try_into().map_err(|_| corrupt())?,
            )))?)
        } else {
            None
        };
        Ok(UelMappingInformation {
            uel_count: total,
            mapped_count: self.map_entries,
            highest_mapped: highest,
        })
    }
    pub(super) fn relaxed_name_frame(f: &Frame) -> Result<Option<(u32, Bytes)>> {
        if f.tag != 6 && f.tag != 11 {
            return Ok(None);
        }
        let mut p = Parse::new(f.payload());
        p.u32()?;
        p.u8()?;
        if f.tag == 6 && p.u8()? != 2 {
            return Ok(None);
        }
        let id = p.u32()?;
        let name = p.string()?;
        p.done()?;
        Ok(Some((id, name)))
    }
    pub(super) fn display_domain(&mut self, id: u32, axis: u8) -> Result<Option<(u32, Bytes)>> {
        let mut at = 0;
        let mut result = None;
        while self.display_overrides {
            let Some(f) = self.j()?.next(&mut at)? else {
                break;
            };
            if f.tag == 11 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                let dimension = p.u8()?;
                let index = p.u32()?;
                let name = p.string()?;
                p.done()?;
                if symbol == id && dimension == axis {
                    result = Some((index, name));
                }
            }
        }
        if result.is_none() && self.mode(id)? == Some(2) {
            let (_, r) = self.domain(id, axis)?;
            result = Some((r.id, r.name));
        }
        Ok(result)
    }
    pub(super) fn relaxed_domains(&mut self, id: WriterSymbolId, names: &[&[u8]]) -> Result<()> {
        let id = self.check(id)?;
        self.edit_symbol(id)?;
        let symbol = self.lookup(id)?;
        if names.len() != usize::from(symbol.dim) {
            return Err(invalid("display domain arity mismatch"));
        }
        for n in names {
            if !n.is_empty() && *n != b"*" {
                name(n)?;
            }
        }
        for (axis, n) in names.iter().enumerate() {
            let n = if n.is_empty() { b"*".as_slice() } else { *n };
            let mut index = if n == b"*" { Some(0) } else { None };
            let mut at = 0;
            while index.is_none() {
                let Some(f) = self.j()?.next(&mut at)? else {
                    break;
                };
                if let Some((i, old)) = Self::relaxed_name_frame(&f)?
                    && old.as_slice() == n
                {
                    index = Some(i);
                }
            }
            let index = if let Some(index) = index {
                index
            } else {
                self.domains = next_id(self.domains)?;
                self.domains
            };
            let mut p = Packet::new();
            p.u32(id)?;
            p.u8(axis as u8)?;
            p.u32(index)?;
            p.string(n)?;
            self.append(11, &p)?;
            self.display_overrides = true;
        }
        Ok(())
    }
    pub(in crate::write) fn mark_has_text(&mut self, id: WriterSymbolId) -> Result<()> {
        let id = self.check(id)?;
        let mut p = Packet::new();
        p.u32(id)?;
        self.append(12, &p)
    }
    pub(super) fn has_text_id(&mut self, id: u32) -> Result<bool> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 12 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                p.done()?;
                if symbol == id {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
impl Writer {
    /// Rename an existing raw ID before or after closed symbol writes. Membership
    /// anchors are relocated in bounded scratch; record keys and alias IDs stay intact.
    pub fn rename_uel(&mut self, id: WriterUelId, new_label: &[u8]) -> Result<()> {
        self.metadata_call(|m| m.rename_uel(id, new_label))
    }
    pub fn find_uel(&mut self, label: &[u8]) -> Result<Option<WriterUelId>> {
        self.metadata_call(|m| m.find_uel(label))
    }
    pub fn uel_information(&mut self, id: WriterUelId) -> Result<UelInformation> {
        self.metadata_call(|m| m.uel_information(id))
    }
    pub fn uel_mapping_information(&mut self) -> Result<UelMappingInformation> {
        self.metadata_call(|m| m.map_information(m.stats.uels))
    }
    pub fn set_text_information(&mut self, id: u32) -> Result<SetTextInformation> {
        self.metadata_call(|m| m.text_information(id))
    }
    /// Native node numbers are session state, not serialized set-text metadata.
    pub fn set_text_node(&mut self, id: u32, node: i32) -> Result<bool> {
        self.metadata_call(|m| {
            if id >= m.stats.set_texts {
                return Err(invalid("set-text ID outside table"));
            }
            m.assign_node(id, node)
        })
    }
    /// Change display domains without changing strict membership or alias IDs.
    pub fn set_relaxed_domains(&mut self, id: WriterSymbolId, names: &[&[u8]]) -> Result<()> {
        self.metadata_call(|m| m.relaxed_domains(id, names))
    }
    pub fn has_set_text(&mut self, id: WriterSymbolId) -> Result<bool> {
        self.metadata_call(|m| {
            let id = m.check(id)?;
            m.has_text_id(id)
        })
    }
}
