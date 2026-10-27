//! Explicit ID-preserving metadata views. Session edits never mutate the source.
use crate::*;
use crate::{budget::Reservation, model::Bytes, write::Metadata};
use std::sync::Arc;
#[derive(Clone)]
pub struct UelInformation {
    pub id: UelId,
    pub mapped: Option<MappedUelId>,
    pub(crate) label: Bytes,
}
impl UelInformation {
    pub fn label(&self) -> &[u8] {
        self.label.as_slice()
    }
}
#[derive(Clone)]
pub struct SetTextInformation {
    pub id: u32,
    pub node: i32,
    pub(crate) text: Bytes,
}
impl SetTextInformation {
    pub fn text(&self) -> &[u8] {
        self.text.as_slice()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UelMappingInformation {
    pub uel_count: u32,
    pub mapped_count: u32,
    pub highest_mapped: Option<MappedUelId>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetadataVisitStatistics {
    pub visited: u64,
    pub complete: bool,
}
/// Explicit metadata view. Source IDs and bytes stay unchanged.
///
/// ```no_run
/// use gdxrs::*;
/// # fn example() -> Result<()> {
/// let reader = Reader::open("input.gdx", OpenOptions::default())?;
/// let mut view = reader.metadata_session(&MetadataOptions::new("scratch"))?;
/// view.rename_uel(UelId::new(1)?, b"display-label")?;
/// view.set_text_node(0, i32::MAX)?; // Session-local, never serialized.
/// view.visit_uels(|uel| { println!("{} {:?}", uel.id.get(), uel.label()); Ok(()) })?;
/// view.close()?;
/// # Ok(()) }
/// ```
pub struct MetadataSession {
    reader: Reader,
    metadata: Metadata,
    initialized: bool,
    closed: bool,
    error: Option<Error>,
    visit: MetadataVisitStatistics,
    _state: Reservation,
}
fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidReference, message)
}
impl Reader {
    pub fn uel_information(&self, id: UelId) -> Result<UelInformation> {
        self.source.check_unchanged()?;
        if id.get() > self.source.counts()?.uels {
            return Err(invalid("UEL ID outside source table"));
        }
        let mut cursor = self.uels()?;
        while let Some(v) = cursor.next()? {
            if v.id == id {
                return Ok(UelInformation {
                    id,
                    mapped: None,
                    label: Bytes::from_slice(v.label)?,
                });
            }
        }
        Err(invalid("missing source UEL"))
    }
    pub fn find_uel(&self, label: &[u8]) -> Result<Option<UelId>> {
        let mut cursor = self.uels()?;
        while let Some(v) = cursor.next()? {
            if v.label == label {
                return Ok(Some(v.id));
            }
        }
        Ok(None)
    }
    pub fn set_text_information(&self, id: u32) -> Result<SetTextInformation> {
        let mut cursor = self.set_texts()?;
        while let Some(v) = cursor.next()? {
            if v.id == id {
                return Ok(SetTextInformation {
                    id,
                    node: 0,
                    text: Bytes::from_slice(v.text)?,
                });
            }
        }
        Err(invalid("set-text ID outside source table"))
    }
    pub fn uel_max_length(&self) -> Result<u8> {
        let mut cursor = self.uels()?;
        let mut maximum = 0;
        while let Some(v) = cursor.next()? {
            maximum = maximum.max(
                u8::try_from(v.label.len())
                    .map_err(|_| invalid("UEL length exceeds short string"))?,
            );
        }
        Ok(maximum)
    }
    pub fn symbol_max_length(&self) -> Result<u8> {
        let mut cursor = self.symbols()?;
        let mut maximum = 0;
        while let Some(v) = cursor.next()? {
            maximum = maximum.max(
                u8::try_from(v.name.len())
                    .map_err(|_| invalid("symbol name exceeds short string"))?,
            );
        }
        Ok(maximum)
    }
    /// Explicit full scan; no ordinary cursor or short-read request invokes it.
    pub fn symbol_index_max_lengths(&self, symbol: &SymbolHandle) -> Result<[u8; 20]> {
        self.check_handle(symbol)?;
        let dimension = symbol.metadata()?.dimension;
        let mut options = self.scan_options();
        options.value_projection = ValueProjection::Mask(0);
        let mut cursor = self.scan(symbol, options)?;
        let mut labels = self.uel_labels()?;
        let mut maximum = [0; 20];
        while let Some(batch) = cursor.next_batch()? {
            for axis in 0..dimension {
                for key in batch
                    .key_column(axis)
                    .ok_or_else(|| invalid("missing key column"))?
                {
                    let length = u8::try_from(labels.get(*key)?.len())
                        .map_err(|_| invalid("UEL exceeds short string"))?;
                    maximum[usize::from(axis)] = maximum[usize::from(axis)].max(length);
                }
            }
        }
        Ok(maximum)
    }
    /// Return the cleanup owner before lazy allocation. Options bound this
    /// explicit scratch job; memory competes with all cursors under the root.
    pub fn metadata_session(&self, options: &MetadataOptions) -> Result<MetadataSession> {
        if std::mem::size_of::<MetadataSession>()
            + std::mem::size_of::<UelInformation>()
            + std::mem::size_of::<SetTextInformation>()
            > 16384
        {
            return Err(Error::new(
                ErrorKind::BudgetTooSmall,
                "metadata session controls exceed allowance",
            ));
        }
        let state = self.source.budget.reserve(16384)?;
        let metadata = Metadata::new(Arc::clone(&self.source.budget), self.source.id, options)?;
        Ok(MetadataSession {
            reader: self.clone(),
            metadata,
            initialized: false,
            closed: false,
            error: None,
            visit: MetadataVisitStatistics::default(),
            _state: state,
        })
    }
}
impl MetadataSession {
    pub(crate) fn check_reader(&mut self, reader: &Reader) -> Result<()> {
        self.ensure()?;
        if !Arc::ptr_eq(&self.reader.source, &reader.source) {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "metadata session belongs to another reader",
            ));
        }
        Ok(())
    }
    fn ensure(&mut self) -> Result<()> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if self.closed {
            return Err(invalid("metadata session is closed"));
        }
        self.reader
            .source
            .check_unchanged()
            .map_err(|e| self.fail(e))
    }
    fn journal(&mut self) -> Result<()> {
        if !self.initialized {
            self.metadata.initialize_journal()?;
            self.initialized = true;
        }
        Ok(())
    }
    fn fail(&mut self, mut error: Error) -> Error {
        self.closed = true;
        if let Err(cleanup) = self.metadata.cleanup()
            && error.cleanup_error().is_none()
        {
            error = error.with_cleanup(cleanup);
        }
        self.error = Some(error.clone());
        error
    }
    fn information(&mut self, id: UelId) -> Result<UelInformation> {
        let mut info = self.reader.uel_information(id)?;
        if self.initialized
            && let Some(label) = self.metadata.overlay_label(id.get())?
        {
            info.label = label;
        }
        Ok(info)
    }
    pub fn uel_information(&mut self, id: UelId) -> Result<UelInformation> {
        self.ensure()?;
        self.information(id).map_err(|e| self.fail(e))
    }
    fn find(&mut self, label: &[u8]) -> Result<Option<UelId>> {
        let mut cursor = self.reader.uels()?;
        while let Some(v) = cursor.next()? {
            let current = if self.initialized {
                self.metadata.overlay_label(v.id.get())?
            } else {
                None
            };
            if current.as_ref().map_or(v.label, Bytes::as_slice) == label {
                return Ok(Some(v.id));
            }
        }
        Ok(None)
    }
    pub fn find_uel(&mut self, label: &[u8]) -> Result<Option<UelId>> {
        self.ensure()?;
        self.find(label).map_err(|e| self.fail(e))
    }
    pub fn rename_uel(&mut self, id: UelId, new_label: &[u8]) -> Result<()> {
        self.ensure()?;
        let result = (|| {
            self.information(id)?;
            crate::write::validate_uel_label(new_label)?;
            if self.find(new_label)?.is_some() {
                return Err(invalid("UEL rename target already exists"));
            }
            self.journal()?;
            self.metadata.assign_label(id.get(), new_label)
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn set_text_information(&mut self, id: u32) -> Result<SetTextInformation> {
        self.ensure()?;
        let result = (|| {
            let mut info = self.reader.set_text_information(id)?;
            if self.initialized {
                info.node = self.metadata.node(id)?;
            }
            Ok(info)
        })();
        result.map_err(|e| self.fail(e))
    }
    /// Assign any signed i32 once while the current node is zero. Not persisted.
    pub fn set_text_node(&mut self, id: u32, node: i32) -> Result<bool> {
        self.ensure()?;
        let result = (|| {
            self.reader.set_text_information(id)?;
            if !self.initialized && node == 0 {
                return Ok(true);
            }
            self.journal()?;
            self.metadata.assign_node(id, node)
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn uel_max_length(&mut self) -> Result<u8> {
        self.ensure()?;
        let result = (|| {
            let mut maximum = 0;
            let mut cursor = self.reader.uels()?;
            while let Some(v) = cursor.next()? {
                let changed = if self.initialized {
                    self.metadata.overlay_label(v.id.get())?
                } else {
                    None
                };
                let len = changed
                    .as_ref()
                    .map_or(v.label.len(), |b| b.as_slice().len());
                maximum = maximum
                    .max(u8::try_from(len).map_err(|_| invalid("UEL exceeds short string"))?);
            }
            Ok(maximum)
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn symbol_index_max_lengths(&mut self, symbol: &SymbolHandle) -> Result<[u8; 20]> {
        self.ensure()?;
        let result = (|| {
            self.reader.check_handle(symbol)?;
            let dimension = symbol.metadata()?.dimension;
            let mut options = self.reader.scan_options();
            options.value_projection = ValueProjection::Mask(0);
            let mut cursor = self.reader.scan(symbol, options)?;
            let mut maximum = [0; 20];
            while let Some(batch) = cursor.next_batch()? {
                for axis in 0..dimension {
                    for key in batch
                        .key_column(axis)
                        .ok_or_else(|| invalid("missing key column"))?
                    {
                        let length = u8::try_from(self.information(*key)?.label().len())
                            .map_err(|_| invalid("UEL exceeds short string"))?;
                        maximum[usize::from(axis)] = maximum[usize::from(axis)].max(length);
                    }
                }
            }
            Ok(maximum)
        })();
        result.map_err(|e| self.fail(e))
    }
    /// Fallible, bounded callback. Statistics acknowledge successful callbacks;
    /// failures are terminal and never automatically replay earlier output.
    pub fn visit_uels<F>(&mut self, mut callback: F) -> Result<MetadataVisitStatistics>
    where
        F: FnMut(UelInformation) -> Result<()>,
    {
        self.ensure()?;
        self.visit = MetadataVisitStatistics::default();
        let result = (|| {
            let mut cursor = self.reader.uels()?;
            while let Some(v) = cursor.next()? {
                self.reader.source.check_unchanged()?;
                let label = if self.initialized {
                    self.metadata.overlay_label(v.id.get())?
                } else {
                    None
                };
                let info = UelInformation {
                    id: v.id,
                    mapped: None,
                    label: match label {
                        Some(label) => label,
                        None => Bytes::from_slice(v.label)?,
                    },
                };
                callback(info)?;
                self.visit.visited = self
                    .visit
                    .visited
                    .checked_add(1)
                    .ok_or_else(|| invalid("UEL visit count overflow"))?;
            }
            self.visit.complete = true;
            Ok(self.visit)
        })();
        result.map_err(|e| self.fail(e))
    }
    pub fn visit_statistics(&self) -> MetadataVisitStatistics {
        self.visit
    }
    pub fn statistics(&self) -> MetadataStatistics {
        self.metadata.statistics()
    }
    pub fn close(&mut self) -> Result<()> {
        self.closed = true;
        self.metadata.cleanup().map_err(Error::from)
    }
    pub fn abort(&mut self) -> Result<()> {
        self.close()
    }
}
