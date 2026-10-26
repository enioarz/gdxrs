//! Disk-backed registration; controls/frames have fixed bounds.
use super::records::{DomainMembership, Domains, RecordSetup};
use super::store::{Frame, Store};
use super::*;
use crate::model::Bytes;

pub type MetadataOptions = SpillOptions;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriterUelId {
    owner: u64,
    id: UelId,
}
impl WriterUelId {
    pub fn get(self) -> u32 {
        self.id.get()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriterSymbolId {
    owner: u64,
    id: SymbolId,
}
impl WriterSymbolId {
    pub fn get(self) -> u32 {
        self.id.get()
    }
}
#[derive(Clone, Copy, Debug)]
pub enum SymbolReference<'a> {
    Universe,
    Registered(WriterSymbolId),
    Name(&'a [u8]),
}
#[derive(Clone, Copy, Debug)]
pub enum WriteDomain<'a> {
    Strict(SymbolReference<'a>),
    Relaxed(&'a [u8]),
}
#[derive(Clone, Debug)]
pub struct WriteSymbol<'a> {
    pub name: &'a [u8],
    pub description: &'a [u8],
    pub kind: SymbolType,
    pub dimension: u8,
    pub user_information: i32,
    pub alias_target: Option<SymbolReference<'a>>,
}
impl<'a> WriteSymbol<'a> {
    pub fn new(name: &'a [u8], kind: SymbolType, dimension: u8) -> Self {
        Self {
            name,
            description: b"",
            kind,
            dimension,
            user_information: match kind {
                SymbolType::Variable => 5,
                SymbolType::Equation => 53,
                _ => 0,
            },
            alias_target: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetadataStatistics {
    pub uels: u32,
    pub symbols: u32,
    pub set_texts: u32,
    pub acronyms: u32,
    pub frozen: bool,
    pub temporary_bytes: u64,
    pub peak_temporary_bytes: u64,
    pub open_files: u64,
    pub peak_open_files: u64,
    pub bulk_requests: u64,
    pub merge_passes: u64,
}
fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidOptions, message)
}
fn corrupt() -> Error {
    Error::new(ErrorKind::InvalidSpill, "invalid metadata journal")
}
fn next_id(n: u32) -> Result<u32> {
    n.checked_add(1)
        .filter(|n| *n <= i32::MAX as u32)
        .ok_or_else(|| Error::new(ErrorKind::InvalidCount, "metadata count exceeds v7 range"))
}
fn name(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes[0].is_ascii_alphabetic()
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return Err(invalid("invalid ASCII symbol/acronym identifier"));
    }
    Ok(())
}
fn text(bytes: &[u8]) -> Result<()> {
    if bytes.len() > 255 || bytes.contains(&0) {
        return Err(invalid("metadata text exceeds 255 bytes or contains NUL"));
    }
    Ok(())
}
fn label(bytes: &[u8]) -> Result<()> {
    if bytes.len() > 63
        || bytes.iter().any(|b| *b < 32)
        || bytes.contains(&b'\'') && bytes.contains(&b'"')
    {
        return Err(invalid(
            "UEL is not a native-compatible quotable <=63-byte string",
        ));
    }
    Ok(())
}
struct Packet {
    b: [u8; 1024],
    n: usize,
}
impl Packet {
    fn new() -> Self {
        Self { b: [0; 1024], n: 0 }
    }
    fn bytes(&mut self, b: &[u8]) -> Result<()> {
        let end = self
            .n
            .checked_add(b.len())
            .filter(|n| *n <= 1024)
            .ok_or_else(corrupt)?;
        self.b[self.n..end].copy_from_slice(b);
        self.n = end;
        Ok(())
    }
    fn u8(&mut self, n: u8) -> Result<()> {
        self.bytes(&[n])
    }
    fn u32(&mut self, n: u32) -> Result<()> {
        self.bytes(&n.to_le_bytes())
    }
    fn u64(&mut self, n: u64) -> Result<()> {
        self.bytes(&n.to_le_bytes())
    }
    fn string(&mut self, b: &[u8]) -> Result<()> {
        self.u8(u8::try_from(b.len()).map_err(|_| corrupt())?)?;
        self.bytes(b)
    }
    fn payload(&self) -> &[u8] {
        &self.b[..self.n]
    }
}
struct Parse<'a> {
    b: &'a [u8],
    at: usize,
}
impl<'a> Parse<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(corrupt)?;
        let s = self.b.get(self.at..end).ok_or_else(corrupt)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        let mut b = [0; 4];
        b.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(b))
    }
    fn u64(&mut self) -> Result<u64> {
        let mut b = [0; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(b))
    }
    fn string(&mut self) -> Result<Bytes> {
        let n = self.u8()? as usize;
        Bytes::from_slice(self.take(n)?)
    }
    fn done(&self) -> Result<()> {
        if self.at != self.b.len() {
            Err(corrupt())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone)]
struct Ref {
    id: u32,
    name: Bytes,
}
#[derive(Clone)]
struct Symbol {
    id: u32,
    name: Bytes,
    description: Bytes,
    kind: SymbolType,
    dim: u8,
    user: i32,
    target: Ref,
}
fn symbol(f: &Frame) -> Result<Symbol> {
    let mut p = Parse::new(f.payload());
    let id = p.u32()?;
    let name = p.string()?;
    let description = p.string()?;
    let kind = SymbolType::decode(p.u8()?)?;
    let dim = p.u8()?;
    let user = p.u32()? as i32;
    let target = Ref {
        id: p.u32()?,
        name: p.string()?,
    };
    p.done()?;
    Ok(Symbol {
        id,
        name,
        description,
        kind,
        dim,
        user,
        target,
    })
}
pub(super) struct Metadata {
    directory: PathBuf,
    journal: Option<Store>,
    owned: [bool; 3],
    budget: Arc<Budget>,
    options: SpillOptions,
    used: u64,
    peak: u64,
    owner: u64,
    stats: MetadataStatistics,
    domains: u32,
    comments: u32,
    sort_directory: Option<PathBuf>,
    sort_used: u64,
    sort_peak: u64,
    sort_handles: u64,
    cleaned: bool,
    _state: Reservation,
    _paths: Reservation,
}
impl Metadata {
    pub fn new(budget: Arc<Budget>, owner: u64, options: &MetadataOptions) -> Result<Self> {
        options.validate()?;
        if options.directory.as_os_str().len() > 4096
            || options.maximum_open_files < 6
            || !(2..=64).contains(&options.merge_fan_in)
        {
            return Err(invalid(
                "metadata needs a bounded scratch parent and at least six handles",
            ));
        }
        if size_of::<Self>() > CONTROL {
            return Err(invalid("metadata controls exceed allowance"));
        }
        let state = budget.reserve(CONTROL)?;
        let paths = budget.reserve(64 * 1024 + 8 * options.directory.as_os_str().len())?;
        let parent = fs::canonicalize(&options.directory).map_err(Error::io)?;
        if parent.as_os_str().len() > 4096 || parent.capacity() > 16384 || !parent.is_dir() {
            return Err(invalid("invalid metadata scratch parent"));
        }
        let mut limits = SpillOptions::new(&parent);
        limits.run_bytes = options.run_bytes;
        limits.maximum_temporary_bytes = options.maximum_temporary_bytes / 2;
        limits.maximum_open_files = options.maximum_open_files - 3;
        limits.merge_fan_in = options.merge_fan_in.min(limits.maximum_open_files - 1);
        let mut selected = None;
        for _ in 0..64 {
            let path = parent.join(format!(
                "gdxrs-metadata-{:08x}-{:016x}",
                std::process::id(),
                next_sequence()?
            ));
            let create = fs::DirBuilder::new();
            #[cfg(unix)]
            let create = {
                use std::os::unix::fs::DirBuilderExt;
                let mut create = create;
                create.mode(0o700);
                create
            };
            event(Event::MetadataCreate).map_err(Error::io)?;
            match create.create(&path) {
                Ok(()) => {
                    selected = Some(path);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(Error::io(e)),
            }
        }
        let directory = selected.ok_or_else(|| invalid("metadata scratch collisions"))?;
        let mut m = Self {
            directory,
            journal: None,
            owned: [false; 3],
            budget,
            options: limits,
            used: 0,
            peak: 0,
            owner,
            stats: MetadataStatistics {
                set_texts: 1,
                ..MetadataStatistics::default()
            },
            domains: 0,
            comments: 0,
            sort_directory: None,
            sort_used: 0,
            sort_peak: 0,
            sort_handles: 0,
            cleaned: false,
            _state: state,
            _paths: paths,
        };
        m.options.directory = m.directory.clone();
        Ok(m)
    }
    // Writer must retain the directory owner before this fallible allocation/open.
    fn initialize_journal(&mut self) -> Result<()> {
        self.journal = Some(Store::create(&self.path(0), &self.budget)?);
        self.owned[0] = true;
        self.stats.peak_open_files = 1;
        Ok(())
    }
    fn path(&self, n: usize) -> PathBuf {
        self.directory.join(match n {
            0 => "journal",
            1 => "requests",
            _ => "ordinals",
        })
    }
    fn j(&mut self) -> Result<&mut Store> {
        self.journal
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::InvalidWriteState, "metadata journal is closed"))
    }
    fn append(&mut self, tag: u8, p: &Packet) -> Result<()> {
        self.journal.as_mut().ok_or_else(corrupt)?.append(
            tag,
            p.payload(),
            &mut self.used,
            &mut self.peak,
            self.options.maximum_temporary_bytes,
        )
    }
    fn mutable(&self) -> Result<()> {
        if self.stats.frozen {
            Err(Error::new(
                ErrorKind::InvalidWriteState,
                "registration is frozen",
            ))
        } else {
            Ok(())
        }
    }
    pub fn statistics(&self) -> MetadataStatistics {
        MetadataStatistics {
            temporary_bytes: self.used + self.sort_used,
            peak_temporary_bytes: self.peak + self.sort_peak,
            open_files: u64::from(self.journal.is_some()),
            peak_open_files: self.stats.peak_open_files.max(if self.sort_handles > 0 {
                3 + self.sort_handles
            } else {
                0
            }),
            ..self.stats
        }
    }
    fn id(&self, id: u32) -> Result<WriterUelId> {
        Ok(WriterUelId {
            owner: self.owner,
            id: UelId::new(id)?,
        })
    }
    fn check(&self, id: WriterSymbolId) -> Result<u32> {
        if id.owner != self.owner || id.get() == 0 || id.get() > self.stats.symbols {
            Err(Error::new(
                ErrorKind::SourceMismatch,
                "symbol belongs to another writer",
            ))
        } else {
            Ok(id.get())
        }
    }
    fn find_string(&mut self, tag: u8, value: &[u8], fold: bool) -> Result<Option<u32>> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == tag {
                let mut p = Parse::new(f.payload());
                let id = p.u32()?;
                let label = p.string()?;
                if if fold {
                    label.as_slice().eq_ignore_ascii_case(value)
                } else {
                    label.as_slice() == value
                } {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }
    pub fn uel(&mut self, bytes: &[u8]) -> Result<WriterUelId> {
        self.mutable()?;
        label(bytes)?;
        if let Some(id) = self.find_string(1, bytes, false)? {
            return self.id(id);
        }
        let id = next_id(self.stats.uels)?;
        let mut p = Packet::new();
        p.u32(id)?;
        p.string(bytes)?;
        self.append(1, &p)?;
        self.stats.uels = id;
        self.id(id)
    }
    fn lookup(&mut self, id: u32) -> Result<Symbol> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 2 {
                let s = symbol(&f)?;
                if s.id == id {
                    return Ok(s);
                }
            }
        }
        Err(Error::new(
            ErrorKind::InvalidReference,
            "missing registered symbol",
        ))
    }
    fn reference(&self, r: SymbolReference<'_>) -> Result<Ref> {
        match r {
            SymbolReference::Universe => Ok(Ref {
                id: 0,
                name: Bytes::default(),
            }),
            SymbolReference::Registered(id) => Ok(Ref {
                id: self.check(id)?,
                name: Bytes::default(),
            }),
            SymbolReference::Name(n) => {
                name(n)?;
                Ok(Ref {
                    id: 0,
                    name: Bytes::from_slice(n)?,
                })
            }
        }
    }
    fn resolve_ref(&mut self, r: &Ref) -> Result<u32> {
        if r.name.as_slice().is_empty() {
            Ok(r.id)
        } else {
            self.find_string(2, r.name.as_slice(), true)?
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidReference,
                        "unresolved forward symbol name",
                    )
                })
        }
    }
    fn set_target(&mut self, r: &Ref) -> Result<u32> {
        let mut id = self.resolve_ref(r)?;
        for _ in 0..=self.stats.symbols {
            if id == 0 {
                return Ok(0);
            }
            let s = self.lookup(id)?;
            if s.kind != SymbolType::Alias {
                return if s.kind == SymbolType::Set && s.dim == 1 {
                    Ok(id)
                } else {
                    Err(Error::new(
                        ErrorKind::InvalidReference,
                        "domain/alias target must resolve to a one-dimensional set",
                    ))
                };
            }
            id = self.resolve_ref(&s.target)?;
        }
        Err(Error::new(ErrorKind::AliasCycle, "writer alias cycle"))
    }
    pub fn symbol(&mut self, spec: &WriteSymbol<'_>) -> Result<WriterSymbolId> {
        self.mutable()?;
        name(spec.name)?;
        text(spec.description)?;
        if spec.dimension > 20 {
            return Err(Error::new(
                ErrorKind::InvalidDimension,
                "writer dimension exceeds 20",
            ));
        }
        if self.find_string(2, spec.name, true)?.is_some()
            || self.find_string(5, spec.name, true)?.is_some()
        {
            return Err(invalid("duplicate ASCII-folded symbol name"));
        }
        if (spec.kind == SymbolType::Variable && !(0..=9).contains(&spec.user_information))
            || (spec.kind == SymbolType::Equation && !(53..=59).contains(&spec.user_information))
        {
            return Err(invalid("invalid variable/equation subtype"));
        }
        let target = if spec.kind == SymbolType::Alias {
            if spec.dimension != 1 {
                return Err(invalid("alias dimension must be one"));
            }
            self.reference(
                spec.alias_target
                    .ok_or_else(|| invalid("alias target required"))?,
            )?
        } else {
            if spec.alias_target.is_some() {
                return Err(invalid("non-alias cannot have alias target"));
            }
            Ref {
                id: 0,
                name: Bytes::default(),
            }
        };
        let id = next_id(self.stats.symbols)?;
        let mut p = Packet::new();
        p.u32(id)?;
        p.string(spec.name)?;
        p.string(spec.description)?;
        p.u8(spec.kind.code())?;
        p.u8(spec.dimension)?;
        p.u32(spec.user_information as u32)?;
        p.u32(target.id)?;
        p.string(target.name.as_slice())?;
        self.append(2, &p)?;
        self.stats.symbols = id;
        Ok(WriterSymbolId {
            owner: self.owner,
            id: SymbolId::new(id)?,
        })
    }
    pub fn comment(&mut self, s: WriterSymbolId, bytes: &[u8]) -> Result<()> {
        self.mutable()?;
        let id = self.check(s)?;
        text(bytes)?;
        let next = next_id(self.comments)?;
        let mut p = Packet::new();
        p.u32(id)?;
        p.string(bytes)?;
        self.append(3, &p)?;
        self.comments = next;
        Ok(())
    }
    pub fn set_text(&mut self, bytes: &[u8]) -> Result<u32> {
        self.mutable()?;
        text(bytes)?;
        if bytes.is_empty() {
            return Ok(0);
        }
        if let Some(id) = self.find_string(4, bytes, false)? {
            return Ok(id);
        }
        let id = self.stats.set_texts;
        let next = next_id(id)?;
        let mut p = Packet::new();
        p.u32(id)?;
        p.string(bytes)?;
        self.append(4, &p)?;
        self.stats.set_texts = next;
        Ok(id)
    }
    pub fn acronym(&mut self, id: AcronymId, n: &[u8], t: &[u8]) -> Result<()> {
        self.mutable()?;
        name(n)?;
        text(t)?;
        if self.find_string(2, n, true)?.is_some() {
            return Err(invalid("acronym collides with symbol name"));
        }
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 5 {
                let mut p = Parse::new(f.payload());
                let old = p.u32()?;
                let name = p.string()?;
                if old == id.get() || name.as_slice().eq_ignore_ascii_case(n) {
                    return Err(invalid("duplicate acronym numeric ID or name"));
                }
            }
        }
        let count = next_id(self.stats.acronyms)?;
        let mut p = Packet::new();
        p.u32(id.get())?;
        p.string(n)?;
        p.string(t)?;
        self.append(5, &p)?;
        self.stats.acronyms = count;
        Ok(())
    }
    pub fn domains(&mut self, id: WriterSymbolId, domains: &[WriteDomain<'_>]) -> Result<()> {
        self.mutable()?;
        let id = self.check(id)?;
        let s = self.lookup(id)?;
        if domains.is_empty() {
            return Ok(());
        }
        if s.kind == SymbolType::Alias || domains.len() != s.dim as usize {
            return Err(invalid("domain arity/kind mismatch"));
        }
        if self.mode(id)?.is_some() {
            return Err(invalid("domains already registered"));
        }
        let mode = match domains[0] {
            WriteDomain::Strict(_) => 1,
            WriteDomain::Relaxed(_) => 2,
        };
        for (axis, domain) in domains.iter().enumerate() {
            let r = match domain {
                WriteDomain::Strict(r) if mode == 1 => self.reference(*r)?,
                WriteDomain::Relaxed(n) if mode == 2 => {
                    if *n != b"*" {
                        name(n)?;
                    }
                    let mut found = None;
                    let mut at = 0;
                    while let Some(f) = self.j()?.next(&mut at)? {
                        if f.tag == 6 {
                            let mut p = Parse::new(f.payload());
                            p.u32()?;
                            p.u8()?;
                            let m = p.u8()?;
                            let old = p.u32()?;
                            let label = p.string()?;
                            if m == 2 && label.as_slice() == *n {
                                found = Some(old);
                                break;
                            }
                        }
                    }
                    let index = if *n == b"*" {
                        0
                    } else if let Some(old) = found {
                        old
                    } else {
                        self.domains = next_id(self.domains)?;
                        self.domains
                    };
                    Ref {
                        id: index,
                        name: Bytes::from_slice(n)?,
                    }
                }
                _ => return Err(invalid("strict and relaxed domains cannot mix")),
            };
            let mut p = Packet::new();
            p.u32(id)?;
            p.u8(axis as u8)?;
            p.u8(mode)?;
            p.u32(r.id)?;
            p.string(r.name.as_slice())?;
            self.append(6, &p)?;
        }
        let mut p = Packet::new();
        p.u32(id)?;
        p.u8(mode)?;
        self.append(7, &p)
    }
    fn mode(&mut self, id: u32) -> Result<Option<u8>> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 7 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                let mode = p.u8()?;
                p.done()?;
                if symbol == id {
                    return Ok(Some(mode));
                }
            }
        }
        Ok(None)
    }
    fn domain(&mut self, id: u32, axis: u8) -> Result<(u8, Ref)> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 6 {
                let mut p = Parse::new(f.payload());
                let symbol = p.u32()?;
                let d = p.u8()?;
                let mode = p.u8()?;
                let r = Ref {
                    id: p.u32()?,
                    name: p.string()?,
                };
                p.done()?;
                if symbol == id && d == axis {
                    return Ok((mode, r));
                }
            }
        }
        Err(corrupt())
    }
    pub fn freeze(&mut self) -> Result<()> {
        if self.stats.frozen {
            return Ok(());
        }
        self.j()?.sync()?;
        let mut counts = [0u32; 6];
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            let mut p = Parse::new(f.payload());
            let id = p.u32()?;
            match f.tag {
                1 | 4 => {
                    let b = p.string()?;
                    p.done()?;
                    if f.tag == 1 {
                        label(b.as_slice())?;
                        counts[1] = next_id(counts[1])?;
                        if id != counts[1] {
                            return Err(corrupt());
                        }
                    } else {
                        text(b.as_slice())?;
                        counts[4] = next_id(counts[4])?;
                        if id != counts[4] {
                            return Err(corrupt());
                        }
                    }
                }
                2 => {
                    let s = symbol(&f)?;
                    name(s.name.as_slice())?;
                    text(s.description.as_slice())?;
                    counts[2] = next_id(counts[2])?;
                    if s.id != counts[2] || s.dim > 20 {
                        return Err(corrupt());
                    }
                }
                3 => {
                    if id == 0 || id > self.stats.symbols {
                        return Err(corrupt());
                    }
                    text(p.string()?.as_slice())?;
                    p.done()?;
                    counts[3] = next_id(counts[3])?;
                }
                5 => {
                    AcronymId::new(id)?;
                    name(p.string()?.as_slice())?;
                    text(p.string()?.as_slice())?;
                    p.done()?;
                    counts[5] = next_id(counts[5])?;
                }
                6 => {
                    let s = self.lookup(id)?;
                    let d = p.u8()?;
                    let mode = p.u8()?;
                    let r = Ref {
                        id: p.u32()?,
                        name: p.string()?,
                    };
                    p.done()?;
                    if d >= s.dim || self.mode(id)? != Some(mode) {
                        return Err(corrupt());
                    }
                    if mode == 1 {
                        self.set_target(&r)?;
                    } else if mode != 2 || r.id > self.domains {
                        return Err(corrupt());
                    }
                }
                7 => {
                    let s = self.lookup(id)?;
                    let mode = p.u8()?;
                    p.done()?;
                    if mode != 1 && mode != 2 {
                        return Err(corrupt());
                    }
                    for axis in 0..s.dim {
                        if self.domain(id, axis)?.0 != mode {
                            return Err(corrupt());
                        }
                    }
                }
                _ => return Err(corrupt()),
            }
        }
        if (counts[1], counts[2], counts[3], counts[4] + 1, counts[5])
            != (
                self.stats.uels,
                self.stats.symbols,
                self.comments,
                self.stats.set_texts,
                self.stats.acronyms,
            )
        {
            return Err(corrupt());
        }
        for id in 1..=self.stats.symbols {
            let s = self.lookup(id)?;
            if s.kind == SymbolType::Alias {
                self.set_target(&s.target)?;
            }
        }
        self.stats.frozen = true;
        Ok(())
    }
    pub fn bulk<I, B, F>(&mut self, labels: I, mut mapping: F) -> Result<()>
    where
        I: IntoIterator<Item = Result<B>>,
        B: AsRef<[u8]>,
        F: FnMut(u64, WriterUelId) -> Result<()>,
    {
        self.mutable()?;
        if self.stats.uels != 0 {
            return Err(invalid("bulk UEL registration requires a fresh dictionary"));
        }
        let mut requests = Store::create(&self.path(1), &self.budget)?;
        self.owned[1] = true;
        self.stats.peak_open_files = self.stats.peak_open_files.max(2);
        let mut index = Store::create(&self.path(2), &self.budget)?;
        self.owned[2] = true;
        self.stats.peak_open_files = self.stats.peak_open_files.max(3);
        let mut sort = SpillSorter::with_budget(Arc::clone(&self.budget), None, &self.options)?;
        self.sort_directory = Some(sort.directory().to_path_buf());
        let observe = sort.observer();
        let result = (|| {
            let mut ordinal = 0u32;
            for bytes in labels {
                let bytes = bytes?;
                let bytes = bytes.as_ref();
                label(bytes)?;
                ordinal = next_id(ordinal)?;
                let mut p = Packet::new();
                p.u32(ordinal)?;
                p.string(bytes)?;
                requests.append(
                    1,
                    p.payload(),
                    &mut self.used,
                    &mut self.peak,
                    self.options.maximum_temporary_bytes,
                )?;
                index.append(
                    8,
                    &0i64.to_le_bytes(),
                    &mut self.used,
                    &mut self.peak,
                    self.options.maximum_temporary_bytes,
                )?;
                sort.push(bytes, u64::from(ordinal))?;
            }
            let mut sorted = sort.finish()?;
            let mut last = None::<Bytes>;
            let mut first = 0u32;
            while let Some(entry) = sorted.next()? {
                let n = u32::try_from(entry.id).map_err(|_| corrupt())?;
                if n == 0 || n > ordinal {
                    return Err(corrupt());
                }
                if last.as_ref().is_none_or(|v| v.as_slice() != entry.label) {
                    first = n;
                    last = Some(Bytes::from_slice(entry.label)?);
                }
                index.set_integer(n, i64::from(first))?;
            }
            let mut at = 0;
            let mut seen = 0u32;
            while let Some(f) = requests.next(&mut at)? {
                let mut p = Parse::new(f.payload());
                let n = p.u32()?;
                let bytes = p.string()?;
                p.done()?;
                seen = next_id(seen)?;
                if n != seen {
                    return Err(corrupt());
                }
                let canonical = index.integer(n)?;
                if canonical <= 0 || canonical > i64::from(n) {
                    return Err(corrupt());
                }
                let id = if canonical == i64::from(n) {
                    let id = next_id(self.stats.uels)?;
                    let mut p = Packet::new();
                    p.u32(id)?;
                    p.string(bytes.as_slice())?;
                    self.append(1, &p)?;
                    self.stats.uels = id;
                    id
                } else {
                    let resolved = index.integer(canonical as u32)?;
                    if resolved >= 0 || resolved < -i64::from(self.stats.uels) {
                        return Err(corrupt());
                    }
                    (-resolved) as u32
                };
                index.set_integer(n, -i64::from(id))?;
            }
            if seen != ordinal {
                return Err(corrupt());
            }
            index.sync()?;
            for n in 1..=ordinal {
                let id = index.integer(n)?;
                if id >= 0 || id < -i64::from(self.stats.uels) {
                    return Err(corrupt());
                }
                mapping(u64::from(n), self.id((-id) as u32)?)?;
            }
            self.stats.bulk_requests = u64::from(ordinal);
            Ok(())
        })();
        let statistics = observe();
        self.sort_used = statistics.temporary_bytes;
        self.sort_peak = self.sort_peak.max(statistics.peak_temporary_bytes);
        self.sort_handles = statistics.peak_open_files;
        self.stats.merge_passes = statistics.merge_passes;
        result?;
        let bytes = requests
            .length
            .checked_add(index.length)
            .ok_or_else(corrupt)?;
        drop(requests);
        drop(index);
        for n in 1..=2 {
            event(Event::MetadataRemove).map_err(Error::io)?;
            fs::remove_file(self.path(n)).map_err(Error::io)?;
            self.owned[n] = false;
        }
        self.used = self.used.checked_sub(bytes).ok_or_else(corrupt)?;
        self.sort_directory = None;
        Ok(())
    }
    pub fn cleanup(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        self.journal = None;
        let mut first = None;
        if let Some(path) = &self.sort_directory {
            if let Err(e) = cleanup_sort_directory(path) {
                first = Some(e);
            } else {
                self.sort_directory = None;
                self.sort_used = 0;
            }
        }
        for n in 0..3 {
            if self.owned[n] {
                match event(Event::MetadataRemove).and_then(|_| fs::remove_file(self.path(n))) {
                    Ok(()) => self.owned[n] = false,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => self.owned[n] = false,
                    Err(e) => {
                        if first.is_none() {
                            first = Some(e);
                        }
                    }
                }
            }
        }
        if let Err(e) = event(Event::MetadataRemove).and_then(|_| fs::remove_dir(&self.directory))
            && e.kind() != io::ErrorKind::NotFound
            && first.is_none()
        {
            first = Some(e);
        }
        if let Some(e) = first {
            return Err(e);
        }
        self.used = 0;
        self.cleaned = true;
        Ok(())
    }
}
impl Drop for Metadata {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
impl Metadata {
    fn emit(w: &mut Writer, b: &[u8]) -> Result<()> {
        w.encoded(|e, w| e.bytes(b, |b| w.bytes(b)))
    }
    fn record_info(&mut self, id: u32) -> Result<Option<(u64, u32, u64)>> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 8 {
                let mut p = Parse::new(f.payload());
                let n = p.u32()?;
                let pos = p.u64()?;
                let count = p.u32()?;
                let start = p.u64()?;
                p.done()?;
                if n == id {
                    return Ok(Some((pos, count, start)));
                }
            }
        }
        Ok(None)
    }
    pub(super) fn start_records(&mut self, id: WriterSymbolId) -> Result<RecordSetup> {
        self.freeze()?;
        let id = self.check(id)?;
        if self.record_info(id)?.is_some() {
            return Err(invalid("symbol already written"));
        }
        let s = self.lookup(id)?;
        if s.kind == SymbolType::Alias {
            return Err(invalid("aliases have no own records"));
        }
        let mut domains = [None; 20];
        if self.mode(id)? == Some(1) {
            for axis in 0..s.dim {
                let (_, r) = self.domain(id, axis)?;
                let target = self.set_target(&r)?;
                if target != 0 {
                    let (_, count, start) = self.record_info(target)?.ok_or_else(|| {
                        invalid("complete strict domain set before dependent symbol")
                    })?;
                    domains[usize::from(axis)] = Some(DomainMembership {
                        target,
                        count,
                        start,
                    });
                }
            }
        }
        Ok(RecordSetup {
            kind: s.kind,
            dimension: s.dim,
            domains,
            members: self.j()?.length,
        })
    }
    pub(super) fn validate_record(
        &mut self,
        domains: &Domains,
        keys: &[WriterUelId],
        values: &[Value],
        kind: SymbolType,
    ) -> Result<()> {
        for (axis, key) in keys.iter().enumerate() {
            if key.owner != self.owner || key.get() > self.stats.uels {
                return Err(Error::new(
                    ErrorKind::SourceMismatch,
                    "UEL belongs to another writer",
                ));
            }
            if let Some(DomainMembership {
                target,
                count,
                start,
            }) = domains[axis]
            {
                let (mut low, mut high) = (0u32, count);
                let mut found = false;
                while low < high {
                    let mid = low + (high - low) / 2;
                    let mut at = start.checked_add(u64::from(mid) * 19).ok_or_else(corrupt)?;
                    let f = self.j()?.next(&mut at)?.ok_or_else(corrupt)?;
                    if f.tag != 9 {
                        return Err(corrupt());
                    }
                    let mut p = Parse::new(f.payload());
                    if p.u32()? != target {
                        return Err(corrupt());
                    }
                    let member = p.u32()?;
                    p.done()?;
                    match member.cmp(&key.get()) {
                        std::cmp::Ordering::Less => low = mid + 1,
                        std::cmp::Ordering::Greater => high = mid,
                        std::cmp::Ordering::Equal => {
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    return Err(invalid("key is outside strict domain"));
                }
            }
        }
        for value in values {
            match *value {
                Value::NumberBits(bits) => {
                    if Value::ordinary(bits)? != *value {
                        return Err(invalid("use typed acronym value"));
                    }
                }
                Value::Acronym { id, encoded_bits } => {
                    if Value::ordinary(encoded_bits)? != *value {
                        return Err(invalid("acronym bits disagree with ID"));
                    }
                    let mut at = 0;
                    let mut found = false;
                    while let Some(f) = self.j()?.next(&mut at)? {
                        if f.tag == 5 && Parse::new(f.payload()).u32()? == id.get() {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(invalid("unregistered acronym"));
                    }
                }
                Value::Special(_) => {}
            }
            if kind == SymbolType::Set {
                let Value::NumberBits(bits) = *value else {
                    return Err(invalid("set value must be a set-text ID"));
                };
                let n = f64::from_bits(bits);
                if !n.is_finite()
                    || n < 0.0
                    || n.fract() != 0.0
                    || n >= f64::from(self.stats.set_texts)
                {
                    return Err(invalid("invalid set-text ID"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn member(&mut self, id: WriterSymbolId, key: WriterUelId) -> Result<()> {
        let mut p = Packet::new();
        p.u32(id.get())?;
        p.u32(key.get())?;
        self.append(9, &p)
    }
    pub(super) fn complete_records(
        &mut self,
        id: WriterSymbolId,
        position: u64,
        count: u32,
        start: u64,
    ) -> Result<()> {
        let mut p = Packet::new();
        p.u32(id.get())?;
        p.u64(position)?;
        p.u32(count)?;
        p.u64(start)?;
        self.append(8, &p)
    }
    pub fn sections(&mut self, w: &mut Writer) -> Result<[u64; 6]> {
        self.freeze()?;
        for id in 1..=self.stats.symbols {
            if self.record_info(id)?.is_some() {
                continue;
            }
            let s = self.lookup(id)?;
            let position = if s.kind == SymbolType::Alias {
                0
            } else {
                let position = w.position;
                w.section_string(b"_DATA_")?;
                Self::emit(w, &[s.dim])?;
                w.section_i32(0)?;
                for _ in 0..s.dim {
                    w.section_i32(0)?;
                    w.section_i32(0)?;
                }
                Self::emit(w, &[255])?;
                w.section_end()?;
                position
            };
            let token = WriterSymbolId {
                owner: self.owner,
                id: SymbolId::new(id)?,
            };
            self.complete_records(token, position, 0, 0)?;
        }
        let symbols = w.position;
        w.section_string(b"_SYMB_")?;
        w.section_i32(self.stats.symbols as i32)?;
        for id in 1..=self.stats.symbols {
            let s = self.lookup(id)?;
            let (position, record_count, _) = self.record_info(id)?.ok_or_else(corrupt)?;
            let mode = self.mode(id)?;
            w.section_string(s.name.as_slice())?;
            Self::emit(w, &position.to_le_bytes())?;
            w.section_i32(i32::from(s.dim))?;
            Self::emit(w, &[s.kind.code()])?;
            let user = if s.kind == SymbolType::Alias {
                self.set_target(&s.target)? as i32
            } else {
                s.user
            };
            w.section_i32(user)?;
            w.section_i32(record_count as i32)?;
            w.section_i32(0)?;
            Self::emit(w, &[0])?;
            w.section_string(s.description.as_slice())?;
            Self::emit(
                w,
                &[
                    u8::from(
                        w.compression == WriteCompression::Zlib && s.kind != SymbolType::Alias,
                    ),
                    u8::from(mode == Some(1)),
                ],
            )?;
            if mode == Some(1) {
                for axis in 0..s.dim {
                    let (_, r) = self.domain(id, axis)?;
                    // Validate the chain without replacing the referenced alias ID.
                    let referenced = self.resolve_ref(&r)?;
                    self.set_target(&Ref {
                        id: referenced,
                        name: Bytes::default(),
                    })?;
                    w.section_i32(referenced as i32)?;
                }
            }
            let mut count = 0u32;
            let mut at = 0;
            while let Some(f) = self.j()?.next(&mut at)? {
                if f.tag == 3 {
                    let mut p = Parse::new(f.payload());
                    if p.u32()? == id {
                        count = next_id(count)?;
                    }
                }
            }
            w.section_i32(count as i32)?;
            at = 0;
            while let Some(f) = self.j()?.next(&mut at)? {
                if f.tag == 3 {
                    let mut p = Parse::new(f.payload());
                    if p.u32()? == id {
                        w.section_string(p.string()?.as_slice())?;
                    }
                }
            }
        }
        w.section_string(b"_SYMB_")?;
        w.section_end()?;
        let texts = w.position;
        w.section_string(b"_SETT_")?;
        w.section_i32(self.stats.set_texts as i32)?;
        w.section_string(b"")?;
        self.strings(w, 4)?;
        w.section_string(b"_SETT_")?;
        w.section_end()?;
        let uels = w.position;
        w.section_string(b"_UEL_")?;
        w.section_i32(self.stats.uels as i32)?;
        self.strings(w, 1)?;
        w.section_string(b"_UEL_")?;
        w.section_end()?;
        let acronyms = w.position;
        w.section_string(b"_ACRO_")?;
        w.section_i32(self.stats.acronyms as i32)?;
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == 5 {
                let mut p = Parse::new(f.payload());
                let id = p.u32()?;
                w.section_string(p.string()?.as_slice())?;
                w.section_string(p.string()?.as_slice())?;
                w.section_i32(id as i32)?;
            }
        }
        w.section_string(b"_ACRO_")?;
        w.section_end()?;
        let mut has_relaxed = false;
        for id in 1..=self.stats.symbols {
            if self.mode(id)? == Some(2) {
                has_relaxed = true;
                break;
            }
        }
        let domains = if has_relaxed {
            let start = w.position;
            w.section_string(b"_DOMS_")?;
            w.section_i32(self.domains as i32)?;
            let mut emitted = 0;
            let mut at = 0;
            while let Some(f) = self.j()?.next(&mut at)? {
                if f.tag == 6 {
                    let mut p = Parse::new(f.payload());
                    p.u32()?;
                    p.u8()?;
                    let mode = p.u8()?;
                    let id = p.u32()?;
                    let label = p.string()?;
                    if mode == 2 && id == emitted + 1 {
                        w.section_string(label.as_slice())?;
                        emitted = id;
                    }
                }
            }
            if emitted != self.domains {
                return Err(corrupt());
            }
            w.section_string(b"_DOMS_")?;
            for id in 1..=self.stats.symbols {
                if self.mode(id)? == Some(2) {
                    let s = self.lookup(id)?;
                    w.section_i32(id as i32)?;
                    for axis in 0..s.dim {
                        w.section_i32(self.domain(id, axis)?.1.id as i32)?;
                    }
                }
            }
            w.section_i32(-1)?;
            w.section_string(b"_DOMS_")?;
            w.section_end()?;
            start
        } else {
            0
        };
        Ok([symbols, uels, texts, acronyms, symbols, domains])
    }
    fn strings(&mut self, w: &mut Writer, tag: u8) -> Result<()> {
        let mut at = 0;
        while let Some(f) = self.j()?.next(&mut at)? {
            if f.tag == tag {
                let mut p = Parse::new(f.payload());
                p.u32()?;
                w.section_string(p.string()?.as_slice())?;
            }
        }
        Ok(())
    }
}
impl Writer {
    pub(super) fn open(&self) -> Result<()> {
        if self.statistics.status != WriteStatus::Open {
            return Err(self.error.clone().unwrap_or_else(|| {
                Error::new(ErrorKind::InvalidWriteState, "writer is not open")
            }));
        }
        Ok(())
    }
    pub fn begin_metadata(&mut self, options: &MetadataOptions) -> Result<()> {
        self.open()?;
        if self.metadata.is_some() {
            return Err(self.fail(invalid("metadata registration already started")));
        }
        match Metadata::new(Arc::clone(&self.budget), self.owner, options) {
            Ok(m) => {
                self.metadata_statistics = m.statistics();
                self.metadata = Some(m);
                self.metadata_call(Metadata::initialize_journal)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub(super) fn metadata_call<T>(
        &mut self,
        f: impl FnOnce(&mut Metadata) -> Result<T>,
    ) -> Result<T> {
        self.open()?;
        let result = self
            .metadata
            .as_mut()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidWriteState,
                    "begin metadata registration first",
                )
            })
            .and_then(f);
        if let Some(m) = &self.metadata {
            self.metadata_statistics = m.statistics();
        }
        result.map_err(|e| self.fail(e))
    }
    pub fn metadata_statistics(&self) -> MetadataStatistics {
        self.metadata
            .as_ref()
            .map_or(self.metadata_statistics, Metadata::statistics)
    }
    pub fn register_uel(&mut self, label: &[u8]) -> Result<WriterUelId> {
        self.metadata_call(|m| m.uel(label))
    }
    pub fn register_uels_fresh<I, B, F>(&mut self, labels: I, mapping: F) -> Result<()>
    where
        I: IntoIterator<Item = Result<B>>,
        B: AsRef<[u8]>,
        F: FnMut(u64, WriterUelId) -> Result<()>,
    {
        self.metadata_call(|m| m.bulk(labels, mapping))
    }
    pub fn register_symbol(&mut self, symbol: &WriteSymbol<'_>) -> Result<WriterSymbolId> {
        self.metadata_call(|m| m.symbol(symbol))
    }
    pub fn add_comment(&mut self, symbol: WriterSymbolId, text: &[u8]) -> Result<()> {
        self.metadata_call(|m| m.comment(symbol, text))
    }
    pub fn add_set_text(&mut self, text: &[u8]) -> Result<u32> {
        self.metadata_call(|m| m.set_text(text))
    }
    pub fn add_acronym(&mut self, id: AcronymId, name: &[u8], text: &[u8]) -> Result<()> {
        self.metadata_call(|m| m.acronym(id, name, text))
    }
    pub fn set_domains(
        &mut self,
        symbol: WriterSymbolId,
        domains: &[WriteDomain<'_>],
    ) -> Result<()> {
        self.metadata_call(|m| m.domains(symbol, domains))
    }
    pub fn freeze_metadata(&mut self) -> Result<()> {
        self.metadata_call(Metadata::freeze)
    }
}
#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;

// Only a directory obtained from this writer's successfully created sorter is
// accepted. Stream numeric run names; preserve unexpected entries/symlinks.
fn cleanup_sort_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(m) if !m.is_dir() => return Err(io::Error::other("owned sort path changed type")),
        Ok(_) => {}
    }
    let mut first = None;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file = entry.file_name();
        let Some(name) = file.to_str() else {
            return Err(io::Error::other("unexpected owned-sort entry"));
        };
        let Some((id, suffix)) = name.strip_prefix("r-").and_then(|s| s.split_once('.')) else {
            return Err(io::Error::other("unexpected owned-sort entry"));
        };
        if id.len() != 16
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(suffix, "run" | "part")
            || !entry.file_type()?.is_file()
        {
            return Err(io::Error::other("unexpected owned-sort entry type"));
        }
        if let Err(e) = event(Event::MetadataRemove).and_then(|_| fs::remove_file(entry.path()))
            && first.is_none()
        {
            first = Some(e);
        }
    }
    if let Some(e) = first {
        return Err(e);
    }
    event(Event::MetadataRemove)?;
    fs::remove_dir(path)
}
