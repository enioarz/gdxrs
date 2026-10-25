use crate::{
    budget::{Buffer, Reservation},
    metadata,
    model::Bytes,
    source::Source,
    stream::{Checkpoint, Stream},
    *,
};
use std::{mem::size_of, sync::Arc};

/// Maximum variable payload, including temporary sort indices, for small lookups.
pub const SMALL_LABEL_LOOKUP_BYTES: usize = 128 * 1024;
const PAGE_LABELS: usize = 128;
fn increment(counter: &mut u64) -> Result<()> {
    *counter = counter
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "metadata counter overflow"))?;
    Ok(())
}
struct Request {
    start: usize,
    length: usize,
    id: Option<UelId>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LabelLookupStatistics {
    pub table_traversals: u64,
    pub uel_entries_decoded: u64,
}
pub struct ResolvedLabel<'a> {
    pub ordinal: usize,
    pub label: &'a [u8],
    pub id: Option<UelId>,
}
/// Ordered, owned, root-budgeted results. Unknown labels have no ID.
pub struct LabelMatches {
    source: Arc<Source>,
    bytes: Buffer<u8>,
    requests: Buffer<Request>,
    statistics: LabelLookupStatistics,
    _state: Reservation,
}
impl LabelMatches {
    pub fn len(&self) -> usize {
        self.requests.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, ordinal: usize) -> Option<ResolvedLabel<'_>> {
        let r = self.requests.data.get(ordinal)?;
        Some(ResolvedLabel {
            ordinal,
            label: self.bytes.data.get(r.start..r.start + r.length)?,
            id: r.id,
        })
    }
    pub fn statistics(&self) -> LabelLookupStatistics {
        self.statistics
    }
    fn bytes_for(&self, ordinal: usize) -> &[u8] {
        // Ordinals come only from the initialized in-place index buffer; all
        // descriptor bounds were checked before copying caller-owned bytes.
        let r = &self.requests.data[ordinal];
        &self.bytes.data[r.start..r.start + r.length]
    }
}
impl Reader {
    pub fn lookup_labels(&self, labels: &[&[u8]]) -> Result<LabelMatches> {
        let mut bytes = 0usize;
        for label in labels {
            if label.len() > 255 {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "label exceeds encoded string maximum",
                ));
            }
            bytes = bytes.checked_add(label.len()).ok_or_else(|| {
                Error::new(ErrorKind::BudgetTooSmall, "label request layout overflow")
            })?;
        }
        let layout = labels
            .len()
            .checked_mul(size_of::<Request>() + size_of::<usize>())
            .and_then(|n| n.checked_add(bytes))
            .ok_or_else(|| {
                Error::new(ErrorKind::BudgetTooSmall, "label request layout overflow")
            })?;
        if layout > SMALL_LABEL_LOOKUP_BYTES {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "small label lookup allowance exceeded; spill path required",
            ));
        }
        if size_of::<LabelMatches>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "lookup fixed state exceeds allowance",
            ));
        }
        let fixed = self.source.budget.reserve(16 * 1024)?;
        let mut result = LabelMatches {
            source: Arc::clone(&self.source),
            bytes: Buffer::new(&self.source.budget, bytes)?,
            requests: Buffer::new(&self.source.budget, labels.len())?,
            statistics: LabelLookupStatistics::default(),
            _state: fixed,
        };
        let mut indices = Buffer::new(&self.source.budget, labels.len())?;
        for (ordinal, label) in labels.iter().enumerate() {
            result.requests.push(Request {
                start: result.bytes.data.len(),
                length: label.len(),
                id: None,
            })?;
            for &byte in *label {
                result.bytes.push(byte)?;
            }
            indices.push(ordinal)?;
        }
        if labels.is_empty() {
            return Ok(result);
        }
        indices
            .data
            .sort_unstable_by(|&a, &b| result.bytes_for(a).cmp(result.bytes_for(b)));
        let mut cursor = self.uels()?;
        result.statistics.table_traversals = 1;
        while let Some(entry) = cursor.next()? {
            increment(&mut result.statistics.uel_entries_decoded)?;
            let at = indices
                .data
                .partition_point(|&i| result.bytes_for(i) < entry.label);
            for &ordinal in &indices.data[at..] {
                if result.bytes_for(ordinal) != entry.label {
                    break;
                }
                if result.requests.data[ordinal].id.is_none() {
                    result.requests.data[ordinal].id = Some(entry.id);
                }
            }
        }
        Ok(result)
    }
    pub fn selection_from_labels(
        &self,
        symbol: &SymbolHandle,
        dimension: u8,
        labels: &LabelMatches,
    ) -> Result<Selection> {
        self.check_handle(symbol)?;
        if !Arc::ptr_eq(&self.source, &labels.source) {
            return Err(
                Error::new(ErrorKind::SourceMismatch, "labels belong to another reader")
                    .symbol(symbol.id().get()),
            );
        }
        if dimension >= self.resolve(&symbol.inner.record)?.dimension {
            return Err(Error::new(
                ErrorKind::InvalidSelection,
                "label selection dimension outside symbol",
            )
            .symbol(symbol.id().get()));
        }
        let count = labels
            .requests
            .data
            .iter()
            .filter(|r| r.id.is_some())
            .count();
        let mut ids = Buffer::new(&self.source.budget, count)?;
        for id in labels.requests.data.iter().filter_map(|r| r.id) {
            ids.push(id)?;
        }
        self.selection(
            symbol,
            &[DimensionSelection {
                dimension,
                keys: KeySelection::AnyOf(&ids.data),
            }],
        )
    }
    pub fn uel_labels(&self) -> Result<UelLabelCursor> {
        UelLabelCursor::new(Arc::clone(&self.source))
    }
    pub fn metadata_cache_statistics(&self) -> BudgetStatistics {
        self.source.metadata_budget.statistics()
    }
}

struct Page {
    first: u32,
    next: Checkpoint,
    labels: Buffer<Bytes>,
    _cache: Reservation,
}
impl Page {
    fn next_id(&self) -> u32 {
        self.first + self.labels.data.len() as u32
    }
    fn contains(&self, id: u32) -> bool {
        self.first <= id && id < self.next_id()
    }
    fn charge() -> usize {
        PAGE_LABELS * size_of::<Bytes>() + size_of::<Option<Self>>()
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UelLabelStatistics {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub evictions: u64,
    pub uel_entries_decoded: u64,
    pub table_restarts: u64,
    pub checkpoint_restarts: u64,
}
enum Location {
    Page(usize, usize),
    Single,
}
/// Lending label lookup with at most two cached pages, under a source-wide cap.
///
/// ```compile_fail,E0499
/// # use gdxrs::{UelLabelCursor, UelId};
/// # fn example(cursor: &mut UelLabelCursor, a: UelId, b: UelId) -> gdxrs::Result<()> {
/// let first = cursor.get(a)?;
/// let second = cursor.get(b)?;
/// println!("{first:?}");
/// # Ok(()) }
/// ```
pub struct UelLabelCursor {
    source: Arc<Source>,
    pages: [Option<Page>; 2],
    mru: usize,
    single: Bytes,
    statistics: UelLabelStatistics,
    error: Option<Error>,
    _state: Reservation,
}
impl UelLabelCursor {
    fn new(source: Arc<Source>) -> Result<Self> {
        if size_of::<Self>() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "label cursor fixed state exceeds allowance",
            ));
        }
        let state = source.budget.reserve(16 * 1024)?;
        Ok(Self {
            source,
            pages: [None, None],
            mru: 0,
            single: Bytes::default(),
            statistics: UelLabelStatistics::default(),
            error: None,
            _state: state,
        })
    }
    pub fn statistics(&self) -> UelLabelStatistics {
        self.statistics
    }
    fn evict(&mut self, slot: usize) -> Result<()> {
        if self.pages[slot].is_some() {
            increment(&mut self.statistics.evictions)?;
            self.pages[slot] = None;
        }
        Ok(())
    }
    fn prepare_page(&mut self) -> Result<Option<(usize, Buffer<Bytes>, Reservation)>> {
        let charge = Page::charge();
        if self.source.options.metadata_cache_bytes < charge {
            return Ok(None);
        }
        let slot = self
            .pages
            .iter()
            .position(Option::is_none)
            .unwrap_or(1 - self.mru);
        self.evict(slot)?;
        let cache = match self.source.metadata_budget.reserve(charge) {
            Ok(r) => r,
            Err(e) if e.kind == ErrorKind::BudgetTooSmall => {
                self.evict(1 - slot)?;
                match self.source.metadata_budget.reserve(charge) {
                    Ok(r) => r,
                    Err(e) if e.kind == ErrorKind::BudgetTooSmall => return Ok(None),
                    Err(e) => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };
        match Buffer::new(&self.source.budget, PAGE_LABELS) {
            Ok(buffer) => Ok(Some((slot, buffer, cache))),
            Err(e) if e.kind == ErrorKind::BudgetTooSmall => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn label(&mut self, stream: &mut Stream) -> Result<Bytes> {
        let label = stream.string()?;
        increment(&mut self.statistics.uel_entries_decoded)?;
        Ok(label)
    }
    fn load(&mut self, id: UelId) -> Result<Location> {
        self.source.check_unchanged()?;
        let count = self.source.counts()?.uels;
        let id = id.get();
        if id > count {
            return Err(Error::new(
                ErrorKind::InvalidReference,
                "UEL ID outside source table",
            ));
        }
        if let Some(slot) = self
            .pages
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| p.contains(id)))
        {
            increment(&mut self.statistics.cache_hits)?;
            self.mru = slot;
            let first = self.pages[slot]
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::InvalidReference, "missing cached page"))?
                .first;
            return Ok(Location::Page(slot, (id - first) as usize));
        }
        increment(&mut self.statistics.cache_misses)?;
        let first = (id - 1) / PAGE_LABELS as u32 * PAGE_LABELS as u32 + 1;
        // Capture a complete string-table checkpoint before optional LRU eviction.
        let predecessor = self
            .pages
            .iter()
            .flatten()
            .filter(|p| p.next_id() <= first)
            .max_by_key(|p| p.next_id())
            .map(|p| (p.next_id(), p.next));
        let page = self.prepare_page()?;
        let h = self.source.header()?;
        let (_, end) = h.section(1, self.source.length)?;
        let (mut next, mut stream) = if let Some((next, point)) = predecessor {
            increment(&mut self.statistics.checkpoint_restarts)?;
            (
                next,
                Stream::resume(
                    Arc::clone(&self.source),
                    point,
                    end,
                    h.compressed,
                    h.order,
                    "UEL labels",
                )?,
            )
        } else {
            increment(&mut self.statistics.table_restarts)?;
            let mut s = metadata::section(Arc::clone(&self.source), 1, "UEL labels")?;
            s.marker(b"_UEL_")?;
            if s.count()? != count {
                return Err(s.error(ErrorKind::SourceChanged, "UEL count changed"));
            }
            (1, s)
        };
        let target = if page.is_some() { first } else { id };
        while next < target {
            self.label(&mut stream)?;
            next += 1;
        }
        if let Some((slot, mut labels, cache)) = page {
            let number = (count - first + 1).min(PAGE_LABELS as u32);
            for _ in 0..number {
                labels.push(self.label(&mut stream)?)?;
            }
            let point = stream.checkpoint();
            if first + number > count {
                stream.marker(b"_UEL_")?;
            }
            self.pages[slot] = Some(Page {
                first,
                next: point,
                labels,
                _cache: cache,
            });
            self.mru = slot;
            Ok(Location::Page(slot, (id - first) as usize))
        } else {
            self.single = self.label(&mut stream)?;
            if id == count {
                stream.marker(b"_UEL_")?;
            }
            Ok(Location::Single)
        }
    }
    pub fn get(&mut self, id: UelId) -> Result<&[u8]> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let location = match self.load(id) {
            Ok(location) => location,
            Err(mut e) => {
                e.source_id = Some(self.source.id);
                self.pages = [None, None];
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        match location {
            Location::Single => Ok(self.single.as_slice()),
            Location::Page(slot, offset) => self
                .pages
                .get(slot)
                .and_then(Option::as_ref)
                .and_then(|p| p.labels.data.get(offset))
                .map(Bytes::as_slice)
                .ok_or_else(|| Error::new(ErrorKind::InvalidReference, "cached label missing")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ReadAt;
    use std::{
        io,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };
    type ReadRanges = Arc<Mutex<Vec<(u64, usize)>>>;
    struct Spy {
        bytes: Vec<u8>,
        fail: Arc<AtomicBool>,
        ranges: Arc<Mutex<Vec<(u64, usize)>>>,
    }
    impl ReadAt for Spy {
        fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "metadata fault",
                ));
            }
            let rest = self.bytes.get(at as usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len()).min(7);
            out[..n].copy_from_slice(&rest[..n]);
            self.ranges.lock().unwrap().push((at, n));
            Ok(n)
        }
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    fn reader() -> (Reader, Arc<AtomicBool>, ReadRanges) {
        let bytes = std::fs::read("tests/fixtures/v7/key-widths.gdx").unwrap();
        let len = bytes.len() as u64;
        let fail = Arc::new(AtomicBool::new(false));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let source = Source::testing(
            Box::new(Spy {
                bytes,
                fail: Arc::clone(&fail),
                ranges: Arc::clone(&ranges),
            }),
            len,
        );
        (Reader::from_source(source).unwrap(), fail, ranges)
    }
    #[test]
    fn lookup_and_pages_never_read_record_regions_and_release_all_owners() {
        let (reader, _, ranges) = reader();
        let budget = Arc::clone(&reader.source.budget);
        let h = reader.source.header().unwrap();
        let data = (h.data_start, h.offsets[0]);
        ranges.lock().unwrap().clear();
        let found = reader
            .lookup_labels(&[b"u65536", b"u1", b"missing"])
            .unwrap();
        assert_eq!(
            found.statistics(),
            LabelLookupStatistics {
                table_traversals: 1,
                uel_entries_decoded: 65536
            }
        );
        let mut cursor = reader.uel_labels().unwrap();
        assert_eq!(cursor.get(UelId::new(65536).unwrap()).unwrap(), b"u65536");
        assert!(
            ranges
                .lock()
                .unwrap()
                .iter()
                .all(|(at, n)| *at + *n as u64 <= data.0 || *at >= data.1)
        );
        let cache = reader.metadata_cache_statistics();
        assert_eq!(cache.current_bytes, Page::charge());
        drop(reader);
        drop(cursor);
        assert!(budget.statistics().current_bytes > 0);
        assert_eq!(found.get(0).unwrap().id, Some(UelId::new(65536).unwrap()));
        drop(found);
        assert_eq!(budget.statistics().current_bytes, 0);
        eprintln!(
            "M2 labels: page_bytes={}, cursor_bytes={}, lookup_bytes={}, peak_root={}",
            Page::charge(),
            size_of::<UelLabelCursor>(),
            size_of::<LabelMatches>(),
            budget.statistics().peak_bytes
        );
    }
    #[test]
    fn page_io_allocation_and_reference_failures_are_terminal_and_free_cache() {
        for failure in [0, 1, 2] {
            let (reader, fail, ranges) = reader();
            let baseline = reader.budget_statistics().current_bytes;
            let mut cursor = reader.uel_labels().unwrap();
            cursor.get(UelId::new(1).unwrap()).unwrap();
            assert_eq!(
                reader.metadata_cache_statistics().current_bytes,
                Page::charge()
            );
            let decoded = cursor.statistics().uel_entries_decoded;
            let (key, expected) = match failure {
                0 => {
                    fail.store(true, Ordering::Relaxed);
                    (129, ErrorKind::Io)
                }
                1 => {
                    crate::budget::inject_buffer_failure();
                    (129, ErrorKind::AllocationFailed)
                }
                _ => (65537, ErrorKind::InvalidReference),
            };
            let error = cursor.get(UelId::new(key).unwrap()).err().unwrap();
            assert_eq!(error.kind, expected);
            if failure == 0 {
                assert!(std::error::Error::source(&error).is_some());
                assert!(error.physical_offset.is_some());
            }
            assert_eq!(error.source_id, Some(reader.source.id));
            let calls = ranges.lock().unwrap().len();
            assert_eq!(
                cursor.get(UelId::new(1).unwrap()).err().unwrap().kind,
                expected
            );
            assert_eq!(ranges.lock().unwrap().len(), calls);
            assert_eq!(cursor.statistics().uel_entries_decoded, decoded);
            assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
            assert_eq!(
                reader.budget_statistics().current_bytes,
                baseline + 16 * 1024
            );
            drop(cursor);
            assert_eq!(reader.budget_statistics().current_bytes, baseline);
        }
    }
    #[test]
    fn duplicate_stored_bytes_choose_first_id_and_bad_closing_marker_discards_results() {
        let bytes = std::fs::read("tests/fixtures/v7/case-distinct-labels.gdx").unwrap();
        let uels = u64::from_le_bytes(bytes[126..134].try_into().unwrap()) as usize;
        let end = u64::from_le_bytes(bytes[142..150].try_into().unwrap()) as usize;
        for corrupt_end in [false, true] {
            let mut input = bytes.clone();
            assert_eq!(input[uels + 15], b'A');
            input[uels + 15] = b'a';
            if corrupt_end {
                input[end - 1] ^= 1;
            }
            let len = input.len() as u64;
            let source = Source::testing(
                Box::new(Spy {
                    bytes: input,
                    fail: Arc::new(AtomicBool::new(false)),
                    ranges: Arc::new(Mutex::new(Vec::new())),
                }),
                len,
            );
            let reader = Reader::from_source(source).unwrap();
            let baseline = reader.budget_statistics().current_bytes;
            if corrupt_end {
                assert_eq!(
                    reader.lookup_labels(&[b"a"]).err().unwrap().kind,
                    ErrorKind::InvalidHeader
                );
            } else {
                let result = reader.lookup_labels(&[b"a", b"A"]).unwrap();
                assert_eq!(result.get(0).unwrap().id, Some(UelId::new(2).unwrap()));
                assert_eq!(result.get(1).unwrap().id, None);
            }
            assert_eq!(reader.budget_statistics().current_bytes, baseline);
        }
    }
    #[test]
    fn exact_small_request_allowance_boundary_is_enforced_before_traversal() {
        let (reader, _, ranges) = reader();
        let count = SMALL_LABEL_LOOKUP_BYTES / (size_of::<Request>() + size_of::<usize>());
        let labels = vec![b"".as_slice(); count + 1];
        let result = reader.lookup_labels(&labels[..count]).unwrap();
        assert_eq!(result.len(), count);
        assert_eq!(result.get(count - 1).unwrap().id, None);
        drop(result);
        let calls = ranges.lock().unwrap().len();
        let baseline = reader.budget_statistics().current_bytes;
        assert_eq!(
            reader.lookup_labels(&labels).err().unwrap().kind,
            ErrorKind::BudgetTooSmall
        );
        assert_eq!(ranges.lock().unwrap().len(), calls);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
    #[test]
    fn batch_lookup_allocation_failure_does_not_escape_or_leak() {
        let (reader, _, _) = reader();
        let baseline = reader.budget_statistics().current_bytes;
        crate::budget::inject_buffer_failure();
        assert_eq!(
            reader.lookup_labels(&[b"u1"]).err().unwrap().kind,
            ErrorKind::AllocationFailed
        );
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
        assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
    }
    #[test]
    fn root_exhaustion_is_terminal_and_releases_partial_cache_admission() {
        let (reader, _, _) = reader();
        // Leave only the cursor allowance and 100 bytes. Optional page storage
        // is denied, then required decoder storage must fail without raising caps.
        let baseline = reader.budget_statistics().current_bytes;
        let remaining = reader.source.budget.statistics().limit_bytes - baseline - 16 * 1024 - 100;
        let held = reader.source.budget.reserve(remaining).unwrap();
        let mut cursor = reader.uel_labels().unwrap();
        assert_eq!(
            cursor.get(UelId::new(1).unwrap()).err().unwrap().kind,
            ErrorKind::BudgetTooSmall
        );
        assert_eq!(reader.metadata_cache_statistics().current_bytes, 0);
        drop(cursor);
        drop(held);
        assert_eq!(reader.budget_statistics().current_bytes, baseline);
    }
}
