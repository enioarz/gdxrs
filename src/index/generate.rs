use super::*;
#[derive(Clone, Copy)]
struct Level {
    entries: [Entry; FAN],
    len: usize,
}
impl Default for Level {
    fn default() -> Self {
        Self {
            entries: [Entry::default(); FAN],
            len: 0,
        }
    }
}
struct Tree<'a, F: FnMut(u64, &Page) -> Result<()>> {
    levels: crate::budget::Buffer<Level>,
    next: u64,
    directories: u64,
    sink: &'a mut F,
    limit: u64,
}
impl<F: FnMut(u64, &Page) -> Result<()>> Tree<'_, F> {
    fn append(&mut self, page: &Page, first: u64, end: u64) -> Result<Entry> {
        let next = self
            .next
            .checked_add(PAGE as u64)
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::TemporarySpaceExceeded,
                    "index byte quota exceeded",
                )
            })?;
        (self.sink)(self.next, page)?;
        let entry = Entry {
            first,
            end,
            offset: self.next,
            digest: hash(&page[..PAGE - 32]),
        };
        self.next = next;
        Ok(entry)
    }
    fn add(&mut self, mut level: usize, mut entry: Entry) -> Result<()> {
        loop {
            let slot = self.levels.data.get_mut(level).ok_or_else(invalid)?;
            slot.entries[slot.len] = entry;
            slot.len += 1;
            if slot.len < FAN {
                return Ok(());
            }
            entry = self.flush(level)?;
            level += 1;
        }
    }
    fn flush(&mut self, level: usize) -> Result<Entry> {
        let slot = self.levels.data.get_mut(level).ok_or_else(invalid)?;
        let count = slot.len;
        if count == 0 {
            return Err(invalid());
        }
        let first = slot.entries[0].first;
        let end = slot.entries[count - 1].end;
        let mut page = prefix(2, (level + 1) as u8, count as u8, first, end);
        for (i, e) in slot.entries.iter().take(count).enumerate() {
            e.write(&mut page, 32 + i * 56);
        }
        seal(&mut page);
        slot.len = 0;
        let entry = self.append(&page, first, end)?;
        self.directories = self.directories.checked_add(1).ok_or_else(invalid)?;
        Ok(entry)
    }
    fn finish(mut self, intervals: u64) -> Result<Root> {
        loop {
            let level = self
                .levels
                .data
                .iter()
                .position(|s| s.len != 0)
                .ok_or_else(invalid)?;
            if self.levels.data[level].len == 1
                && self.levels.data[level + 1..].iter().all(|s| s.len == 0)
            {
                return Ok(Root {
                    entry: self.levels.data[level].entries[0],
                    depth: level as u8,
                    intervals,
                    directory_pages: self.directories,
                    bytes: self.next,
                });
            }
            let entry = self.flush(level)?;
            self.add(level + 1, entry)?;
        }
    }
}
pub(super) fn generate<F: FnMut(u64, &Page) -> Result<()>>(
    reader: &Reader,
    symbol: &SymbolHandle,
    options: &IndexOptions,
    mut sink: F,
) -> Result<(Root, DecoderPosition, u64)> {
    let _state = reader.source.budget.reserve(16 * 1024)?;
    let mut levels = crate::budget::Buffer::new(&reader.source.budget, LEVELS)?;
    for _ in 0..LEVELS {
        levels.push(Level::default())?;
    }
    let mut tree = Tree {
        levels,
        next: PAGE as u64,
        directories: 0,
        sink: &mut sink,
        limit: options.maximum_index_bytes,
    };
    let mut decoder = Decoder::new(reader, &symbol.inner.record)?;
    let initial = decoder.checkpoint()?;
    let mut start = initial;
    let (mut count, mut intervals, mut logical) = (0u64, 0u64, initial.logical_bytes());
    let (mut minimum, mut maximum) = ([0u32; 20], [0u32; 20]);
    loop {
        let next = decoder.next()?;
        let eof = next.is_none();
        if let Some(row) = next {
            for d in 0..usize::from(row.dimension) {
                let key = row.key(d).ok_or_else(invalid)?.get();
                minimum[d] = if count == 0 { key } else { minimum[d].min(key) };
                maximum[d] = maximum[d].max(key);
            }
            count = count.checked_add(1).ok_or_else(invalid)?;
            logical = logical.max(decoder.position()?.logical_bytes());
        }
        if eof && count == 0 && intervals != 0 {
            break;
        }
        if eof
            || count >= options.record_stride
            || logical
                .checked_sub(start.logical_bytes())
                .ok_or_else(invalid)?
                >= options.decoded_byte_stride
        {
            let end = start.row_ordinal().checked_add(count).ok_or_else(invalid)?;
            let next_interval = intervals.checked_add(1).ok_or_else(invalid)?;
            let mut page = prefix(1, 0, 1, intervals, next_interval);
            put(&mut page, 32, &start.row_ordinal().to_le_bytes());
            put(&mut page, 40, &end.to_le_bytes());
            for d in 0..20 {
                put(&mut page, 48 + 4 * d, &minimum[d].to_le_bytes());
                put(&mut page, 128 + 4 * d, &maximum[d].to_le_bytes());
            }
            put(&mut page, 208, &start.index_bytes()?);
            seal(&mut page);
            let entry = tree.append(&page, intervals, next_interval)?;
            tree.add(0, entry)?;
            intervals = next_interval;
            count = 0;
            minimum = [0; 20];
            maximum = [0; 20];
            if eof {
                break;
            }
            start = decoder.checkpoint()?;
            logical = start.logical_bytes();
        }
    }
    reader.source.check_unchanged()?;
    let rows = decoder.position()?.row_ordinal();
    Ok((tree.finish(intervals)?, initial, rows))
}
