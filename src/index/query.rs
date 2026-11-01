use super::*;

/// How an explicit indexed scan is executing. Missing/rejected caches use serial
/// selection before any output; integrity errors during an active scan are fatal.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexScanMode {
    #[default]
    Off,
    Missing,
    Rejected,
    Indexed,
    Sequential,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct IndexedScanStatistics {
    pub mode: IndexScanMode,
    pub intervals_considered: u64,
    /// Proven nonmatching intervals, including ordered prefix/suffix skips.
    /// Can exceed intervals_considered, which counts inspected summaries.
    pub intervals_pruned: u64,
    pub intervals_skipped_by_leading_bounds: u64,
    pub boundary_probes: u64,
    pub adjacent_intervals_coalesced: u64,
    pub checkpoint_restarts: u64,
    pub index_pages_read: u64,
    pub index_cache_hits: u64,
    /// Work of the explicit strict open, separate from this query.
    pub validation: IndexStatistics,
    /// Source work during cursor calls. As with Reader::io_statistics, concurrent
    /// operations on the same source may contribute; calls between batches do not.
    pub source_io: IoStatistics,
}
pub(crate) struct Plan {
    pub(crate) index: Option<CheckpointIndex>,
    next: u64,
    end: u64,
    ordinal: u64,
    tail: bool,
    tail_end: Option<u64>,
    leading: Option<(u32, u32)>,
    sought: bool,
    observed: u64,
    misses: u64,
    statistics: IndexedScanStatistics,
    baseline: IndexStatistics,
    rejection: Option<Error>,
}
impl Plan {
    pub(crate) fn new(
        reader: &Reader,
        symbol: &SymbolHandle,
        selection: &Selection,
        open: IndexOpen,
    ) -> Result<Self> {
        let (mode, rejection) = match open.status {
            IndexStatus::Off => (IndexScanMode::Off, None),
            IndexStatus::Missing => (IndexScanMode::Missing, None),
            IndexStatus::Rejected(e) => (IndexScanMode::Rejected, Some(e)),
            IndexStatus::Ready => (IndexScanMode::Indexed, None),
        };
        if (mode == IndexScanMode::Indexed) != open.index.is_some() {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "inconsistent index open result",
            ));
        }
        if let Some(index) = &open.index {
            if !Arc::ptr_eq(&reader.source, &index.reader.source)
                || index.symbol.id() != symbol.id()
            {
                return Err(Error::new(
                    ErrorKind::SourceMismatch,
                    "index belongs to another source/symbol",
                ));
            }
            if let Some(e) = &index.error {
                return Err(e.clone());
            }
        }
        let baseline = open
            .index
            .as_ref()
            .map_or_else(IndexStatistics::default, CheckpointIndex::statistics);
        let validation = open.index.as_ref().map_or_else(
            IndexStatistics::default,
            CheckpointIndex::validation_statistics,
        );
        let tail = open.index.is_none() || !selection.can_prune_intervals();
        let mode = if open.index.is_some() && tail {
            IndexScanMode::Sequential
        } else {
            mode
        };
        Ok(Self {
            index: open.index,
            next: 0,
            end: 0,
            ordinal: 0,
            tail,
            tail_end: None,
            leading: selection.leading_bounds(),
            sought: false,
            observed: 0,
            misses: 0,
            statistics: IndexedScanStatistics {
                mode,
                validation,
                ..IndexedScanStatistics::default()
            },
            baseline,
            rejection,
        })
    }
    pub(crate) fn statistics(&self) -> IndexedScanStatistics {
        let mut statistics = self.statistics;
        if let Some(index) = &self.index {
            let current = index.statistics();
            statistics.index_pages_read = current.pages_read - self.baseline.pages_read;
            statistics.index_cache_hits = current.cache_hits - self.baseline.cache_hits;
        }
        statistics
    }
    pub(crate) fn rejection(&self) -> Option<&Error> {
        self.rejection.as_ref()
    }
    pub(crate) fn record_io(&mut self, before: IoStatistics, after: IoStatistics) -> Result<()> {
        let add = |target: &mut u64, first: u64, last: u64| -> Result<()> {
            *target = target
                .checked_add(last.checked_sub(first).ok_or_else(invalid)?)
                .ok_or_else(invalid)?;
            Ok(())
        };
        let io = &mut self.statistics.source_io;
        add(&mut io.read_calls, before.read_calls, after.read_calls)?;
        add(&mut io.bytes_read, before.bytes_read, after.bytes_read)?;
        add(
            &mut io.decoded_bytes,
            before.decoded_bytes,
            after.decoded_bytes,
        )?;
        add(
            &mut io.decoded_buffers,
            before.decoded_buffers,
            after.decoded_buffers,
        )
    }
    pub(crate) fn fallback(&mut self, error: Error) {
        self.tail = true;
        self.tail_end = None;
        self.ordinal = 0;
        self.statistics.mode = IndexScanMode::Rejected;
        self.rejection = Some(error);
    }
    pub(crate) fn decoded(&mut self) -> Result<()> {
        self.ordinal = self.ordinal.checked_add(1).ok_or_else(invalid)?;
        Ok(())
    }
    pub(crate) fn prepare(
        &mut self,
        reader: &Reader,
        symbol: &SymbolHandle,
        selection: &Selection,
        decoder: &mut Option<Decoder>,
    ) -> Result<bool> {
        if self.tail {
            return Ok(self.tail_end.is_none_or(|end| {
                self.ordinal < end
                    || self
                        .index
                        .as_ref()
                        .is_some_and(|index| end == index.validation_statistics.rows_decoded)
            }));
        }
        if self.ordinal < self.end {
            return Ok(true);
        }
        let index = self.index.as_mut().ok_or_else(invalid)?;
        if !self.sought {
            // Strict canonical replay proved lexicographic raw-key ordering.
            // First-axis maxima are monotone, even inside long leading groups.
            if let Some((lower, _)) = self.leading.filter(|(lower, _)| *lower > 1) {
                let (mut low, mut high) = (0, index.interval_count());
                while low < high {
                    let mid = low + (high - low) / 2;
                    let interval = index.interval(mid)?;
                    self.statistics.boundary_probes = self
                        .statistics
                        .boundary_probes
                        .checked_add(1)
                        .ok_or_else(invalid)?;
                    self.statistics.intervals_considered = self
                        .statistics
                        .intervals_considered
                        .checked_add(1)
                        .ok_or_else(invalid)?;
                    if interval
                        .key_bounds(0)
                        .is_none_or(|(_, maximum)| maximum.get() < lower)
                    {
                        low = mid.checked_add(1).ok_or_else(invalid)?;
                    } else {
                        high = mid;
                    }
                }
                self.next = low;
                self.statistics.intervals_pruned = low;
                self.statistics.intervals_skipped_by_leading_bounds = low;
            }
            self.sought = true;
        }
        while self.next < index.interval_count() {
            let interval = index.interval(self.next)?;
            if self.leading.is_some_and(|(_, upper)| {
                interval
                    .key_bounds(0)
                    .is_some_and(|(minimum, _)| minimum.get() > upper)
            }) {
                self.statistics.intervals_considered = self
                    .statistics
                    .intervals_considered
                    .checked_add(1)
                    .ok_or_else(invalid)?;
                let skipped = index
                    .interval_count()
                    .checked_sub(self.next)
                    .ok_or_else(invalid)?;
                self.statistics.intervals_pruned = self
                    .statistics
                    .intervals_pruned
                    .checked_add(skipped)
                    .ok_or_else(invalid)?;
                self.statistics.intervals_skipped_by_leading_bounds = self
                    .statistics
                    .intervals_skipped_by_leading_bounds
                    .checked_add(skipped)
                    .ok_or_else(invalid)?;
                self.next = index.interval_count();
                break;
            }
            self.next = self.next.checked_add(1).ok_or_else(invalid)?;
            self.observed = self.observed.checked_add(1).ok_or_else(invalid)?;
            self.statistics.intervals_considered = self
                .statistics
                .intervals_considered
                .checked_add(1)
                .ok_or_else(invalid)?;
            if !selection.may_match_interval(&interval) {
                self.misses = self.misses.checked_add(1).ok_or_else(invalid)?;
                self.statistics.intervals_pruned = self
                    .statistics
                    .intervals_pruned
                    .checked_add(1)
                    .ok_or_else(invalid)?;
                continue;
            }
            if decoder.is_some() && self.ordinal == interval.start {
                self.statistics.adjacent_intervals_coalesced = self
                    .statistics
                    .adjacent_intervals_coalesced
                    .checked_add(1)
                    .ok_or_else(invalid)?;
            } else {
                // Release the old decoder before admitting the new codec buffers.
                *decoder = None;
                *decoder = Some(Decoder::resume(
                    reader,
                    &symbol.inner.record,
                    &interval.checkpoint.position(),
                )?);
                self.statistics.checkpoint_restarts = self
                    .statistics
                    .checkpoint_restarts
                    .checked_add(1)
                    .ok_or_else(invalid)?;
            }
            self.ordinal = interval.start;
            self.end = interval.end;
            // Decide from observed interval work only. Never prescan summaries
            // or records to choose a plan for a short accepted-row limit.
            if self.observed >= 8 && self.misses <= self.observed / 8 {
                if let Some((_, upper)) = self.leading {
                    let (mut low, mut high) = (self.next, index.interval_count());
                    while low < high {
                        let mid = low + (high - low) / 2;
                        let probe = index.interval(mid)?;
                        self.statistics.boundary_probes = self
                            .statistics
                            .boundary_probes
                            .checked_add(1)
                            .ok_or_else(invalid)?;
                        self.statistics.intervals_considered = self
                            .statistics
                            .intervals_considered
                            .checked_add(1)
                            .ok_or_else(invalid)?;
                        let (minimum, _) = probe.key_bounds(0).ok_or_else(invalid)?;
                        if minimum.get() <= upper {
                            low = mid.checked_add(1).ok_or_else(invalid)?;
                        } else {
                            high = mid;
                        }
                    }
                    let end = if low == index.interval_count() {
                        index.validation_statistics.rows_decoded
                    } else {
                        self.statistics.boundary_probes = self
                            .statistics
                            .boundary_probes
                            .checked_add(1)
                            .ok_or_else(invalid)?;
                        self.statistics.intervals_considered = self
                            .statistics
                            .intervals_considered
                            .checked_add(1)
                            .ok_or_else(invalid)?;
                        index.interval(low)?.start
                    };
                    let skipped = index
                        .interval_count()
                        .checked_sub(low)
                        .ok_or_else(invalid)?;
                    self.statistics.intervals_pruned = self
                        .statistics
                        .intervals_pruned
                        .checked_add(skipped)
                        .ok_or_else(invalid)?;
                    self.statistics.intervals_skipped_by_leading_bounds = self
                        .statistics
                        .intervals_skipped_by_leading_bounds
                        .checked_add(skipped)
                        .ok_or_else(invalid)?;
                    self.tail_end = Some(end);
                }
                self.tail = true;
                self.statistics.mode = IndexScanMode::Sequential;
            }
            return Ok(true);
        }
        // A candidate ending exactly at EOF still owes its terminator. A pruned
        // suffix owes no decoding: its bounds were proved during explicit open.
        if self.ordinal == index.statistics.rows_decoded && decoder.is_some() {
            self.tail = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
impl Reader {
    /// Exact selection using an explicitly opened cache. This never opens,
    /// validates or builds a sidecar implicitly. Off/Missing/Rejected fall back
    /// before output. Ready must belong to this immutable source/requested symbol.
    /// Recover the validated session with ScanCursor::into_index for warm reuse.
    pub fn scan_selected_indexed(
        &self,
        symbol: &SymbolHandle,
        selection: &Selection,
        options: ScanOptions,
        open: IndexOpen,
    ) -> Result<ScanCursor> {
        selection.validate(self, symbol)?;
        let plan = Plan::new(self, symbol, selection, open)?;
        let mut cursor =
            ScanCursor::selected(self.clone(), symbol.clone(), selection.clone(), options)?;
        cursor.set_index_plan(plan);
        Ok(cursor)
    }
}
