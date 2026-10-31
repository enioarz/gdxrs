//! Shared checksummed sparse bidirectional maps. All extents share metadata quotas.
use super::*;
fn conflict() -> Error {
    Error::new(
        ErrorKind::InvalidReference,
        "UEL map is missing or ambiguous",
    )
}
fn u32_at(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| corrupt())?))
}
impl Metadata {
    fn map_prepare(&mut self) -> Result<()> {
        self.mutable()?;
        if self.owned[4] || self.map_ready || self.options.maximum_open_files < 4 {
            return Err(invalid(
                "one map phase and seven available scratch handles are required",
            ));
        }
        self.mapped_store = Some(Store::create(&self.path(4), &self.budget)?);
        self.owned[4] = true;
        self.options.maximum_open_files -= 1;
        self.options.merge_fan_in = self
            .options
            .merge_fan_in
            .min(self.options.maximum_open_files - 1);
        Ok(())
    }
    fn map_sort<I>(&mut self, entries: I, maximum: u32) -> Result<SpillSorter>
    where
        I: IntoIterator<Item = Result<(u32, u32)>>,
    {
        let mut sort = self.record_sorter()?;
        let observe = sort.observer();
        let result = (|| {
            for pair in entries {
                let (raw, mapped) = pair?;
                if raw == 0 || raw > maximum || mapped > i32::MAX as u32 {
                    return Err(conflict());
                }
                for (direction, key, target) in [(0u8, raw, mapped), (1, mapped, raw)] {
                    let mut label = [0; 5];
                    label[0] = direction;
                    label[1..].copy_from_slice(&key.to_be_bytes());
                    sort.push(&label, u64::from(target))?;
                }
            }
            Ok(())
        })();
        if let Err(mut error) = result {
            if let Err(cleanup) = sort.cancel()
                && error.cleanup_error().is_none()
            {
                error = error.with_cleanup(io::Error::other(cleanup));
            }
            drop(sort);
            self.record_sort_statistics(observe());
            return Err(error);
        }
        Ok(sort)
    }
    fn map_finish(&mut self, sort: SpillSorter, previous_bytes: u64, maximum: u32) -> Result<()> {
        let observe = sort.observer();
        let mut pending = Some(sort);
        let mut result = (|| {
            // All input iterators/handles must close before replacing their spool.
            self.mapped_store = None;
            event(Event::MetadataRemove).map_err(Error::io)?;
            fs::remove_file(self.path(4)).map_err(Error::io)?;
            self.owned[4] = false;
            self.used = self.used.checked_sub(previous_bytes).ok_or_else(corrupt)?;
            self.mapped_store = Some(Store::create(&self.path(4), &self.budget)?);
            self.owned[4] = true;
            let mut cursor = pending.take().ok_or_else(corrupt)?.finish()?;
            let replay = (|| {
                let mut last = None::<([u8; 5], u64)>;
                let mut counts = [0u32; 2];
                while let Some(entry) = cursor.next()? {
                    let key: [u8; 5] = entry.label.try_into().map_err(|_| corrupt())?;
                    if key[0] > 1 || entry.id > i32::MAX as u64 {
                        return Err(corrupt());
                    }
                    let number = u32::from_be_bytes(key[1..].try_into().map_err(|_| corrupt())?);
                    if key[0] == 0 && (number == 0 || number > maximum)
                        || key[0] == 1 && (entry.id == 0 || entry.id > u64::from(maximum))
                    {
                        return Err(corrupt());
                    }
                    if let Some((previous, target)) = last
                        && previous == key
                    {
                        if target != entry.id {
                            return Err(conflict());
                        }
                        continue; // Repeating an identical assignment is idempotent.
                    }
                    counts[key[0] as usize] = next_id(counts[key[0] as usize])?;
                    let mut payload = [0; 8];
                    payload[..4].copy_from_slice(&number.to_le_bytes());
                    payload[4..].copy_from_slice(&(entry.id as u32).to_le_bytes());
                    self.mapped_store.as_mut().ok_or_else(corrupt)?.append(
                        8,
                        &payload,
                        &mut self.used,
                        &mut self.peak,
                        self.options.maximum_temporary_bytes,
                    )?;
                    last = Some((key, entry.id));
                }
                if counts[0] != counts[1] {
                    return Err(corrupt());
                }
                Ok(counts[0])
            })();
            let closed = cursor.close();
            let entries = match (replay, closed) {
                (Err(mut error), cleanup) => {
                    if let Err(cleanup) = cleanup
                        && error.cleanup_error().is_none()
                    {
                        error = error.with_cleanup(io::Error::other(cleanup));
                    }
                    return Err(error);
                }
                (Ok(_), Err(error)) => return Err(error),
                (Ok(entries), Ok(_)) => entries,
            };
            self.mapped_store.as_mut().ok_or_else(corrupt)?.sync()?;
            self.mapped_store
                .as_mut()
                .ok_or_else(corrupt)?
                .enable_cache(&self.budget, self.options.lookup_cache_bytes)?;
            self.map_entries = entries;
            self.map_maximum = maximum;
            self.map_ready = true;
            Ok(())
        })();
        if let Some(mut sorter) = pending.take()
            && let Err(cleanup) = sorter.cancel()
        {
            result = match result {
                Err(error) if error.cleanup_error().is_some() => Err(error),
                Err(error) => Err(error.with_cleanup(io::Error::other(cleanup))),
                Ok(()) => Err(cleanup),
            };
        }
        self.record_sort_statistics(observe());
        result
    }
    pub(crate) fn build_raw_map<I>(&mut self, entries: I, maximum: u32) -> Result<()>
    where
        I: IntoIterator<Item = Result<(u32, u32)>>,
    {
        self.map_prepare()?;
        let sorted = self.map_sort(entries, maximum)?;
        self.map_finish(sorted, 0, maximum)
    }
    pub(crate) fn map_count(&self) -> u32 {
        self.map_entries
    }
    pub(crate) fn has_map(&self) -> bool {
        self.map_ready
    }
    pub(crate) fn map_lookup(&mut self, key: u32, inverse: bool) -> Result<Option<u32>> {
        if !self.map_ready {
            return Err(invalid("UEL map has not been built"));
        }
        if key > i32::MAX as u32 || !inverse && (key == 0 || key > self.map_maximum) {
            return Err(conflict());
        }
        let (mut low, mut high) = (0u64, u64::from(self.map_entries));
        let base = if inverse {
            u64::from(self.map_entries)
        } else {
            0
        };
        let store = self.mapped_store.as_mut().ok_or_else(corrupt)?;
        let stamp = store.begin_snapshot()?;
        let result = (|| {
            while low < high {
                let mid = low + (high - low) / 2;
                let ordinal = base
                    .checked_add(mid)
                    .and_then(|n| n.checked_add(1))
                    .ok_or_else(corrupt)?;
                // Reuse the verified ordinal path, including physical NTFS reads.
                let pair = store
                    .integer_in_snapshot(u32::try_from(ordinal).map_err(|_| corrupt())?)?
                    .to_le_bytes();
                let found = u32_at(&pair[..4])?;
                let target = u32_at(&pair[4..])?;
                if found > i32::MAX as u32
                    || target > i32::MAX as u32
                    || inverse && (target == 0 || target > self.map_maximum)
                    || !inverse && (found == 0 || found > self.map_maximum)
                {
                    return Err(corrupt());
                }
                match found.cmp(&key) {
                    std::cmp::Ordering::Less => low = mid + 1,
                    std::cmp::Ordering::Greater => high = mid,
                    std::cmp::Ordering::Equal => return Ok(Some(target)),
                }
            }
            Ok(None)
        })();
        if result.is_ok() {
            store.end_snapshot(stamp)?;
        }
        result
    }
    pub(crate) fn map_sort_closed(&mut self, statistics: SpillStatistics, closed: bool) {
        self.record_sort_statistics(statistics);
        if closed {
            self.sort_directory = None;
            self.sort_used = 0;
        }
    }
    pub(super) fn register_map<I, B>(&mut self, entries: I) -> Result<()>
    where
        I: IntoIterator<Item = Result<(MappedUelId, B)>>,
        B: AsRef<[u8]>,
    {
        self.map_prepare()?;
        let mut spool = self.mapped_store.take().ok_or_else(corrupt)?;
        let mut count = 0u32;
        for entry in entries {
            let (mapped, bytes) = entry?;
            let bytes = bytes.as_ref();
            label(bytes)?;
            count = next_id(count)?;
            let mut packet = Packet::new();
            packet.u32(mapped.get())?;
            packet.bytes(bytes)?;
            spool.append(
                1,
                packet.payload(),
                &mut self.used,
                &mut self.peak,
                self.options.maximum_temporary_bytes,
            )?;
        }
        spool.sync()?;
        let extent = spool.length;
        let extra = u64::from(count).checked_mul(19).ok_or_else(corrupt)?;
        self.string_reserve_mapping(extra)?;
        let spool = std::cell::RefCell::new(spool);
        let mut at = 0;
        let labels = std::iter::from_fn(|| {
            if at == extent {
                return None;
            }
            Some((|| {
                let frame = spool.borrow_mut().next(&mut at)?.ok_or_else(corrupt)?;
                if frame.tag != 1 || frame.payload().len() < 4 || at > extent {
                    return Err(corrupt());
                }
                Bytes::from_slice(&frame.payload()[4..])
            })())
        });
        let limit = extent.checked_add(extra).ok_or_else(corrupt)?;
        let (mut used, mut peak, mut mapped) = (extent, extent, 0u32);
        self.bulk_extend(labels, |ordinal, id| {
            mapped = next_id(mapped)?;
            if ordinal != u64::from(mapped) {
                return Err(corrupt());
            }
            let mut p = [0; 8];
            p[..4].copy_from_slice(&mapped.to_le_bytes());
            p[4..].copy_from_slice(&id.get().to_le_bytes());
            spool
                .borrow_mut()
                .append(8, &p, &mut used, &mut peak, limit)
        })?;
        if mapped != count || used != limit {
            return Err(corrupt());
        }
        spool.borrow_mut().sync()?;
        // Bounded batch prevents alternating two distant spool pages per entry.
        let mut ids = Buffer::new(&self.budget, 1024)?;
        let (mut at, mut tail, mut emitted, mut loaded, mut index) =
            (0, extent, 0u32, 0u32, 0usize);
        let entries = std::iter::from_fn(|| {
            if emitted == count {
                return None;
            }
            Some((|| {
                let frame = spool.borrow_mut().next(&mut at)?.ok_or_else(corrupt)?;
                if frame.tag != 1 || frame.payload().len() < 4 || at > extent {
                    return Err(corrupt());
                }
                let caller = u32_at(&frame.payload()[..4])?;
                if index == ids.data.len() {
                    ids.data.clear();
                    index = 0;
                    while ids.data.len() < ids.data.capacity() && loaded < count {
                        let frame = spool.borrow_mut().next(&mut tail)?.ok_or_else(corrupt)?;
                        loaded = next_id(loaded)?;
                        if frame.tag != 8
                            || frame.payload().len() != 8
                            || u32_at(&frame.payload()[..4])? != loaded
                            || tail > limit
                        {
                            return Err(corrupt());
                        }
                        ids.push(u32_at(&frame.payload()[4..])?)?;
                    }
                }
                let raw = ids.data.get(index).copied().ok_or_else(corrupt)?;
                index += 1;
                emitted = next_id(emitted)?;
                Ok((raw, caller))
            })())
        });
        let maximum = self.stats.uels;
        let sorted = self.map_sort(entries, maximum)?;
        if at != extent || tail != limit || emitted != count || loaded != count {
            return Err(corrupt());
        }
        drop(spool.into_inner());
        self.map_finish(sorted, limit, maximum)
    }
}
impl Writer {
    /// Build one immutable sparse caller map, retaining existing raw registration
    /// IDs and assigning new labels in first-encounter order. Same pairs repeat
    /// idempotently; either-direction collisions are terminal errors.
    pub fn register_mapped_uels<I, B>(&mut self, entries: I) -> Result<()>
    where
        I: IntoIterator<Item = Result<(MappedUelId, B)>>,
        B: AsRef<[u8]>,
    {
        self.metadata_call(|m| m.register_map(entries))
    }
    pub fn raw_id_for_mapped(&mut self, id: MappedUelId) -> Result<WriterUelId> {
        self.metadata_call(|m| {
            let raw = m.map_lookup(id.get(), true)?.ok_or_else(conflict)?;
            m.bound_uel(raw)
        })
    }
    pub fn mapped_id_for_raw(&mut self, id: WriterUelId) -> Result<MappedUelId> {
        self.metadata_call(|m| {
            if id.owner != m.owner || id.get() > m.stats.uels {
                return Err(Error::new(
                    ErrorKind::SourceMismatch,
                    "UEL belongs to another writer",
                ));
            }
            MappedUelId::new(i64::from(
                m.map_lookup(id.get(), false)?.ok_or_else(conflict)?,
            ))
        })
    }
}
