use crate::{
    budget::{Buffer, Reservation},
    records::Record,
    source::Source,
    *,
};
use std::{mem::size_of, sync::Arc};

/// An inclusive range in raw UEL registration order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UelRange {
    first: UelId,
    last: UelId,
}
impl UelRange {
    pub fn new(first: UelId, last: UelId) -> Result<Self> {
        if first.get() > last.get() {
            return Err(Error::new(
                ErrorKind::InvalidSelection,
                "reversed UEL range",
            ));
        }
        Ok(Self { first, last })
    }
    pub fn first(self) -> UelId {
        self.first
    }
    pub fn last(self) -> UelId {
        self.last
    }
}

/// Union within one key dimension. Borrowed requests are copied into the root quota.
#[derive(Clone, Copy, Debug)]
pub enum KeySelection<'a> {
    All,
    One(UelId),
    AnyOf(&'a [UelId]),
    Range(UelRange),
    Ranges(&'a [UelRange]),
}
/// One zero-based dimension clause. Distinct dimension clauses are conjoined.
#[derive(Clone, Copy, Debug)]
pub struct DimensionSelection<'a> {
    pub dimension: u8,
    pub keys: KeySelection<'a>,
}

enum Constraint {
    All,
    Empty,
    One(UelId),
    Range(UelRange),
    Set(Buffer<UelId>),
    Ranges(Buffer<UelRange>),
}
impl Constraint {
    fn matches(&self, key: UelId) -> bool {
        let value = key.get();
        match self {
            Self::All => true,
            Self::Empty => false,
            Self::One(id) => *id == key,
            Self::Range(range) => range.first.get() <= value && value <= range.last.get(),
            Self::Set(ids) => ids.data.binary_search_by_key(&value, |id| id.get()).is_ok(),
            Self::Ranges(ranges) => {
                let at = ranges.data.partition_point(|r| r.first.get() <= value);
                at.checked_sub(1)
                    .and_then(|i| ranges.data.get(i))
                    .is_some_and(|r| value <= r.last.get())
            }
        }
    }
}
struct State {
    source: Arc<Source>,
    symbol: SymbolId,
    dimension: u8,
    constraints: [Constraint; 20],
    empty: bool,
    _state: Reservation,
}
/// Immutable source/symbol-bound selection; clones share one accounted allocation.
#[derive(Clone)]
pub struct Selection {
    disk: Option<(u8, Arc<crate::spill::DiskIds>)>,
    closed: bool,
    inner: Arc<State>,
}
impl Selection {
    pub fn is_empty(&self) -> bool {
        self.closed || self.inner.empty || self.disk.as_ref().is_some_and(|(_, ids)| ids.is_empty())
    }
    pub(crate) fn validate(&self, reader: &Reader, symbol: &SymbolHandle) -> Result<()> {
        reader.check_handle(symbol)?;
        if !Arc::ptr_eq(&reader.source, &self.inner.source) {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "selection belongs to another reader",
            )
            .symbol(symbol.id().get()));
        }
        if self.inner.symbol != symbol.id() {
            return Err(Error::new(
                ErrorKind::InvalidSelection,
                "selection belongs to another symbol",
            )
            .symbol(symbol.id().get()));
        }
        Ok(())
    }
    pub(crate) fn matches(&self, row: &Record) -> Result<bool> {
        if !self
            .inner
            .constraints
            .iter()
            .take(usize::from(self.inner.dimension))
            .enumerate()
            .all(|(d, c)| row.key(d).is_some_and(|key| c.matches(key)))
        {
            return Ok(false);
        }
        if let Some((dimension, ids)) = &self.disk {
            let key = row.key(usize::from(*dimension)).ok_or_else(|| {
                Error::new(ErrorKind::InvalidSelection, "missing membership dimension")
            })?;
            return ids.contains(key);
        }
        Ok(true)
    }
}
impl Reader {
    /// Compile exact raw-ID constraints without reading symbol records.
    ///
    /// Empty clauses mean all rows. Empty ID sets/unions mean no rows. IDs must
    /// belong to this file; dimensions are zero-based and may appear only once.
    pub fn selection(
        &self,
        symbol: &SymbolHandle,
        clauses: &[DimensionSelection<'_>],
    ) -> Result<Selection> {
        self.check_handle(symbol)?;
        let dimension = self.resolve(&symbol.inner.record)?.dimension;
        let uels = self.source.counts()?.uels;
        let error = |kind, message| {
            let mut e = Error::new(kind, message).symbol(symbol.id().get());
            e.source_id = Some(self.source.id);
            e
        };
        let validate_id = |id: UelId| {
            if id.get() > uels {
                Err(error(
                    ErrorKind::InvalidReference,
                    "selection UEL ID outside source table",
                ))
            } else {
                Ok(())
            }
        };
        // Validate every request before copying any variable-sized payload.
        let mut seen = 0u32;
        for clause in clauses {
            if clause.dimension >= dimension {
                return Err(error(
                    ErrorKind::InvalidSelection,
                    "selection dimension outside symbol",
                ));
            }
            let bit = 1u32 << clause.dimension;
            if seen & bit != 0 {
                return Err(error(
                    ErrorKind::InvalidSelection,
                    "duplicate selection dimension",
                ));
            }
            seen |= bit;
            match clause.keys {
                KeySelection::All => {}
                KeySelection::One(id) => validate_id(id)?,
                KeySelection::AnyOf(ids) => {
                    for &id in ids {
                        validate_id(id)?;
                    }
                }
                KeySelection::Range(r) => {
                    validate_id(r.first)?;
                    validate_id(r.last)?;
                }
                KeySelection::Ranges(ranges) => {
                    for r in ranges {
                        validate_id(r.first)?;
                        validate_id(r.last)?;
                    }
                }
            }
        }
        if size_of::<State>() > 16 * 1024 {
            return Err(error(
                ErrorKind::BudgetTooSmall,
                "selection state exceeds audited bound",
            ));
        }
        let fixed = self.source.budget.reserve(16 * 1024)?;
        let mut constraints = std::array::from_fn(|_| Constraint::All);
        let mut empty = false;
        for clause in clauses {
            constraints[usize::from(clause.dimension)] = match clause.keys {
                KeySelection::All => Constraint::All,
                KeySelection::One(id) => Constraint::One(id),
                KeySelection::Range(r) => Constraint::Range(r),
                KeySelection::AnyOf([]) | KeySelection::Ranges([]) => {
                    empty = true;
                    Constraint::Empty
                }
                KeySelection::AnyOf(ids) => {
                    let mut buffer = Buffer::new(&self.source.budget, ids.len())?;
                    for &id in ids {
                        buffer.push(id)?;
                    }
                    buffer.data.sort_unstable_by_key(|id| id.get());
                    buffer.data.dedup();
                    Constraint::Set(buffer)
                }
                KeySelection::Ranges(ranges) => {
                    let mut buffer = Buffer::new(&self.source.budget, ranges.len())?;
                    for &range in ranges {
                        buffer.push(range)?;
                    }
                    buffer.data.sort_unstable_by_key(|r| r.first.get());
                    let mut length = 0;
                    for at in 0..buffer.data.len() {
                        let next = buffer.data[at];
                        if length != 0
                            && u64::from(next.first.get())
                                <= u64::from(buffer.data[length - 1].last.get()) + 1
                        {
                            if next.last.get() > buffer.data[length - 1].last.get() {
                                buffer.data[length - 1].last = next.last;
                            }
                        } else {
                            buffer.data[length] = next;
                            length += 1;
                        }
                    }
                    buffer.data.truncate(length);
                    Constraint::Ranges(buffer)
                }
            };
        }
        Ok(Selection {
            disk: None,
            closed: false,
            inner: Arc::new(State {
                source: Arc::clone(&self.source),
                symbol: symbol.id(),
                dimension,
                constraints,
                empty,
                _state: fixed,
            }),
        })
    }
    /// Filter before projection and the exact public-row limit, without an index.
    ///
    /// ```no_run
    /// use gdxrs::{Reader, OpenOptions, DimensionSelection, KeySelection, UelId};
    /// # fn example() -> gdxrs::Result<()> {
    /// let reader = Reader::open("model.gdx", OpenOptions::default())?;
    /// if let Some(symbol) = reader.find_symbol(b"demand")? {
    ///     let selection = reader.selection(&symbol, &[DimensionSelection {
    ///         dimension: 0, keys: KeySelection::One(UelId::new(42)?),
    ///     }])?;
    ///     let mut options = reader.scan_options();
    ///     options.limit = Some(10); // Ten matching rows, after rejection.
    ///     let mut cursor = reader.scan_selected(&symbol, &selection, options)?;
    ///     while let Some(batch) = cursor.next_batch()? {
    ///         println!("{} matching rows", batch.len());
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    pub fn scan_selected(
        &self,
        symbol: &SymbolHandle,
        selection: &Selection,
        options: ScanOptions,
    ) -> Result<ScanCursor> {
        selection.validate(self, symbol)?;
        ScanCursor::selected(self.clone(), symbol.clone(), selection.clone(), options)
    }
}

impl Selection {
    pub(crate) fn has_disk(&self) -> bool {
        self.disk.is_some()
    }
    pub(crate) fn with_disk(mut self, dimension: u8, ids: crate::spill::DiskIds) -> Result<Self> {
        if self.disk.is_some() {
            return Err(Error::new(
                ErrorKind::InvalidSelection,
                "selection already has a disk clause",
            ));
        }
        self.disk = Some((dimension, Arc::new(ids)));
        Ok(self)
    }
    pub fn spill_directory(&self) -> Option<&std::path::Path> {
        self.disk.as_ref().map(|(_, ids)| ids.directory())
    }
    pub fn spill_statistics(&self) -> Option<SpillStatistics> {
        self.disk.as_ref().map(|(_, ids)| ids.statistics())
    }
    /// Release disk scratch explicitly. Retained clones/scans prevent closure.
    pub fn close_spill(&mut self) -> Result<()> {
        if let Some((_, ids)) = &mut self.disk {
            let ids = Arc::get_mut(ids).ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidOptions,
                    "membership still shared by another owner",
                )
            })?;
            self.closed = true;
            ids.close()?;
            self.disk = None;
        }
        Ok(())
    }
}
