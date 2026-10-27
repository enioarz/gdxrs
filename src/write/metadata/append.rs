//! Bounded inherited catalog import, physical-record validation and edit guards.
use super::*;
impl WriterUelId {
    pub(in crate::write) fn for_append(symbol: WriterSymbolId, id: UelId) -> Self {
        Self {
            owner: symbol.owner,
            id,
        }
    }
}
impl Metadata {
    pub(super) fn edit_symbol(&self, id: u32) -> Result<()> {
        if id <= self.inherited_symbols {
            return Err(invalid("cannot edit inherited symbol metadata"));
        }
        Ok(())
    }
    pub(super) fn import_info(&mut self, id: u32) -> Result<Option<(u32, bool)>> {
        if id > self.inherited_symbols {
            return Ok(None);
        }
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 13 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                let errors = p.u32()?;
                let text = p.u8()?;
                p.done()?;
                if errors > i32::MAX as u32 || text > 1 {
                    return Err(corrupt());
                }
                if symbol == id {
                    return Ok(Some((errors, text == 1)));
                }
            }
        }
        Ok(None)
    }
    pub(super) fn check_import_frame(
        &mut self,
        f: &Frame,
        members: &mut Option<(u32, u32, u32)>,
    ) -> Result<()> {
        let mut p = Parse::new(f.payload());
        let id = p.u32()?;
        if id == 0 || id > self.stats.symbols {
            return Err(corrupt());
        }
        match f.tag {
            8 => {
                let position = p.u64()?;
                let count = p.u32()?;
                let start = p.u64()?;
                if position > i64::MAX as u64 || count > i32::MAX as u32 || start > self.j()?.length
                {
                    return Err(corrupt());
                }
                if let Some((symbol, _, seen)) = members.take()
                    && (symbol != id
                        || count != seen
                        || start
                            .checked_add(u64::from(count) * 19)
                            .is_none_or(|end| end > self.journal.as_ref().map_or(0, |j| j.length)))
                {
                    return Err(corrupt());
                }
            }
            9 => {
                let key = p.u32()?;
                if key == 0 || key > self.stats.uels {
                    return Err(corrupt());
                }
                if let Some((symbol, last, count)) = members {
                    if *symbol != id || key <= *last {
                        return Err(corrupt());
                    }
                    *last = key;
                    *count = next_id(*count)?;
                } else {
                    let s = self.lookup(id)?;
                    if s.kind != SymbolType::Set || s.dim != 1 {
                        return Err(corrupt());
                    }
                    *members = Some((id, key, 1));
                }
            }
            12 => {
                if self.lookup(id)?.kind != SymbolType::Set {
                    return Err(corrupt());
                }
            }
            13 => {
                if p.u32()? > i32::MAX as u32 || p.u8()? > 1 {
                    return Err(corrupt());
                }
            }
            _ => return Err(corrupt()),
        }
        p.done()
    }
    fn import_metadata(&mut self, reader: &Reader) -> Result<()> {
        let mut uels = reader.uels()?;
        while let Some(entry) = uels.next()? {
            label(entry.label)?;
            if entry.id.get() != next_id(self.stats.uels)? {
                return Err(corrupt());
            }
            let mut p = Packet::new();
            p.u32(entry.id.get())?;
            p.string(entry.label)?;
            self.append(1, &p)?;
            self.stats.uels = entry.id.get();
        }
        drop(uels);
        let mut texts = reader.set_texts()?;
        let mut seen = 0;
        while let Some(entry) = texts.next()? {
            text(entry.text)?;
            if entry.id != seen {
                return Err(corrupt());
            }
            seen = next_id(seen)?;
            if entry.id == 0 {
                if !entry.text.is_empty() {
                    return Err(invalid(
                        "nonempty set-text slot zero is unsupported for append",
                    ));
                }
                continue;
            }
            let mut p = Packet::new();
            p.u32(entry.id)?;
            p.string(entry.text)?;
            self.append(4, &p)?;
            self.stats.set_texts = seen;
        }
        if seen == 0 {
            return Err(invalid("append requires the native empty set-text slot"));
        }
        drop(texts);
        // Append exact catalog order. Numeric alias references are retained even
        // for forward chains; freeze validates them once all symbols exist.
        let mut catalog = crate::metadata::Catalog::new(Arc::clone(&reader.source))?;
        while let Some(s) = catalog.next_record()? {
            name(s.name.as_slice())?;
            text(s.description.as_slice())?;
            if self.find_string(2, s.name.as_slice(), true)?.is_some() {
                return Err(invalid("duplicate inherited symbol name"));
            }
            if (s.kind == SymbolType::Variable && !(0..=9).contains(&s.user))
                || (s.kind == SymbolType::Equation && !(53..=59).contains(&s.user))
            {
                return Err(invalid("unsupported inherited symbol subtype"));
            }
            let mut p = Packet::new();
            p.u32(s.id)?;
            p.string(s.name.as_slice())?;
            p.string(s.description.as_slice())?;
            p.u8(s.kind.code())?;
            p.u8(s.dimension)?;
            p.u32(s.user as u32)?;
            p.u32(if s.kind == SymbolType::Alias {
                u32::try_from(s.user).map_err(|_| corrupt())?
            } else {
                0
            })?;
            p.string(b"")?;
            self.append(2, &p)?;
            self.stats.symbols = next_id(self.stats.symbols)?;
            if self.stats.symbols != s.id {
                return Err(corrupt());
            }
            let mut info = Packet::new();
            info.u32(s.id)?;
            info.u32(s.errors)?;
            info.u8(u8::from(s.has_text))?;
            self.append(13, &info)?;
            let token = self.string_symbol(s.id)?;
            let handle = reader.symbol(SymbolId::new(s.id)?)?;
            let mut comments = reader.comments(&handle)?;
            while let Some(comment) = comments.next()? {
                self.comment(token, comment.text)?;
            }
        }
        drop(catalog);
        let mut acronyms = reader.acronyms()?;
        while let Some(a) = acronyms.next()? {
            self.acronym(a.id, a.name, a.text)?;
        }
        Ok(())
    }
    fn import_domains(&mut self, reader: &Reader) -> Result<()> {
        self.stats.frozen = false;
        for id in 1..=self.stats.symbols {
            let raw = reader.lookup_record(SymbolId::new(id)?)?;
            let token = self.string_symbol(id)?;
            if raw.strict {
                for axis in 0..raw.dimension {
                    let mut p = Packet::new();
                    p.u32(id)?;
                    p.u8(axis)?;
                    p.u8(1)?;
                    p.u32(raw.domains[usize::from(axis)])?;
                    p.string(b"")?;
                    self.append(6, &p)?;
                }
                let mut p = Packet::new();
                p.u32(id)?;
                p.u8(1)?;
                self.append(7, &p)?;
            }
            let handle = reader.symbol(SymbolId::new(id)?)?;
            let mut domains = reader.domains(&handle)?;
            let relaxed = domains.mode() == DomainMode::Relaxed;
            let mut names: [Bytes; 20] = std::array::from_fn(|_| Bytes::default());
            let mut count = 0;
            while let Some(d) = domains.next()? {
                if count >= usize::from(raw.dimension) {
                    return Err(corrupt());
                }
                names[count] = Bytes::from_slice(d.name)?;
                count += 1;
            }
            if count != usize::from(raw.dimension) {
                return Err(corrupt());
            }
            if relaxed {
                let refs: [&[u8]; 20] = std::array::from_fn(|axis| names[axis].as_slice());
                self.relaxed_domains(token, &refs[..count])?;
            }
        }
        self.freeze()?;
        // All set membership runs now exist, so validate inherited strict data
        // without requiring a topological order or retaining a dependency table.
        for id in 1..=self.stats.symbols {
            let s = self.lookup(id)?;
            if s.kind == SymbolType::Alias || self.mode(id)? != Some(1) {
                continue;
            }
            let domains = self.record_domains(&s)?;
            let handle = reader.symbol(SymbolId::new(id)?)?;
            let raw = reader.lookup_record(SymbolId::new(id)?)?;
            let mut options = reader.scan_options();
            options.batch_bytes = options.batch_bytes.min(4096);
            let mut scan = reader.scan(&handle, options)?;
            while let Some(batch) = scan.next_batch()? {
                for row in 0..batch.len() {
                    if raw.dimension == 0 && raw.count == 0 {
                        continue;
                    }
                    let row = batch.row(row).ok_or_else(corrupt)?;
                    let token = self.string_symbol(id)?;
                    let mut keys = [WriterUelId::for_append(token, UelId::new(1)?); 20];
                    for (axis, key) in keys.iter_mut().enumerate().take(usize::from(s.dim)) {
                        *key = self.bound_uel(row.key(axis as u8).ok_or_else(corrupt)?.get())?;
                    }
                    if self.domain_violations(&domains, &keys[..usize::from(s.dim)])? != 0 {
                        return Err(invalid("inherited record violates strict domain"));
                    }
                }
            }
        }
        self.inherited_symbols = self.stats.symbols;
        self.inherited_uels = self.stats.uels;
        self.inherited_acronyms = self.stats.acronyms;
        self.stats.frozen = false;
        Ok(())
    }
}
impl Writer {
    pub(in crate::write) fn import_append_metadata(&mut self, reader: &Reader) -> Result<()> {
        self.metadata_call(|m| m.import_metadata(reader))
    }
    pub(in crate::write) fn import_append_domains(&mut self, reader: &Reader) -> Result<()> {
        self.metadata_call(|m| m.import_domains(reader))
    }
}
