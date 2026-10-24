use crate::{Error, ErrorKind, Result};
use std::{
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetStatistics {
    pub current_bytes: usize,
    pub peak_bytes: usize,
    pub limit_bytes: usize,
}

pub(crate) struct Budget {
    used: AtomicUsize,
    peak: AtomicUsize,
    limit: usize,
}
pub(crate) struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            limit,
        })
    }
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation> {
        let mut old = self.used.load(Ordering::Relaxed);
        loop {
            let new = old
                .checked_add(bytes)
                .filter(|n| *n <= self.limit)
                .ok_or_else(|| {
                    Error::new(ErrorKind::BudgetTooSmall, "root memory reservation denied")
                })?;
            match self
                .used
                .compare_exchange_weak(old, new, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.peak.fetch_max(new, Ordering::Relaxed);
                    return Ok(Reservation {
                        budget: Arc::clone(self),
                        bytes,
                    });
                }
                Err(actual) => old = actual,
            }
        }
    }
    pub fn statistics(&self) -> BudgetStatistics {
        BudgetStatistics {
            current_bytes: self.used.load(Ordering::Acquire),
            peak_bytes: self.peak.load(Ordering::Relaxed),
            limit_bytes: self.limit,
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(crate) struct Buffer<T> {
    pub data: Vec<T>,
    _reservation: Reservation,
}
impl<T> Buffer<T> {
    pub fn new(budget: &Arc<Budget>, capacity: usize) -> Result<Self> {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .filter(|_| size_of::<T>() != 0)
            .ok_or_else(|| Error::new(ErrorKind::AllocationFailed, "buffer layout overflow"))?;
        let reservation = budget.reserve(bytes)?;
        #[cfg(test)]
        if FAIL_ALLOCATION.with(|flag| flag.replace(false)) {
            return Err(Error::new(
                ErrorKind::AllocationFailed,
                "injected allocation failure",
            ));
        }
        let mut data = Vec::new();
        data.try_reserve_exact(capacity)
            .map_err(|_| Error::new(ErrorKind::AllocationFailed, "buffer allocation failed"))?;
        if data.capacity() != capacity {
            return Err(Error::new(
                ErrorKind::AllocationFailed,
                "unexpected buffer capacity",
            ));
        }
        Ok(Self {
            data,
            _reservation: reservation,
        })
    }
    pub fn push(&mut self, value: T) -> Result<()> {
        if self.data.len() == self.data.capacity() {
            return Err(Error::new(
                ErrorKind::InvalidCount,
                "bounded buffer is full",
            ));
        }
        self.data.push(value);
        Ok(())
    }
}
impl Buffer<u8> {
    pub fn zeroed(budget: &Arc<Budget>, size: usize) -> Result<Self> {
        let mut buffer = Self::new(budget, size)?;
        buffer.data.resize(size, 0);
        Ok(buffer)
    }
}

#[cfg(test)]
thread_local! { static FAIL_ALLOCATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_failure_releases_reserved_layout() {
        let budget = Budget::new(100);
        FAIL_ALLOCATION.with(|flag| flag.set(true));
        assert_eq!(
            Buffer::<u32>::new(&budget, 10).err().unwrap().kind,
            ErrorKind::AllocationFailed
        );
        assert_eq!(budget.statistics().current_bytes, 0);
        assert_eq!(budget.statistics().peak_bytes, 40);
    }
    #[test]
    fn reservations_reject_overflow_and_release() {
        let b = Budget::new(100);
        let a = b.reserve(70).unwrap();
        assert_eq!(b.reserve(31).err().unwrap().kind, ErrorKind::BudgetTooSmall);
        assert_eq!(
            b.reserve(usize::MAX).err().unwrap().kind,
            ErrorKind::BudgetTooSmall
        );
        assert_eq!(b.statistics().current_bytes, 70);
        drop(a);
        assert_eq!(b.statistics().current_bytes, 0);
        assert_eq!(b.statistics().peak_bytes, 70);
    }
    #[test]
    fn buffer_capacity_and_failed_layout_are_charged() {
        let b = Budget::new(100);
        let mut data = Buffer::<u32>::new(&b, 10).unwrap();
        assert_eq!(b.statistics().current_bytes, 40);
        for i in 0..10 {
            data.push(i).unwrap();
        }
        assert!(data.push(10).is_err());
        drop(data);
        assert_eq!(b.statistics().current_bytes, 0);
        assert!(Buffer::<u32>::new(&b, usize::MAX).is_err());
        assert_eq!(b.statistics().current_bytes, 0);
    }
}
