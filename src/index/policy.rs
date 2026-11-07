use super::*;
use crate::{BatchView, ScanCursor, ScanOptions, ScanStatistics, Selection, SymbolHandle};

/// Measured execution choice for one explicit scan request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanChoice {
    Serial,
    Indexed,
    Parallel { workers: usize },
}
impl ScanChoice {
    pub fn workers(&self) -> usize {
        match self {
            Self::Parallel { workers } => *workers,
            _ => 1,
        }
    }
}

/// Caller-supplied reproducible measurements. The policy never measures, scans a
/// source or opens/builds a sidecar by itself; every input is explicit.
#[derive(Clone, Copy, Debug)]
pub struct ScanMeasurements {
    /// Verified public rows of the requested symbol.
    pub rows: u64,
    /// Pruning estimate for the requested selection, from index summaries or a
    /// representative measurement, never from scanning the whole source.
    pub selected_rows: u64,
    /// An already strictly validated index session for this source/symbol exists.
    pub session_valid: bool,
    /// Measured complete serial scan seconds for this source.
    pub serial_full_scan_seconds: f64,
    /// Measured warm indexed seconds for this query shape.
    pub indexed_selected_seconds: f64,
    /// Measured parallel full ordered scan seconds at `max_workers`.
    pub parallel_full_scan_seconds: f64,
    /// Measured strict reopen seconds of an existing sidecar.
    pub strict_reopen_seconds: f64,
    /// Measured explicit build seconds. Reported for break-even only; a plan
    /// never builds.
    pub build_seconds: f64,
}

/// Explicit conservative policy limits. No hidden or unmeasured threshold: every
/// bound is a caller-specified field.
#[derive(Clone, Copy, Debug)]
pub struct ScanPolicy {
    /// Explicit override; when set, every other rule only validates the choice.
    pub choice_override: Option<ScanChoice>,
    /// Caller concurrency limit, one through eight.
    pub max_workers: usize,
    /// Whether a plan may charge a strict reopen of an existing sidecar. A plan
    /// never builds or opens implicitly when this is false and no session exists.
    pub allow_strict_reopen: bool,
    /// At or below this many rows the conservative choice is serial.
    pub small_rows: u64,
    /// Accepted limits at or below this choose serial (head/zero queries).
    pub head_limit: u64,
    /// Indexed is poor-pruning-disallowed unless the selected fraction is
    /// strictly below this value.
    pub min_prune_fraction: f64,
    /// Parallel requires a measured advantage of at least this factor over
    /// serial before admission; guards against admission pressure.
    pub required_parallel_advantage: f64,
}
impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            choice_override: None,
            max_workers: 8,
            allow_strict_reopen: false,
            small_rows: 65_536,
            head_limit: 1_024,
            min_prune_fraction: 0.5,
            required_parallel_advantage: 1.0,
        }
    }
}

/// One considered alternative with its disposition.
#[derive(Clone, Copy, Debug)]
pub struct PlannedAlternative {
    pub choice: ScanChoice,
    pub estimated_seconds: f64,
    pub allowed: bool,
    pub reason: &'static str,
}

/// Observable result of a measured plan.
#[derive(Clone, Copy, Debug)]
pub struct ScanPlan {
    pub choice: ScanChoice,
    pub estimated_seconds: f64,
    /// Strict-reopen seconds charged to the chosen plan.
    pub reopen_seconds_charged: f64,
    /// Queries needed to amortize a measured build plus strict reopen against the
    /// measured warm saving, when the saving is positive.
    pub build_break_even_queries: Option<u64>,
    /// Fixed order: serial, indexed, parallel.
    pub alternatives: [PlannedAlternative; 3],
}

impl ScanPolicy {
    /// Deterministic pure choice over explicit measurements. Performs no I/O and
    /// never scans a source or directory to decide.
    pub fn plan(&self, m: &ScanMeasurements, limit: Option<u64>) -> Result<ScanPlan> {
        self.validate()?;
        if m.selected_rows > m.rows
            || ![
                m.serial_full_scan_seconds,
                m.indexed_selected_seconds,
                m.parallel_full_scan_seconds,
                m.strict_reopen_seconds,
                m.build_seconds,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid scan measurements",
            ));
        }
        let reopen_charge = if m.session_valid {
            0.0
        } else if self.allow_strict_reopen {
            m.strict_reopen_seconds
        } else {
            f64::INFINITY
        };
        let serial_cost = m.serial_full_scan_seconds;
        let indexed_cost = reopen_charge + m.indexed_selected_seconds;
        let parallel_cost = reopen_charge + m.parallel_full_scan_seconds;
        let mut alternatives = [
            PlannedAlternative {
                choice: ScanChoice::Serial,
                estimated_seconds: serial_cost,
                allowed: true,
                reason: "serial always available",
            },
            PlannedAlternative {
                choice: ScanChoice::Indexed,
                estimated_seconds: indexed_cost,
                allowed: false,
                reason: "",
            },
            PlannedAlternative {
                choice: ScanChoice::Parallel {
                    workers: self.max_workers,
                },
                estimated_seconds: parallel_cost,
                allowed: false,
                reason: "",
            },
        ];
        let forced_serial =
            limit.is_some_and(|l| l <= self.head_limit) || m.rows <= self.small_rows;
        if let Some(choice) = self.choice_override {
            if let ScanChoice::Parallel { workers } = choice
                && (workers == 0 || workers > self.max_workers)
            {
                return Err(Error::new(
                    ErrorKind::InvalidOptions,
                    "override exceeds caller worker limit",
                ));
            }
            let index = match choice {
                ScanChoice::Serial => 0,
                ScanChoice::Indexed => 1,
                ScanChoice::Parallel { .. } => 2,
            };
            for (i, alt) in alternatives.iter_mut().enumerate() {
                alt.allowed = false;
                alt.reason = "explicit override";
                if i == index {
                    alt.allowed = true;
                    alt.reason = "explicit override";
                }
            }
            alternatives[index].choice = choice;
            let chosen = alternatives[index];
            let uses_index = !matches!(choice, ScanChoice::Serial);
            return Ok(ScanPlan {
                choice,
                estimated_seconds: chosen.estimated_seconds,
                reopen_seconds_charged: if uses_index && reopen_charge.is_finite() {
                    reopen_charge
                } else {
                    0.0
                },
                build_break_even_queries: Self::break_even(self, m),
                alternatives,
            });
        }
        if forced_serial {
            alternatives[0].reason = if m.rows <= self.small_rows {
                "small input"
            } else {
                "head/zero limit"
            };
            alternatives[1].reason = "conservative serial for head/zero or small input";
            alternatives[2].reason = "conservative serial for head/zero or small input";
            return Ok(ScanPlan {
                choice: ScanChoice::Serial,
                estimated_seconds: serial_cost,
                reopen_seconds_charged: 0.0,
                build_break_even_queries: Self::break_even(self, m),
                alternatives,
            });
        }
        let indexable = reopen_charge.is_finite();
        if indexable {
            let prune_fraction = if m.rows == 0 {
                0.0
            } else {
                m.selected_rows as f64 / m.rows as f64
            };
            if prune_fraction < self.min_prune_fraction {
                alternatives[1].allowed = true;
                alternatives[1].reason = "pruning estimate within limit";
            } else {
                alternatives[1].reason = "poor pruning";
            }
            if parallel_cost * self.required_parallel_advantage <= serial_cost {
                alternatives[2].allowed = true;
                alternatives[2].reason = "measured parallel advantage within limit";
            } else {
                alternatives[2].reason = "admission pressure: parallel advantage below limit";
            }
        } else {
            alternatives[1].reason = "no valid session and strict reopen disallowed";
            alternatives[2].reason = "no valid session and strict reopen disallowed";
        }
        let chosen = alternatives
            .iter()
            .enumerate()
            .filter(|(_, a)| a.allowed)
            .min_by(|(ia, a), (ib, b)| {
                a.estimated_seconds
                    .partial_cmp(&b.estimated_seconds)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(ia.cmp(ib))
            })
            .map(|(i, a)| (i, *a))
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "no allowed scan choice"))?;
        let uses_index = !matches!(chosen.1.choice, ScanChoice::Serial);
        Ok(ScanPlan {
            choice: chosen.1.choice,
            estimated_seconds: chosen.1.estimated_seconds,
            reopen_seconds_charged: if uses_index && reopen_charge.is_finite() {
                reopen_charge
            } else {
                0.0
            },
            build_break_even_queries: Self::break_even(self, m),
            alternatives,
        })
    }
    fn validate(&self) -> Result<()> {
        if !(1..=8).contains(&self.max_workers)
            || !self.min_prune_fraction.is_finite()
            || self.min_prune_fraction <= 0.0
            || self.min_prune_fraction > 1.0
            || !self.required_parallel_advantage.is_finite()
            || self.required_parallel_advantage < 1.0
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid scan policy limits",
            ));
        }
        Ok(())
    }
    fn break_even(&self, m: &ScanMeasurements) -> Option<u64> {
        if !m.session_valid {
            return None;
        }
        let saving = m.serial_full_scan_seconds - m.indexed_selected_seconds;
        if saving > 0.0 {
            Some(
                ((m.build_seconds + m.strict_reopen_seconds) / saving)
                    .ceil()
                    .max(1.0) as u64,
            )
        } else {
            None
        }
    }
}

/// Statistics of an executed planned scan.
#[derive(Clone, Copy, Debug)]
pub enum PlannedStatistics {
    Serial(ScanStatistics),
    Parallel(ParallelStatistics),
}

/// A planned scan executed through one of the existing exact cursors. Filters,
/// projection, global accepted limit, source trust and acknowledgment semantics
/// are identical across choices.
pub enum PlannedScan {
    Serial(Box<ScanCursor>),
    Parallel(Box<ParallelScanCursor>),
}
impl PlannedScan {
    pub fn next_batch(&mut self) -> Result<Option<BatchView<'_>>> {
        match self {
            Self::Serial(cursor) => cursor.next_batch(),
            Self::Parallel(cursor) => cursor.next_batch(),
        }
    }
    pub fn statistics(&self) -> PlannedStatistics {
        match self {
            Self::Serial(cursor) => PlannedStatistics::Serial(cursor.statistics()),
            Self::Parallel(cursor) => PlannedStatistics::Parallel(cursor.statistics()),
        }
    }
    /// Fallible cancellation/cleanup; parallel joins workers, serial stops at the
    /// acknowledged prefix and releases its buffers. Idempotent; sticky errors
    /// are reported, never hidden.
    pub fn close(&mut self) -> Result<()> {
        match self {
            Self::Serial(cursor) => cursor.close(),
            Self::Parallel(cursor) => cursor.close(),
        }
    }
}

impl Reader {
    /// Plan from explicit measurements, then execute the chosen exact scan. An
    /// `Indexed`/`Parallel` choice requires the caller's already opened
    /// `IndexOpen` with a Ready session; nothing is built or opened implicitly.
    pub fn scan_planned(
        &self,
        symbol: &SymbolHandle,
        selection: &Selection,
        options: ScanOptions,
        open: Option<IndexOpen>,
        policy: &ScanPolicy,
        measurements: &ScanMeasurements,
    ) -> Result<(ScanPlan, PlannedScan)> {
        selection.validate(self, symbol)?;
        let plan = policy.plan(measurements, options.limit)?;
        let scan = match plan.choice {
            ScanChoice::Serial => {
                PlannedScan::Serial(Box::new(self.scan_selected(symbol, selection, options)?))
            }
            ScanChoice::Indexed => {
                let open = open.ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOptions,
                        "indexed plan requires an explicit open",
                    )
                })?;
                if !matches!(open.status, IndexStatus::Ready) || open.index.is_none() {
                    return Err(Error::new(
                        ErrorKind::InvalidOptions,
                        "indexed plan requires a Ready session; never opened implicitly",
                    ));
                }
                PlannedScan::Serial(Box::new(
                    self.scan_selected_indexed(symbol, selection, options, open)?,
                ))
            }
            ScanChoice::Parallel { workers } => {
                let open = open.ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOptions,
                        "parallel plan requires an explicit open",
                    )
                })?;
                let index = open.index.ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOptions,
                        "parallel plan requires a Ready session; never opened implicitly",
                    )
                })?;
                PlannedScan::Parallel(Box::new(self.scan_selected_parallel(
                    symbol,
                    selection,
                    index,
                    options,
                    ParallelOptions {
                        workers,
                        maximum_in_flight_tasks: workers,
                        ..ParallelOptions::default()
                    },
                )?))
            }
        };
        Ok((plan, scan))
    }
}
