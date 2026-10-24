use crate::{
    AcronymId, Error, ErrorKind, Result, SymbolId, SymbolType, UelId,
    budget::Reservation,
    model::Bytes,
    reader::Reader,
    source::Source,
    stream::{Checkpoint, Stream},
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct SymbolRecord {
    pub id: u32,
    pub name: Bytes,
    pub description: Bytes,
    pub dimension: u8,
    pub kind: SymbolType,
    pub user: i32,
    pub position: u64,
    pub count: u32,
    pub errors: u32,
    pub has_text: bool,
    pub compressed: bool,
    pub strict: bool,
    pub domains: [u32; 20],
    pub comment_count: u32,
    pub comments: Checkpoint,
}
impl SymbolRecord {
    pub fn universe() -> Self {
        Self {
            id: 0,
            name: Bytes::literal(b"*"),
            description: Bytes::literal(b"Universe"),
            dimension: 1,
            kind: SymbolType::Set,
            user: 0,
            position: 0,
            count: 0,
            errors: 0,
            has_text: false,
            compressed: false,
            strict: false,
            domains: [0; 20],
            comment_count: 0,
            comments: Checkpoint::default(),
        }
    }
}
fn nonnegative(stream: &mut Stream) -> Result<u32> {
    u32::try_from(stream.i32()?)
        .map_err(|_| stream.error(ErrorKind::InvalidCount, "negative metadata count"))
}

pub(crate) struct Catalog {
    pub stream: Stream,
    source: Arc<Source>,
    remaining: u32,
    index: u32,
    pending_comments: u32,
    done: bool,
}
impl Catalog {
    pub fn new(source: Arc<Source>) -> Result<Self> {
        let mut stream = section(Arc::clone(&source), 0, "symbols")?;
        stream.marker(b"_SYMB_")?;
        let count = stream.count()?;
        if count != source.counts()?.symbols {
            return Err(stream.error(ErrorKind::SourceChanged, "catalog count changed"));
        }
        Ok(Self {
            stream,
            source,
            remaining: count,
            index: 0,
            pending_comments: 0,
            done: false,
        })
    }
    pub fn next_record(&mut self) -> Result<Option<SymbolRecord>> {
        if self.done {
            return Ok(None);
        }
        for _ in 0..self.pending_comments {
            self.stream.string()?;
        }
        self.pending_comments = 0;
        if self.remaining == 0 {
            self.stream.marker(b"_SYMB_")?;
            self.done = true;
            return Ok(None);
        }
        let s = &mut self.stream;
        let name = s.string()?;
        let position = u64::try_from(s.i64()?)
            .map_err(|_| s.error(ErrorKind::InvalidOffset, "negative data position"))?;
        let dimension = u8::try_from(s.i32()?)
            .ok()
            .filter(|v| *v <= 20)
            .ok_or_else(|| s.error(ErrorKind::InvalidDimension, "invalid symbol dimension"))?;
        let kind = SymbolType::decode(s.byte()?).map_err(|e| s.context(e))?;
        let user = s.i32()?;
        let count = nonnegative(s)?;
        let errors = nonnegative(s)?;
        let has_text = s.boolean()?;
        let description = s.string()?;
        let compressed = s.boolean()?;
        let strict = s.boolean()?;
        let mut domains = [0; 20];
        if strict {
            for slot in domains.iter_mut().take(usize::from(dimension)) {
                *slot = u32::try_from(s.i32()?)
                    .map_err(|_| s.error(ErrorKind::InvalidReference, "negative domain ID"))?;
                if *slot > self.source.counts()?.symbols {
                    return Err(s.error(ErrorKind::InvalidReference, "domain ID outside catalog"));
                }
            }
        }
        let comment_count = s.count()?;
        let comments = s.checkpoint();
        let header = self.source.header()?;
        if kind == SymbolType::Alias {
            if position != 0
                || count != 0
                || user < 0
                || user as u32 > self.source.counts()?.symbols
            {
                return Err(s.error(ErrorKind::InvalidReference, "invalid alias metadata"));
            }
        } else if position < header.data_start || position >= header.offsets[0] {
            return Err(s.error(ErrorKind::InvalidOffset, "symbol data outside data region"));
        }
        if dimension == 0 && kind != SymbolType::Alias && count > 1 {
            return Err(s.error(
                ErrorKind::InvalidCount,
                "scalar has multiple physical records",
            ));
        }
        self.index += 1;
        self.remaining -= 1;
        self.pending_comments = comment_count;
        Ok(Some(SymbolRecord {
            id: self.index,
            name,
            description,
            dimension,
            kind,
            user,
            position,
            count,
            errors,
            has_text,
            compressed,
            strict,
            domains,
            comment_count,
            comments,
        }))
    }
}
pub(crate) fn section(source: Arc<Source>, index: usize, label: &'static str) -> Result<Stream> {
    let h = source.header()?;
    let (start, end) = h.section(index, source.length)?;
    let (compressed, order) = (h.compressed, h.order);
    Stream::new(source, start, end, compressed, order, label)
}

pub struct SymbolMetadata<'a> {
    pub id: SymbolId,
    pub name: &'a [u8],
    pub description: &'a [u8],
    pub dimension: u8,
    pub kind: SymbolType,
    pub user_information: i32,
    pub physical_record_count: u64,
    pub raw_record_count: u64,
    pub error_count: u32,
    pub has_set_text: bool,
    pub compressed: bool,
    pub comment_count: u32,
}
pub(crate) fn metadata_view(record: &SymbolRecord, raw_count: u64) -> Result<SymbolMetadata<'_>> {
    Ok(SymbolMetadata {
        id: SymbolId::new(record.id)?,
        name: record.name.as_slice(),
        description: record.description.as_slice(),
        dimension: record.dimension,
        kind: record.kind,
        user_information: record.user,
        physical_record_count: u64::from(record.count),
        raw_record_count: raw_count,
        error_count: record.errors,
        has_set_text: record.has_text,
        compressed: record.compressed,
        comment_count: record.comment_count,
    })
}
pub struct SymbolCursor {
    reader: Reader,
    catalog: Catalog,
    last: Option<SymbolRecord>,
    error: Option<Error>,
}
impl SymbolCursor {
    pub(crate) fn new(reader: Reader) -> Result<Self> {
        let catalog = Catalog::new(Arc::clone(&reader.source))?;
        Ok(Self {
            reader,
            catalog,
            last: None,
            error: None,
        })
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<SymbolMetadata<'_>>> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let record = match self.catalog.next_record() {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        let Some(record) = record else {
            self.last = None;
            return Ok(None);
        };
        let raw = match self.reader.raw_count(&record) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        self.last = Some(record);
        self.last
            .as_ref()
            .map(|r| metadata_view(r, raw))
            .transpose()
    }
}
pub(crate) struct HandleState {
    pub id: SymbolId,
    pub source: Arc<Source>,
    pub record: SymbolRecord,
    pub _reservation: Reservation,
}
#[derive(Clone)]
pub struct SymbolHandle {
    pub(crate) inner: Arc<HandleState>,
}
impl SymbolHandle {
    pub fn metadata(&self) -> Result<SymbolMetadata<'_>> {
        let reader = Reader {
            source: Arc::clone(&self.inner.source),
        };
        metadata_view(&self.inner.record, reader.raw_count(&self.inner.record)?)
    }
    pub fn id(&self) -> SymbolId {
        self.inner.id
    }
}

struct Strings {
    stream: Stream,
    remaining: u32,
    next: u32,
    last: Bytes,
    marker: &'static [u8],
    done: bool,
    error: Option<Error>,
}
impl Strings {
    fn new(source: Arc<Source>, index: usize, marker: &'static [u8], first: u32) -> Result<Self> {
        let mut stream = section(source, index, "string table")?;
        stream.marker(marker)?;
        let remaining = stream.count()?;
        Ok(Self {
            stream,
            remaining,
            next: first,
            last: Bytes::default(),
            marker,
            done: false,
            error: None,
        })
    }
    fn next(&mut self) -> Result<Option<(u32, &[u8])>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.done {
            return Ok(None);
        }
        if self.remaining == 0 {
            if let Err(e) = self.stream.marker(self.marker) {
                self.error = Some(e.clone());
                return Err(e);
            }
            self.done = true;
            return Ok(None);
        }
        self.last = match self.stream.string() {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        let id = self.next;
        self.next += 1;
        self.remaining -= 1;
        Ok(Some((id, self.last.as_slice())))
    }
}
pub struct UelEntry<'a> {
    pub id: UelId,
    pub label: &'a [u8],
}
pub struct UelCursor {
    strings: Strings,
}
impl UelCursor {
    pub(crate) fn new(source: Arc<Source>) -> Result<Self> {
        let count = source.counts()?.uels;
        let strings = Strings::new(source, 1, b"_UEL_", 1)?;
        if strings.remaining != count {
            return Err(strings
                .stream
                .error(ErrorKind::SourceChanged, "UEL count changed"));
        }
        Ok(Self { strings })
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<UelEntry<'_>>> {
        self.strings
            .next()?
            .map(|(id, label)| {
                Ok(UelEntry {
                    id: UelId::new(id)?,
                    label,
                })
            })
            .transpose()
    }
}
pub struct SetTextView<'a> {
    pub id: u32,
    pub text: &'a [u8],
}
pub struct SetTextCursor {
    strings: Strings,
}
impl SetTextCursor {
    pub(crate) fn new(source: Arc<Source>) -> Result<Self> {
        Ok(Self {
            strings: Strings::new(source, 2, b"_SETT_", 0)?,
        })
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<SetTextView<'_>>> {
        Ok(self
            .strings
            .next()?
            .map(|(id, text)| SetTextView { id, text }))
    }
}
pub struct CommentView<'a> {
    pub ordinal: u32,
    pub text: &'a [u8],
}
pub struct CommentCursor {
    stream: Option<Stream>,
    remaining: u32,
    next: u32,
    last: Bytes,
    error: Option<Error>,
    _state: Reservation,
}
impl CommentCursor {
    pub(crate) fn new(source: Arc<Source>, record: &SymbolRecord) -> Result<Self> {
        let state = source.budget.reserve(16 * 1024)?;
        let stream = if record.comment_count == 0 {
            None
        } else {
            let h = source.header()?;
            let (_, end) = h.section(0, source.length)?;
            Some(Stream::resume(
                Arc::clone(&source),
                record.comments,
                end,
                h.compressed,
                h.order,
                "comments",
            )?)
        };
        Ok(Self {
            stream,
            remaining: record.comment_count,
            next: 1,
            last: Bytes::default(),
            error: None,
            _state: state,
        })
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<CommentView<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.remaining == 0 {
            return Ok(None);
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "missing comment stream"))?;
        self.last = match stream.string() {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        let ordinal = self.next;
        self.next += 1;
        self.remaining -= 1;
        Ok(Some(CommentView {
            ordinal,
            text: self.last.as_slice(),
        }))
    }
}
pub struct AcronymView<'a> {
    pub ordinal: u32,
    pub id: AcronymId,
    pub name: &'a [u8],
    pub text: &'a [u8],
}
pub struct AcronymCursor {
    stream: Stream,
    remaining: u32,
    next: u32,
    name: Bytes,
    text: Bytes,
    error: Option<Error>,
    done: bool,
}
impl AcronymCursor {
    pub(crate) fn new(source: Arc<Source>) -> Result<Self> {
        let mut stream = section(source, 3, "acronyms")?;
        stream.marker(b"_ACRO_")?;
        let remaining = stream.count()?;
        Ok(Self {
            stream,
            remaining,
            next: 1,
            name: Bytes::default(),
            text: Bytes::default(),
            error: None,
            done: false,
        })
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<AcronymView<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.done {
            return Ok(None);
        }
        if self.remaining == 0 {
            if let Err(e) = self.stream.marker(b"_ACRO_") {
                self.error = Some(e.clone());
                return Err(e);
            }
            self.done = true;
            return Ok(None);
        }
        let result = (|| {
            self.name = self.stream.string()?;
            self.text = self.stream.string()?;
            let raw = u32::try_from(self.stream.i32()?).map_err(|_| {
                self.stream
                    .error(ErrorKind::InvalidAcronymId, "negative acronym ID")
            })?;
            AcronymId::new(raw).map_err(|e| self.stream.context(e))
        })();
        let id = match result {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        let ordinal = self.next;
        self.next += 1;
        self.remaining -= 1;
        Ok(Some(AcronymView {
            ordinal,
            id,
            name: self.name.as_slice(),
            text: self.text.as_slice(),
        }))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainMode {
    Implicit,
    Relaxed,
    Strict,
}
impl DomainMode {
    pub const fn code(self) -> u8 {
        match self {
            Self::Implicit => 1,
            Self::Relaxed => 2,
            Self::Strict => 3,
        }
    }
}
pub struct DomainView<'a> {
    pub dimension: u8,
    pub strict_symbol: Option<SymbolId>,
    pub relaxed_name: Option<&'a [u8]>,
    pub name: &'a [u8],
}
pub struct DomainCursor {
    reader: Reader,
    record: SymbolRecord,
    mode: DomainMode,
    relaxed: [u32; 20],
    next: u8,
    name: Bytes,
    error: Option<Error>,
    _state: Reservation,
}
impl DomainCursor {
    pub(crate) fn new(reader: Reader, record: SymbolRecord) -> Result<Self> {
        let state = reader.source.budget.reserve(16 * 1024)?;
        let mut mode = if record.strict {
            DomainMode::Strict
        } else {
            DomainMode::Implicit
        };
        let mut relaxed = [0; 20];
        if reader.source.header()?.offsets[5] != 0 && record.id != 0 {
            let mut stream = section(Arc::clone(&reader.source), 5, "domains")?;
            stream.marker(b"_DOMS_")?;
            let names = stream.count()?;
            for _ in 0..names {
                stream.string()?;
            }
            stream.marker(b"_DOMS_")?;
            let mut seen = 0u64;
            loop {
                let id = stream.i32()?;
                if id <= 0 {
                    stream.marker(b"_DOMS_")?;
                    break;
                }
                seen += 1;
                if seen > u64::from(reader.source.counts()?.symbols) {
                    return Err(
                        stream.error(ErrorKind::InvalidCount, "too many relaxed domain mappings")
                    );
                }
                let current = reader.lookup_record(SymbolId::new(id as u32)?)?;
                for d in 0..usize::from(current.dimension) {
                    let index = u32::try_from(stream.i32()?).map_err(|_| {
                        stream.error(ErrorKind::InvalidReference, "negative domain-name ID")
                    })?;
                    if index > names {
                        return Err(stream
                            .error(ErrorKind::InvalidReference, "domain-name ID outside table"));
                    }
                    if current.id == record.id {
                        relaxed[d] = index;
                        mode = DomainMode::Relaxed;
                    }
                }
            }
        }
        Ok(Self {
            reader,
            record,
            mode,
            relaxed,
            next: 0,
            name: Bytes::default(),
            error: None,
            _state: state,
        })
    }
    pub fn mode(&self) -> DomainMode {
        self.mode
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<DomainView<'_>>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.next >= self.record.dimension {
            return Ok(None);
        }
        let d = usize::from(self.next);
        let strict = if self.record.strict {
            Some(SymbolId::new(self.record.domains[d])?)
        } else {
            None
        };
        let result = (|| {
            if let Some(id) = strict {
                let target = self.reader.resolve(&self.reader.lookup_record(id)?)?;
                if target.kind != SymbolType::Set {
                    return Err(Error::new(
                        ErrorKind::InvalidReference,
                        "strict domain is not a set",
                    ));
                }
            }
            if self.mode == DomainMode::Relaxed {
                if self.relaxed[d] == 0 {
                    Bytes::from_slice(b"*")
                } else {
                    let mut table = Strings::new(Arc::clone(&self.reader.source), 5, b"_DOMS_", 1)?;
                    loop {
                        let Some((id, bytes)) = table.next()? else {
                            return Err(Error::new(
                                ErrorKind::InvalidReference,
                                "missing relaxed domain name",
                            ));
                        };
                        if id == self.relaxed[d] {
                            return Bytes::from_slice(bytes);
                        }
                    }
                }
            } else if let Some(id) = strict {
                if id == SymbolId::UNIVERSE {
                    Bytes::from_slice(b"*")
                } else {
                    Ok(self.reader.lookup_record(id)?.name)
                }
            } else {
                Bytes::from_slice(b"*")
            }
        })();
        self.name = match result {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.clone());
                return Err(e);
            }
        };
        let dimension = self.next;
        self.next += 1;
        let name = self.name.as_slice();
        Ok(Some(DomainView {
            dimension,
            strict_symbol: strict,
            relaxed_name: (self.mode == DomainMode::Relaxed).then_some(name),
            name,
        }))
    }
}
