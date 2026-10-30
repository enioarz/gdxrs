//! Optional bounded 16-way page cache. Every buffer/control slot is root charged.
use super::*;
const WAYS: usize = 16;
#[derive(Clone, Copy)]
struct Slot {
    start: u64,
    len: usize,
    age: u64,
    dirty: bool,
}
pub(super) struct Cache {
    data: Buffer<u8>,
    slots: Buffer<Slot>,
    sets: usize,
    clock: u64,
    valid: usize,
    dirty: usize,
}
impl Cache {
    pub(super) fn new(budget: &Arc<Budget>, bytes: usize, length: u64) -> Result<Option<Self>> {
        let per_set = WAYS
            .checked_mul(PAGE + size_of::<Slot>())
            .ok_or_else(Store::invalid_snapshot)?;
        let maximum = bytes / per_set;
        if maximum == 0 {
            return Ok(None);
        }
        let desired = if length == 0 {
            maximum
        } else {
            usize::try_from(length.div_ceil(PAGE as u64).div_ceil(WAYS as u64)).unwrap_or(maximum)
        };
        let sets = maximum.min(desired.max(1));
        let count = sets * WAYS;
        let mut data = Buffer::new(budget, count * PAGE)?;
        data.data.resize(count * PAGE, 0);
        let mut slots = Buffer::new(budget, count)?;
        for _ in 0..count {
            slots.push(Slot {
                start: u64::MAX,
                len: 0,
                age: 0,
                dirty: false,
            })?;
        }
        Ok(Some(Self {
            data,
            slots,
            sets,
            clock: 0,
            valid: 0,
            dirty: 0,
        }))
    }
    pub(super) fn invalidate(&mut self) {
        for s in &mut self.slots.data {
            s.start = u64::MAX;
            s.len = 0;
            s.dirty = false;
            s.age = 0;
        }
        self.valid = 0;
        self.dirty = 0;
        self.clock = 0;
    }
    fn set(&self, start: u64) -> usize {
        let mut h = start / (PAGE as u64);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((h ^ (h >> 31)) % (self.sets as u64)) as usize
    }
    fn flush_slot(&mut self, store: &mut Store, index: usize) -> Result<()> {
        let slot = self.slots.data[index];
        if !slot.dirty {
            return Ok(());
        }
        event(Event::MetadataSeek).map_err(Error::io)?;
        let file = store.file()?;
        file.seek(SeekFrom::Start(slot.start)).map_err(Error::io)?;
        let bytes = &self.data.data[index * PAGE..index * PAGE + slot.len];
        let mut done = 0;
        while done < bytes.len() {
            let n = io_chunk(bytes.len() - done);
            match event(Event::MetadataWrite).and_then(|_| file.write(&bytes[done..done + n])) {
                Ok(0) => {
                    return Err(Error::io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "cached metadata zero write",
                    )));
                }
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::io(e)),
            }
        }
        store.stamp = Some(
            file.metadata()
                .map_err(Error::io)?
                .modified()
                .map_err(Error::io)?,
        );
        self.slots.data[index].dirty = false;
        self.dirty -= 1;
        Ok(())
    }
    pub(super) fn flush(&mut self, store: &mut Store) -> Result<()> {
        if self.dirty != 0 {
            for i in 0..self.slots.data.len() {
                self.flush_slot(store, i)?;
            }
        }
        Ok(())
    }
    fn page(&mut self, store: &mut Store, start: u64) -> Result<usize> {
        let begin = self.set(start) * WAYS;
        let end = begin + WAYS;
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or_else(Store::invalid_snapshot)?;
        if let Some(at) = (begin..end).find(|i| self.slots.data[*i].start == start) {
            self.slots.data[at].age = self.clock;
            return Ok(at);
        }
        let at = (begin..end)
            .min_by_key(|i| self.slots.data[*i].age)
            .ok_or_else(Store::invalid_snapshot)?;
        self.flush_slot(store, at)?;
        if self.slots.data[at].start != u64::MAX {
            self.valid -= 1;
        }
        self.slots.data[at] = Slot {
            start: u64::MAX,
            len: 0,
            age: 0,
            dirty: false,
        };
        let len = (store.length - start).min(PAGE as u64) as usize;
        event(Event::MetadataSeek).map_err(Error::io)?;
        let file = store.file()?;
        file.seek(SeekFrom::Start(start)).map_err(Error::io)?;
        event(Event::MetadataRead).map_err(Error::io)?;
        file.read_exact(&mut self.data.data[at * PAGE..at * PAGE + len])
            .map_err(Error::io)?;
        self.slots.data[at] = Slot {
            start,
            len,
            age: self.clock,
            dirty: false,
        };
        self.valid += 1;
        Ok(at)
    }
    pub(super) fn read(
        &mut self,
        store: &mut Store,
        mut at: u64,
        mut out: &mut [u8],
    ) -> Result<()> {
        while !out.is_empty() {
            let start = at / (PAGE as u64) * (PAGE as u64);
            let i = self.page(store, start)?;
            let offset = (at - start) as usize;
            let n = out.len().min(self.slots.data[i].len - offset);
            out[..n].copy_from_slice(&self.data.data[i * PAGE + offset..i * PAGE + offset + n]);
            at += n as u64;
            out = &mut out[n..];
        }
        Ok(())
    }
    #[cfg(not(windows))]
    pub(super) fn patch(&mut self, store: &mut Store, at: u64, bytes: &[u8; 19]) -> Result<()> {
        let start = at / (PAGE as u64) * (PAGE as u64);
        let i = self.page(store, start)?;
        let offset = (at - start) as usize;
        self.data.data[i * PAGE + offset..i * PAGE + offset + 19].copy_from_slice(bytes);
        if !self.slots.data[i].dirty {
            self.slots.data[i].dirty = true;
            self.dirty += 1;
        }
        Ok(())
    }
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty != 0
    }
    pub(super) fn is_valid(&self) -> bool {
        self.valid != 0
    }
}
