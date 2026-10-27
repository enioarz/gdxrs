//! Bounded writer-journal replacement; relocate membership anchors, not file data.
//! Reader policy journals use other frame meanings and never call this helper.
use super::*;
pub(super) struct Edit {
    replacement: Option<Packet>,
}
impl Edit {
    pub(super) fn keep() -> Self {
        Self { replacement: None }
    }
    pub(super) fn replace(packet: Packet) -> Self {
        Self {
            replacement: Some(packet),
        }
    }
}
#[derive(Clone, Copy)]
struct Members {
    symbol: u32,
    old: u64,
    new: u64,
    count: u32,
    last: u32,
}
impl Metadata {
    pub(super) fn rewrite(&mut self, mut edit: impl FnMut(&Frame) -> Result<Edit>) -> Result<()> {
        if self.owned[1] || self.sort_directory.is_some() {
            return Err(invalid("metadata scratch phase is active"));
        }
        let mut output = Store::create(&self.path(1), &self.budget)?;
        self.owned[1] = true;
        self.stats.peak_open_files = self
            .stats
            .peak_open_files
            .max(self.statistics().open_files + 1);
        let original_bytes = self.j()?.length;
        let mut at = 0;
        let mut members = None::<Members>;
        loop {
            let old_at = at;
            let Some(f) = self.j()?.next(&mut at)? else {
                break;
            };
            let replacement = if f.tag == 9 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                let key = p.u32()?;
                p.done()?;
                if symbol == 0 || symbol > self.stats.symbols || key == 0 || key > self.stats.uels {
                    return Err(corrupt());
                }
                if let Some(group) = members.as_mut() {
                    if group.symbol != symbol || key <= group.last {
                        return Err(corrupt());
                    }
                    group.count = next_id(group.count)?;
                    group.last = key;
                } else {
                    members = Some(Members {
                        symbol,
                        old: old_at,
                        new: output.length,
                        count: 1,
                        last: key,
                    });
                }
                Edit::keep()
            } else if f.tag == 8 {
                let mut p = Parse::new(f.payload());
                let id = p.u32()?;
                let position = p.u64()?;
                let count = p.u32()?;
                let start = p.u64()?;
                p.done()?;
                let relocated = if let Some(group) = members.take() {
                    if group.symbol != id || group.count != count || group.old != start {
                        return Err(corrupt());
                    }
                    group.new
                } else if start == old_at {
                    output.length
                } else if start == 0 && count == 0 {
                    0
                } else {
                    return Err(corrupt());
                };
                let mut p = Packet::new();
                p.u32(id)?;
                p.u64(position)?;
                p.u32(count)?;
                p.u64(relocated)?;
                Edit::replace(p)
            } else {
                if members.is_some() {
                    return Err(corrupt());
                }
                edit(&f)?
            };
            let payload = replacement
                .replacement
                .as_ref()
                .map_or(f.payload(), Packet::payload);
            output.append(
                f.tag,
                payload,
                &mut self.used,
                &mut self.peak,
                self.options.maximum_temporary_bytes,
            )?;
        }
        if members.is_some() {
            return Err(corrupt());
        }
        output.sync()?;
        output.close();
        self.journal = None;
        event(Event::MetadataRename).map_err(Error::io)?;
        fs::rename(self.path(1), self.path(0)).map_err(Error::io)?;
        self.owned[1] = false;
        self.used = self.used.checked_sub(original_bytes).ok_or_else(corrupt)?;
        output.reopen(&self.path(0))?;
        self.journal = Some(output);
        Ok(())
    }
}
