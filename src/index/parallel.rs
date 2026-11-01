use super::*;
use crate::scan::{Batch, ParallelStep};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

const MAX_WORKERS: usize = 8;
const GRANULE: usize = 64 * 1024;
const THREAD_CONTROL: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Whether budget admission may reduce the requested concurrency before spawn.
pub enum ParallelAdmission {
    /// Reject budget denial; task count may still cap concurrency naturally.
    Exact,
    /// Retry only budget denial with fewer workers, never below one.
    Reduce,
}
#[derive(Clone, Copy, Debug)]
/// Explicit bounded execution options. No automatic index build or serial fallback.
pub struct ParallelOptions {
    /// Requested concurrency, from one through eight.
    pub workers: usize,
    /// Upper bound on active tasks, from `workers` through eight.
    /// This implementation admits at most one task/batch per worker.
    pub maximum_in_flight_tasks: usize,
    /// Consecutive authenticated intervals grouped per task, one through 1,000,000.
    pub intervals_per_task: u64,
    /// Explicit stack size, two through eight MiB; rounded up to 64 KiB.
    /// The shared root also reserves a 64 KiB guard allowance per worker.
    pub worker_stack_bytes: usize,
    pub admission: ParallelAdmission,
}
impl Default for ParallelOptions {
    fn default() -> Self {
        Self {
            workers: 2,
            maximum_in_flight_tasks: 2,
            intervals_per_task: 1,
            worker_stack_bytes: 2 * 1024 * 1024,
            admission: ParallelAdmission::Exact,
        }
    }
}
impl ParallelOptions {
    fn validate(self) -> Result<usize> {
        if !(1..=MAX_WORKERS).contains(&self.workers)
            || !(self.workers..=MAX_WORKERS).contains(&self.maximum_in_flight_tasks)
            || !(1..=1_000_000).contains(&self.intervals_per_task)
            || !(2 * 1024 * 1024..=8 * 1024 * 1024).contains(&self.worker_stack_bytes)
        {
            return Err(Error::new(
                ErrorKind::InvalidOptions,
                "invalid parallel worker/slot/stack options",
            ));
        }
        self.worker_stack_bytes
            .checked_add(GRANULE - 1)
            .map(|n| n / GRANULE * GRANULE)
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "parallel stack overflow"))
    }
}
#[derive(Clone, Copy, Debug, Default)]
/// Execution counters. Active work is a snapshot at completed decoder steps;
/// after joining it includes speculative/rejected work. Returned rows count only
/// acknowledged batches. Source I/O is available from the Reader's aggregate counters.
pub struct ParallelStatistics {
    pub requested_workers: usize,
    pub admitted_workers: usize,
    pub stack_reserved_bytes: usize,
    pub control_reserved_bytes: usize,
    pub maximum_buffered_batches: usize,
    pub peak_ready_batches: usize,
    pub tasks_dispatched: u64,
    pub tasks_completed: u64,
    pub decoded_rows: u64,
    pub produced_rows: u64,
    pub returned_rows: u64,
    pub joined_workers: usize,
    pub complete: bool,
    pub cancelled: bool,
}
struct Task {
    id: u64,
    cursor: ScanCursor,
}
struct Slot {
    task: Option<Task>,
    ready: Option<(u64, Batch)>,
    recycled: Option<Batch>,
    finished: Option<Task>,
}
impl Slot {
    fn new() -> Self {
        Self {
            task: None,
            ready: None,
            recycled: None,
            finished: None,
        }
    }
}
struct State {
    slots: Buffer<Slot>,
    error: Option<Error>,
    work: ParallelStatistics,
    ready_count: usize,
}
struct Signal {
    gate: Mutex<()>,
    wake: Condvar,
    stop: AtomicBool,
    user_cancelled: AtomicBool,
    poisoned: AtomicBool,
    _state: Reservation,
}
impl Signal {
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().unwrap_or_else(|poison| {
            self.poisoned.store(true, Ordering::Release);
            self.stop.store(true, Ordering::Release);
            poison.into_inner()
        })
    }
    fn notify(&self) {
        let _gate = self.lock();
        self.wake.notify_all();
    }
    fn request_stop(&self, user: bool) {
        let _gate = self.lock();
        if user {
            self.user_cancelled.store(true, Ordering::Release);
        }
        self.stop.store(true, Ordering::Release);
        self.wake.notify_all();
    }
}
/// Cloneable stop request. The cursor's close/next_batch joins and reports errors.
/// A retained handle owns only a small charged signal, not task/result buffers.
#[derive(Clone)]
pub struct ParallelCancellation {
    signal: Arc<Signal>,
}
impl ParallelCancellation {
    pub fn request_cancel(&self) {
        self.signal.request_stop(true);
    }
}
struct Shared {
    state: Mutex<State>,
    signal: Arc<Signal>,
    _state: Reservation,
    #[cfg(test)]
    hooks: Arc<tests::Hooks>,
}
fn broken() -> Error {
    Error::new(ErrorKind::WorkerFailure, "parallel worker state failed")
}
impl Shared {
    fn recover<'a>(
        &self,
        result: std::sync::LockResult<MutexGuard<'a, State>>,
    ) -> MutexGuard<'a, State> {
        match result {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                if guard.error.is_none() {
                    guard.error = Some(broken());
                }
                self.signal.request_stop(false);
                guard
            }
        }
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        let mut state = self.recover(self.state.lock());
        if self.signal.poisoned.load(Ordering::Acquire) && state.error.is_none() {
            state.error = Some(broken());
        }
        state
    }
    fn wait<'a>(&'a self, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        // Lock order is state -> gate. Registration and stop/notify use the
        // same gate, closing the check-then-wait missed-wakeup window. Never
        // reacquire state while holding the gate.
        let gate = self.signal.lock();
        if self.signal.stop.load(Ordering::Acquire) {
            drop(gate);
            return guard;
        }
        drop(guard);
        let gate = self.signal.wake.wait(gate).unwrap_or_else(|poison| {
            self.signal.poisoned.store(true, Ordering::Release);
            self.signal.stop.store(true, Ordering::Release);
            poison.into_inner()
        });
        drop(gate);
        self.lock()
    }
    fn fail(&self, error: Error) {
        let mut state = self.lock();
        if state.error.is_none() {
            state.error = Some(error);
        }
        self.signal.request_stop(false);
    }
}
struct Prepared {
    shared: Arc<Shared>,
    handles: Buffer<JoinHandle<()>>,
    next_interval: u64,
    _stacks: Reservation,
}
struct Setup<'a> {
    reader: &'a Reader,
    symbol: &'a SymbolHandle,
    selection: Option<&'a Selection>,
    scan: &'a ScanOptions,
    options: ParallelOptions,
    #[cfg(test)]
    hooks: &'a Arc<tests::Hooks>,
}
impl Setup<'_> {
    fn task(&self, index: &mut CheckpointIndex, next: &mut u64, id: u64) -> Result<Task> {
        let (reader, symbol, selection, options, group) = (
            self.reader,
            self.symbol,
            self.selection,
            self.scan,
            self.options.intervals_per_task,
        );
        let left = index
            .interval_count()
            .checked_sub(*next)
            .ok_or_else(broken)?;
        if left == 0 {
            return Err(broken());
        }
        let count = left.min(group);
        let first = index.interval(*next)?;
        let last = next.checked_add(count - 1).ok_or_else(broken)?;
        let end = if count == 1 {
            first.end
        } else {
            index.interval(last)?.end
        };
        let mut local = options.clone();
        local.limit = None;
        let mut cursor = match selection {
            Some(selection) => {
                reader.scan_selected_range(symbol, selection, &first.checkpoint, end, local)?
            }
            None => reader.scan_range(symbol, &first.checkpoint, end, local)?,
        };
        cursor.prepare_parallel(self.scan.limit)?;
        *next = next.checked_add(count).ok_or_else(broken)?;
        Ok(Task { id, cursor })
    }
}
impl Prepared {
    fn new(
        setup: &Setup<'_>,
        index: &mut CheckpointIndex,
        workers: usize,
        stack: usize,
        signal: &Arc<Signal>,
    ) -> Result<Self> {
        let reader = setup.reader;
        let options = setup.options;
        let stack_bytes = stack
            .checked_add(GRANULE)
            .and_then(|s| s.checked_mul(workers))
            .ok_or_else(broken)?;
        let thread_bytes = THREAD_CONTROL.checked_mul(workers).ok_or_else(broken)?;
        let stacks = reader
            .source
            .budget
            .reserve(stack_bytes.checked_add(thread_bytes).ok_or_else(broken)?)?;
        // Fixed small Arc/Mutex/packet control; variable slot/handle arrays have
        // separate fallible exact-capacity reservations below.
        let state = reader
            .source
            .budget
            .reserve(std::mem::size_of::<Shared>() + 256)?;
        let handles = Buffer::new(&reader.source.budget, workers)?;
        let mut slots = Buffer::new(&reader.source.budget, workers)?;
        let mut next = 0;
        for id in 0..workers {
            let mut slot = Slot::new();
            slot.task = Some(setup.task(index, &mut next, id as u64)?);
            slots.push(slot)?;
        }
        let control = thread_bytes
            + std::mem::size_of::<Shared>()
            + std::mem::size_of::<ParallelScanCursor>()
            + std::mem::size_of::<Signal>()
            + 64
            + 256
            + workers * (std::mem::size_of::<Slot>() + std::mem::size_of::<JoinHandle<()>>());
        let work = ParallelStatistics {
            requested_workers: options.workers,
            admitted_workers: workers,
            stack_reserved_bytes: stack_bytes,
            control_reserved_bytes: control,
            maximum_buffered_batches: workers,
            tasks_dispatched: workers as u64,
            ..ParallelStatistics::default()
        };
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    slots,
                    error: None,
                    work,
                    ready_count: 0,
                }),
                signal: Arc::clone(signal),
                _state: state,
                #[cfg(test)]
                hooks: Arc::clone(setup.hooks),
            }),
            handles,
            next_interval: next,
            _stacks: stacks,
        })
    }
}

fn worker(shared: &Arc<Shared>, slot: usize) -> Result<()> {
    loop {
        let mut task = {
            let mut state = shared.lock();
            loop {
                if shared.signal.stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                if let Some(task) = state
                    .slots
                    .data
                    .get_mut(slot)
                    .ok_or_else(broken)?
                    .task
                    .take()
                {
                    #[cfg(test)]
                    tests::waiting(shared, slot, 0);
                    break task;
                }
                #[cfg(test)]
                tests::waiting(shared, slot, 1);
                state = shared.wait(state);
            }
        };
        #[cfg(test)]
        tests::pause(shared, slot, false);
        loop {
            if shared.signal.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let before = task.cursor.statistics();
            let result = catch_unwind(AssertUnwindSafe(|| {
                #[cfg(test)]
                tests::before_step(shared, slot);
                task.cursor.parallel_step()
            }))
            .unwrap_or_else(|_| Err(broken()));
            let after = task.cursor.statistics();
            let mut state = shared.lock();
            state.work.decoded_rows = state
                .work
                .decoded_rows
                .checked_add(
                    after
                        .decoded_rows
                        .checked_sub(before.decoded_rows)
                        .ok_or_else(broken)?,
                )
                .ok_or_else(broken)?;
            state.work.produced_rows = state
                .work
                .produced_rows
                .checked_add(
                    after
                        .returned_rows
                        .checked_sub(before.returned_rows)
                        .ok_or_else(broken)?,
                )
                .ok_or_else(broken)?;
            if let Err(error) = &result {
                if state.error.is_none() {
                    state.error = Some(error.clone());
                }
                shared.signal.request_stop(false);
                shared.signal.notify();
                return Ok(());
            }
            if shared.signal.stop.load(Ordering::Acquire) {
                shared.signal.notify();
                return Ok(());
            }
            match result? {
                (ParallelStep::Pending, None) => {
                    drop(state);
                    #[cfg(test)]
                    tests::pause(shared, slot, true);
                }
                (ParallelStep::Complete, None) => {
                    let cell = state.slots.data.get_mut(slot).ok_or_else(broken)?;
                    if cell.finished.is_some() {
                        return Err(broken());
                    }
                    cell.finished = Some(task);
                    state.work.tasks_completed = state
                        .work
                        .tasks_completed
                        .checked_add(1)
                        .ok_or_else(broken)?;
                    shared.signal.notify();
                    break;
                }
                (ParallelStep::Batch, Some(batch)) => {
                    let cell = state.slots.data.get_mut(slot).ok_or_else(broken)?;
                    if cell.ready.is_some() || cell.recycled.is_some() {
                        return Err(broken());
                    }
                    cell.ready = Some((task.id, batch));
                    state.ready_count = state.ready_count.checked_add(1).ok_or_else(broken)?;
                    state.work.peak_ready_batches =
                        state.work.peak_ready_batches.max(state.ready_count);
                    shared.signal.notify();
                    #[cfg(test)]
                    tests::waiting(shared, slot, 3);
                    loop {
                        if shared.signal.stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        if let Some(batch) = state
                            .slots
                            .data
                            .get_mut(slot)
                            .ok_or_else(broken)?
                            .recycled
                            .take()
                        {
                            task.cursor.recycle_parallel(batch);
                            #[cfg(test)]
                            tests::waiting(shared, slot, 0);
                            break;
                        }
                        state = shared.wait(state);
                    }
                }
                _ => return Err(broken()),
            }
        }
    }
}

/// Explicit ordered worker cursor. Drop cancels/joins; use close for diagnostics.
/// A borrowed batch prevents advancing or closing the cursor until it is released.
///
/// ```compile_fail
/// use gdxrs::{ParallelScanCursor, Result};
/// fn overlapping(cursor: &mut ParallelScanCursor) -> Result<()> {
///     let batch = cursor.next_batch()?.unwrap();
///     cursor.next_batch()?;
///     println!("{}", batch.len());
///     Ok(())
/// }
/// ```
pub struct ParallelScanCursor {
    reader: Reader,
    symbol: SymbolHandle,
    selection: Option<Selection>,
    index: Option<CheckpointIndex>,
    scan: ScanOptions,
    options: ParallelOptions,
    prepared: Option<Prepared>,
    current: Option<(usize, Batch)>,
    frontier: u64,
    dispatched: u64,
    total_tasks: u64,
    remaining: u64,
    status: ParallelStatistics,
    closed: bool,
    error: Option<Error>,
    signal: Arc<Signal>,
    _state: Reservation,
}
impl ParallelScanCursor {
    fn new(
        reader: &Reader,
        symbol: &SymbolHandle,
        selection: Option<&Selection>,
        index: CheckpointIndex,
        scan: ScanOptions,
        options: ParallelOptions,
    ) -> Result<Self> {
        #[cfg(test)]
        {
            Self::new_inner(
                reader,
                symbol,
                selection,
                index,
                scan,
                options,
                Arc::new(tests::Hooks::default()),
            )
        }
        #[cfg(not(test))]
        {
            Self::new_inner(reader, symbol, selection, index, scan, options)
        }
    }
    fn new_inner(
        reader: &Reader,
        symbol: &SymbolHandle,
        selection: Option<&Selection>,
        mut index: CheckpointIndex,
        scan: ScanOptions,
        options: ParallelOptions,
        #[cfg(test)] hooks: Arc<tests::Hooks>,
    ) -> Result<Self> {
        let stack = options.validate()?;
        reader.check_handle(symbol)?;
        if !Arc::ptr_eq(&reader.source, &index.reader.source) || symbol.id() != index.symbol.id() {
            return Err(Error::new(
                ErrorKind::SourceMismatch,
                "parallel index belongs to another source/symbol",
            ));
        }
        if let Some(error) = &index.error {
            return Err(error.clone());
        }
        if let Some(selection) = selection {
            selection.validate(reader, symbol)?;
        }
        reader.source.check_unchanged()?;
        // Validate even zero-limit projections/caps without reading records.
        drop(ScanCursor::new(
            reader.clone(),
            symbol.clone(),
            scan.clone(),
        )?);
        let total = index.validation_statistics().rows_decoded;
        let lazy =
            scan.limit == Some(0) || total == 0 || selection.is_some_and(Selection::is_empty);
        let count = index.interval_count();
        let tasks = if lazy {
            0
        } else {
            count
                .checked_sub(1)
                .and_then(|n| (n / options.intervals_per_task).checked_add(1))
                .ok_or_else(broken)?
        };
        let state = reader.source.budget.reserve(std::mem::size_of::<Self>())?;
        let signal = Arc::new(Signal {
            gate: Mutex::new(()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            user_cancelled: AtomicBool::new(false),
            poisoned: AtomicBool::new(false),
            _state: reader
                .source
                .budget
                .reserve(std::mem::size_of::<Signal>() + 64)?,
        });
        let prepared = if lazy {
            None
        } else {
            let setup = Setup {
                reader,
                symbol,
                selection,
                scan: &scan,
                options,
                #[cfg(test)]
                hooks: &hooks,
            };
            let mut workers = options
                .workers
                .min(usize::try_from(tasks).unwrap_or(usize::MAX));
            loop {
                match Prepared::new(&setup, &mut index, workers, stack, &signal) {
                    Ok(prepared) => break Some(prepared),
                    Err(error)
                        if error.kind == ErrorKind::BudgetTooSmall
                            && options.admission == ParallelAdmission::Reduce
                            && workers > 1 =>
                    {
                        workers -= 1
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        let admitted = prepared
            .as_ref()
            .map_or(0, |p| p.shared.lock().work.admitted_workers);
        let mut cursor = Self {
            reader: reader.clone(),
            symbol: symbol.clone(),
            selection: selection.cloned(),
            index: Some(index),
            remaining: scan.limit.unwrap_or(u64::MAX),
            scan,
            options,
            prepared,
            current: None,
            frontier: 0,
            dispatched: admitted as u64,
            total_tasks: tasks,
            status: ParallelStatistics {
                requested_workers: options.workers,
                control_reserved_bytes: std::mem::size_of::<Self>()
                    + std::mem::size_of::<Signal>()
                    + 64,
                ..ParallelStatistics::default()
            },
            closed: lazy,
            error: None,
            signal,
            _state: state,
        };
        if lazy {
            cursor.status.complete = true;
            return Ok(cursor);
        }
        for slot in 0..admitted {
            #[cfg(test)]
            if let Some(error) =
                tests::spawn_error(&cursor.prepared.as_ref().ok_or_else(broken)?.shared, slot)
            {
                return Err(cursor.fail(error));
            }
            let handles = &cursor.prepared.as_ref().ok_or_else(broken)?.handles.data;
            if handles.len() == handles.capacity() {
                return Err(cursor.fail(broken()));
            }
            let shared = Arc::clone(&cursor.prepared.as_ref().ok_or_else(broken)?.shared);
            let failure = shared.lock().error.clone();
            if let Some(error) = failure {
                return Err(cursor.fail(error));
            }
            let handle = match thread::Builder::new().stack_size(stack).spawn(move || {
                #[cfg(test)]
                shared.hooks.started.fetch_add(1, Ordering::SeqCst);
                let result = catch_unwind(AssertUnwindSafe(|| worker(&shared, slot)))
                    .unwrap_or_else(|_| Err(broken()));
                if let Err(error) = result {
                    shared.fail(error);
                }
                #[cfg(test)]
                {
                    tests::after_exit(&shared, slot);
                    shared.hooks.exited.fetch_add(1, Ordering::SeqCst);
                    if shared.hooks.panic_join.load(Ordering::SeqCst) == slot {
                        panic!("injected join panic");
                    }
                }
            }) {
                Ok(handle) => handle,
                Err(error) => return Err(cursor.fail(Error::io(error))),
            };
            // Capacity was admitted exactly, and push cannot allocate.
            cursor
                .prepared
                .as_mut()
                .ok_or_else(broken)?
                .handles
                .data
                .push(handle);
        }
        Ok(cursor)
    }
    pub fn statistics(&self) -> ParallelStatistics {
        let mut s = self
            .prepared
            .as_ref()
            .map_or(self.status, |p| p.shared.lock().work);
        s.returned_rows = self.status.returned_rows;
        s.joined_workers = self.status.joined_workers;
        s.complete = self.status.complete;
        s.cancelled = self.status.cancelled;
        s
    }
    pub fn cancellation_handle(&self) -> ParallelCancellation {
        ParallelCancellation {
            signal: Arc::clone(&self.signal),
        }
    }
    fn shutdown(&mut self) -> Result<()> {
        self.closed = true;
        self.signal.request_stop(false);
        if let Some(mut prepared) = self.prepared.take() {
            prepared.shared.signal.request_stop(false);
            prepared.shared.signal.notify();
            let mut join_errors = 0;
            while let Some(handle) = prepared.handles.data.pop() {
                if handle.join().is_err() {
                    join_errors += 1;
                }
                self.status.joined_workers += 1;
            }
            let mut state = prepared.shared.lock();
            let mut status = state.work;
            status.returned_rows = self.status.returned_rows;
            status.joined_workers = self.status.joined_workers;
            status.complete = self.status.complete;
            status.cancelled = self.status.cancelled;
            self.status = status;
            if self.error.is_none() {
                self.error = state.error.take();
            }
            for _ in 0..join_errors {
                let secondary = io::Error::other("parallel worker panicked while joining");
                self.error = Some(match self.error.take() {
                    Some(primary) => primary.append_cleanup(secondary),
                    None => Error::worker_failure(secondary),
                });
            }
        }
        self.selection = None;
        if self.signal.user_cancelled.load(Ordering::Acquire) {
            self.status.cancelled = true;
            self.status.complete = false;
        }
        if let Some(error) = &mut self.error {
            error.source_id = Some(self.reader.source.id);
            error.symbol_id = Some(self.symbol.id().get());
            self.status.complete = false;
        }
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn fail(&mut self, error: Error) -> Error {
        self.status.complete = false;
        if let Some(prepared) = &self.prepared {
            prepared.shared.fail(error.clone());
        }
        if self.error.is_none() {
            self.error = Some(
                self.prepared
                    .as_ref()
                    .and_then(|p| p.shared.lock().error.clone())
                    .unwrap_or(error),
            );
        }
        self.current = None;
        let _ = self.shutdown();
        self.index = None;
        self.error.clone().unwrap_or_else(broken)
    }
    /// Stop without draining, wake all waiters and join. Cancellation is not EOF.
    pub fn cancel(&mut self) -> Result<()> {
        if !self.closed {
            self.status.cancelled = true;
            self.cancellation_handle().request_cancel();
        }
        self.current = None;
        let result = self.shutdown();
        self.index = None;
        result
    }
    /// Fallible cancellation/cleanup; idempotent, including sticky errors.
    pub fn close(&mut self) -> Result<()> {
        self.cancel()
    }
    /// Cancel/join and recover the explicitly validated session for reuse.
    /// Close/cancel release the index instead. Failures are never hidden.
    pub fn into_index(mut self) -> Result<CheckpointIndex> {
        self.current = None;
        self.shutdown()?;
        self.index
            .take()
            .ok_or_else(|| Error::new(ErrorKind::InvalidOptions, "parallel index already released"))
    }
    fn advance(&mut self) -> Result<bool> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.closed {
            self.current = None;
            return Ok(false);
        }
        let shared = Arc::clone(&self.prepared.as_ref().ok_or_else(broken)?.shared);
        if self.signal.stop.load(Ordering::Acquire) {
            self.shutdown()?;
            return Ok(false);
        }
        if let Some((slot, batch)) = self.current.take() {
            let mut state = shared.lock();
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            let cell = state.slots.data.get_mut(slot).ok_or_else(broken)?;
            if cell.recycled.is_some() {
                return Err(broken());
            }
            cell.recycled = Some(batch);
            shared.signal.notify();
        }
        loop {
            if self.signal.stop.load(Ordering::Acquire) {
                self.shutdown()?;
                return Ok(false);
            }
            if self.frontier == self.total_tasks {
                self.reader.source.check_unchanged()?;
                if self.signal.stop.load(Ordering::Acquire) {
                    self.shutdown()?;
                    return Ok(false);
                }
                self.status.complete = true;
                self.shutdown()?;
                self.reader.source.check_unchanged()?;
                return Ok(false);
            }
            let workers = self
                .prepared
                .as_ref()
                .ok_or_else(broken)?
                .shared
                .lock()
                .work
                .admitted_workers;
            let slot = usize::try_from(self.frontier % workers as u64).map_err(|_| broken())?;
            let mut state = shared.lock();
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            let cell = state.slots.data.get_mut(slot).ok_or_else(broken)?;
            if let Some((id, mut batch)) = cell.ready.take() {
                if id != self.frontier {
                    return Err(broken());
                }
                state.ready_count = state.ready_count.checked_sub(1).ok_or_else(broken)?;
                drop(state);
                self.reader.source.check_unchanged()?;
                let rows = batch
                    .len()
                    .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
                batch.truncate(rows)?;
                if rows as u64 == self.remaining {
                    self.status.complete = true;
                    self.shutdown()?;
                    if self.status.cancelled {
                        return Ok(false);
                    }
                    self.reader.source.check_unchanged()?;
                    self.status.returned_rows = self
                        .status
                        .returned_rows
                        .checked_add(rows as u64)
                        .ok_or_else(broken)?;
                    self.remaining = 0;
                    self.current = Some((slot, batch));
                    return Ok(true);
                }
                // Acknowledge under the same lock used to publish first errors.
                let state = shared.lock();
                if let Some(error) = &state.error {
                    return Err(error.clone());
                }
                if self.signal.stop.load(Ordering::Acquire) {
                    drop(state);
                    self.shutdown()?;
                    return Ok(false);
                }
                self.status.returned_rows = self
                    .status
                    .returned_rows
                    .checked_add(rows as u64)
                    .ok_or_else(broken)?;
                self.remaining -= rows as u64;
                self.current = Some((slot, batch));
                drop(state);
                return Ok(true);
            }
            if let Some(task) = cell.finished.take() {
                if task.id != self.frontier {
                    return Err(broken());
                }
                drop(state);
                drop(task);
                self.frontier = self.frontier.checked_add(1).ok_or_else(broken)?;
                if self.dispatched < self.total_tasks {
                    let prepared = self.prepared.as_mut().ok_or_else(broken)?;
                    let setup = Setup {
                        reader: &self.reader,
                        symbol: &self.symbol,
                        selection: self.selection.as_ref(),
                        scan: &self.scan,
                        options: self.options,
                        #[cfg(test)]
                        hooks: &shared.hooks,
                    };
                    let task = setup.task(
                        self.index.as_mut().ok_or_else(broken)?,
                        &mut prepared.next_interval,
                        self.dispatched,
                    )?;
                    let mut state = shared.lock();
                    if let Some(error) = &state.error {
                        return Err(error.clone());
                    }
                    if self.signal.stop.load(Ordering::Acquire) {
                        drop(state);
                        self.shutdown()?;
                        return Ok(false);
                    }
                    state.slots.data.get_mut(slot).ok_or_else(broken)?.task = Some(task);
                    state.work.tasks_dispatched = state
                        .work
                        .tasks_dispatched
                        .checked_add(1)
                        .ok_or_else(broken)?;
                    self.dispatched = self.dispatched.checked_add(1).ok_or_else(broken)?;
                    shared.signal.notify();
                }
            } else {
                #[cfg(test)]
                {
                    shared.hooks.coordinator_wait.store(true, Ordering::SeqCst);
                    shared.signal.notify();
                }
                drop(shared.wait(state));
            }
        }
    }
    /// Return the next ordered borrowed batch. The previous batch is recycled.
    pub fn next_batch(&mut self) -> Result<Option<BatchView<'_>>> {
        match self.advance() {
            Ok(true) => Ok(self.current.as_ref().map(|(_, batch)| batch.view())),
            Ok(false) => Ok(None),
            Err(error) => Err(self.fail(error)),
        }
    }
}

#[cfg(test)]
mod tests;
impl Drop for ParallelScanCursor {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}
impl Reader {
    /// Consume an explicitly validated index and stream batches in serial row order.
    /// Worker state, stacks and bounded batch slots share this Reader's root.
    /// Errors remain sticky; close joins workers and reports cleanup failures.
    ///
    /// ```no_run
    /// use gdxrs::{Reader, SymbolHandle, CheckpointIndex, ParallelOptions, Result};
    /// fn consume(reader: &Reader, symbol: &SymbolHandle,
    ///            index: CheckpointIndex) -> Result<CheckpointIndex> {
    ///     let mut cursor = reader.scan_parallel(symbol, index,
    ///         reader.scan_options(), ParallelOptions::default())?;
    ///     while let Some(batch) = cursor.next_batch()? {
    ///         println!("{} rows", batch.len());
    ///     }
    ///     cursor.into_index() // Reuse source proof without hashing/replaying it again.
    /// }
    /// ```
    pub fn scan_parallel(
        &self,
        symbol: &SymbolHandle,
        index: CheckpointIndex,
        scan: ScanOptions,
        options: ParallelOptions,
    ) -> Result<ParallelScanCursor> {
        ParallelScanCursor::new(self, symbol, None, index, scan, options)
    }
    /// Parallel scan with an exact source/symbol-bound selection and global
    /// accepted-row limit. Rejected rows still count as decoded work.
    pub fn scan_selected_parallel(
        &self,
        symbol: &SymbolHandle,
        selection: &Selection,
        index: CheckpointIndex,
        scan: ScanOptions,
        options: ParallelOptions,
    ) -> Result<ParallelScanCursor> {
        ParallelScanCursor::new(self, symbol, Some(selection), index, scan, options)
    }
}
