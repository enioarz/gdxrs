use crate::{
    format::Header,
    metadata::{self, Catalog, HandleState, SymbolRecord},
    source::{Counts, Source},
    *,
};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct Reader {
    pub(crate) source: Arc<Source>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, sync::Mutex};
    struct Spy {
        data: Vec<u8>,
        ranges: Arc<Mutex<Vec<(u64, usize)>>>,
    }
    impl crate::source::ReadAt for Spy {
        fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<usize> {
            let at_usize = usize::try_from(at).map_err(io::Error::other)?;
            let rest = self.data.get(at_usize..).unwrap_or(&[]);
            let n = rest.len().min(out.len()).min(3);
            out[..n].copy_from_slice(&rest[..n]);
            self.ranges.lock().unwrap().push((at, n));
            Ok(n)
        }
        fn unchanged(&self) -> io::Result<bool> {
            Ok(true)
        }
    }
    #[test]
    fn opening_and_catalogs_never_read_data_sections() {
        let data = std::fs::read("tests/fixtures/v7/semantics-0.gdx").unwrap();
        let len = data.len() as u64;
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let source = Source::testing(
            Box::new(Spy {
                data,
                ranges: Arc::clone(&ranges),
            }),
            len,
        );
        let reader = Reader::from_source(source).unwrap();
        let mut symbols = reader.symbols().unwrap();
        while symbols.next().unwrap().is_some() {}
        let header = reader.source.header().unwrap();
        assert!(
            ranges
                .lock()
                .unwrap()
                .iter()
                .all(|(start, n)| *start + *n as u64 <= header.data_start
                    || *start >= header.offsets[0])
        );
    }
}
pub struct FileInformation<'a> {
    pub version: u8,
    pub compression_level: u8,
    pub producer: &'a [u8],
    pub audit: &'a [u8],
    pub symbol_count: u32,
    pub uel_count: u32,
}
impl Reader {
    pub fn open(path: impl AsRef<Path>, options: OpenOptions) -> Result<Self> {
        let source = Source::open(path.as_ref(), options)?;
        Self::from_source(source)
    }
    pub(crate) fn from_source(source: Arc<Source>) -> Result<Self> {
        let h = Header::read(&source)?;
        source
            .header
            .set(h)
            .map_err(|_| Error::new(ErrorKind::InvalidHeader, "duplicate header initialization"))?;
        let mut symbols = metadata::section(Arc::clone(&source), 0, "symbols")?;
        symbols.marker(b"_SYMB_")?;
        let symbol_count = symbols.count()?;
        drop(symbols);
        let mut uels = metadata::section(Arc::clone(&source), 1, "UELs")?;
        uels.marker(b"_UEL_")?;
        let uel_count = uels.count()?;
        drop(uels);
        source
            .counts
            .set(Counts {
                symbols: symbol_count,
                uels: uel_count,
            })
            .map_err(|_| Error::new(ErrorKind::InvalidHeader, "duplicate counts initialization"))?;
        Ok(Self { source })
    }
    pub fn information(&self) -> Result<FileInformation<'_>> {
        let h = self.source.header()?;
        let c = self.source.counts()?;
        Ok(FileInformation {
            version: 7,
            compression_level: u8::from(h.compressed),
            producer: h.producer.as_slice(),
            audit: h.audit.as_slice(),
            symbol_count: c.symbols,
            uel_count: c.uels,
        })
    }
    pub fn budget_statistics(&self) -> BudgetStatistics {
        self.source.budget.statistics()
    }
    pub fn io_statistics(&self) -> IoStatistics {
        self.source.statistics()
    }
    pub fn symbols(&self) -> Result<SymbolCursor> {
        SymbolCursor::new(self.clone())
    }
    pub(crate) fn lookup_record(&self, id: SymbolId) -> Result<SymbolRecord> {
        if id == SymbolId::UNIVERSE {
            return Ok(SymbolRecord::universe());
        }
        if id.get() > self.source.counts()?.symbols {
            return Err(
                Error::new(ErrorKind::InvalidReference, "symbol ID outside catalog")
                    .symbol(id.get()),
            );
        }
        let mut cursor = Catalog::new(Arc::clone(&self.source))?;
        while let Some(record) = cursor.next_record()? {
            if record.id == id.get() {
                return Ok(record);
            }
        }
        Err(Error::new(ErrorKind::InvalidReference, "symbol not found").symbol(id.get()))
    }
    pub(crate) fn resolve(&self, record: &SymbolRecord) -> Result<SymbolRecord> {
        let mut resolved = record.clone();
        for _ in 0..=self.source.counts()?.symbols {
            if resolved.kind != SymbolType::Alias {
                if record.kind == SymbolType::Alias
                    && (resolved.kind != SymbolType::Set || resolved.dimension != record.dimension)
                {
                    return Err(Error::new(
                        ErrorKind::InvalidReference,
                        "alias target is not a compatible set",
                    )
                    .symbol(record.id));
                }
                return Ok(resolved);
            }
            let id = u32::try_from(resolved.user)
                .map_err(|_| Error::new(ErrorKind::InvalidReference, "negative alias target"))?;
            resolved = self.lookup_record(SymbolId::new(id)?)?;
        }
        Err(Error::new(ErrorKind::AliasCycle, "alias chain exceeds catalog size").symbol(record.id))
    }
    pub(crate) fn raw_count(&self, record: &SymbolRecord) -> Result<u64> {
        let resolved = self.resolve(record)?;
        Ok(if resolved.id == 0 {
            u64::from(self.source.counts()?.uels)
        } else if resolved.dimension == 0 {
            1
        } else {
            u64::from(resolved.count)
        })
    }
    fn handle(&self, record: SymbolRecord) -> Result<SymbolHandle> {
        let reservation = self.source.budget.reserve(16 * 1024)?;
        Ok(SymbolHandle {
            inner: Arc::new(HandleState {
                id: SymbolId::new(record.id)?,
                source: Arc::clone(&self.source),
                record,
                _reservation: reservation,
            }),
        })
    }
    pub fn symbol(&self, id: SymbolId) -> Result<SymbolHandle> {
        self.handle(self.lookup_record(id)?)
    }
    pub fn find_symbol(&self, name: &[u8]) -> Result<Option<SymbolHandle>> {
        if name == b"*" {
            return self.symbol(SymbolId::UNIVERSE).map(Some);
        }
        let mut cursor = Catalog::new(Arc::clone(&self.source))?;
        while let Some(record) = cursor.next_record()? {
            if record.name.as_slice().eq_ignore_ascii_case(name) {
                return self.handle(record).map(Some);
            }
        }
        Ok(None)
    }
    pub fn uels(&self) -> Result<UelCursor> {
        UelCursor::new(Arc::clone(&self.source))
    }
    pub fn set_texts(&self) -> Result<SetTextCursor> {
        SetTextCursor::new(Arc::clone(&self.source))
    }
    pub fn acronyms(&self) -> Result<AcronymCursor> {
        AcronymCursor::new(Arc::clone(&self.source))
    }
    pub(crate) fn check_handle(&self, handle: &SymbolHandle) -> Result<()> {
        if !Arc::ptr_eq(&self.source, &handle.inner.source) {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "symbol belongs to another reader",
            )
            .symbol(handle.id().get()));
        }
        Ok(())
    }
    pub fn comments(&self, symbol: &SymbolHandle) -> Result<CommentCursor> {
        self.check_handle(symbol)?;
        CommentCursor::new(Arc::clone(&self.source), &symbol.inner.record)
    }
    pub fn domains(&self, symbol: &SymbolHandle) -> Result<DomainCursor> {
        self.check_handle(symbol)?;
        DomainCursor::new(self.clone(), symbol.inner.record.clone())
    }
}
