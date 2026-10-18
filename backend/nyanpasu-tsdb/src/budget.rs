//! Memory budgets (§7.4). One total budget per store; the head, the catalog, the backend
//! cache, staging and queries all reserve from it. A reservation is returned when dropped.
//! A reservation that does not fit fails with `BudgetExceeded` instead of growing past the
//! budget.
//!
//! The profile gives one limit per memory field (§10.1), and each field gets its own
//! [`MemoryBudget`]. Queries share one through the client, which is the admission control
//! of §7.4: a query that does not fit is refused at the door, and no scheduler sits
//! behind it.
//!
//! # Charged buffers
//!
//! Use [`MemoryAccount`] for one query's or one maintenance round's allocation envelope.
//! [`MemoryPlan`] checks its layout arithmetic before admission. [`BudgetVec`] borrows
//! credit from the account before allocating; dropped buffers return local credit, which
//! leases reuse without taking the root budget's lock until more bytes are needed.
//! Preserving growth covers the old and new allocations together. Bounded writes never
//! grow a buffer. A consuming [`Retain`] builder moves frozen storage and its credit into
//! one [`Reserved`] result, so the result may outlive the account without releasing and
//! reserving its bytes again.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use snafu::{Snafu, ensure};

use crate::ErrorKind;

mod account;
mod buffer;

pub(crate) use account::{MemoryAccount, Reserved, Retain, Retained};
pub(crate) use buffer::{BudgetVec, FrozenVec};

/// Checked, allocation-free byte arithmetic for allocations that coexist in an envelope.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MemoryPlan(u64);

impl MemoryPlan {
    /// Plans an array capacity, rejecting multiplication or layout overflow before allocation.
    pub(crate) fn array<T>(capacity: usize) -> Result<Self, BudgetExceeded> {
        let layout = std::alloc::Layout::array::<T>(capacity).map_err(|_| BudgetExceeded {
            what: "buffer layout",
            needed: u64::MAX,
            available: 0,
        })?;
        Ok(Self(layout.size() as u64))
    }

    /// Adds a simultaneously live layout, rejecting byte-count overflow.
    pub(crate) fn and(self, other: Self) -> Result<Self, BudgetExceeded> {
        self.0.checked_add(other.0).map(Self).ok_or(BudgetExceeded {
            what: "memory plan",
            needed: u64::MAX,
            available: 0,
        })
    }
}

/// A byte budget, cloneable and shared: every clone reserves from the same counter.
#[derive(Clone, Debug)]
pub struct MemoryBudget {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    limit: u64,
    /// Bytes held by live reservations, never above `limit`. Every change happens under
    /// this lock, so a reserve that fails changes nothing and a waiter cannot miss the
    /// moment the count reaches zero.
    reserved: Mutex<u64>,
    /// Signalled when `reserved` becomes zero.
    released: Condvar,
}

impl MemoryBudget {
    pub fn new(limit: u64) -> Self {
        Self {
            shared: Arc::new(Shared {
                limit,
                reserved: Mutex::new(0),
                released: Condvar::new(),
            }),
        }
    }

    /// Reserves `bytes` for `what`, which names the owner in the error. The bytes return
    /// to the budget when the reservation is dropped.
    pub fn try_reserve(
        &self,
        what: &'static str,
        bytes: u64,
    ) -> Result<Reservation, BudgetExceeded> {
        self.take(what, bytes)?;
        Ok(Reservation {
            budget: self.clone(),
            what,
            bytes,
        })
    }

    /// The bytes held by live reservations.
    pub fn reserved(&self) -> u64 {
        *self.lock()
    }

    /// Blocks until no bytes are reserved. Recovery waits on it after revoking the
    /// snapshots, so that the old catalog and head are gone before the new ones are built
    /// (§7.4, §7.5).
    ///
    /// There is no timeout: the callers bound the wait themselves, because a query
    /// notices its revocation at the next column block or row batch and then drops its
    /// reservation.
    pub fn wait_until_released(&self) {
        let reserved = self.lock();
        drop(
            self.shared
                .released
                .wait_while(reserved, |reserved| *reserved != 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }

    fn take(&self, what: &'static str, bytes: u64) -> Result<(), BudgetExceeded> {
        let mut reserved = self.lock();
        let available = self.shared.limit - *reserved;
        ensure!(
            bytes <= available,
            BudgetExceededSnafu {
                what,
                needed: bytes,
                available
            }
        );
        *reserved += bytes;
        Ok(())
    }

    fn give_back(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let mut reserved = self.lock();
        *reserved -= bytes;
        if *reserved == 0 {
            self.shared.released.notify_all();
        }
    }

    /// A poisoned lock still holds a consistent count: nothing inside a critical section
    /// can panic part-way through an update.
    fn lock(&self) -> MutexGuard<'_, u64> {
        self.shared
            .reserved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Bytes held in a [`MemoryBudget`], returned when this is dropped.
///
/// Growing, shrinking, merging and splitting change the budget's count by exactly the
/// bytes involved, so a handover between owners never passes through zero and never wakes
/// a [`wait_until_released`](MemoryBudget::wait_until_released) early.
#[derive(Debug)]
pub struct Reservation {
    budget: MemoryBudget,
    what: &'static str,
    bytes: u64,
}

impl Reservation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Reserves `bytes` more. If they do not fit, the reservation and the budget stay as
    /// they were.
    pub fn try_grow(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        self.budget.take(self.what, bytes)?;
        self.bytes += bytes;
        Ok(())
    }

    /// Returns `bytes` to the budget.
    ///
    /// # Panics
    ///
    /// If `bytes` is more than the reservation holds: that is an accounting error in the
    /// caller.
    pub fn shrink(&mut self, bytes: u64) {
        assert!(
            bytes <= self.bytes,
            "shrinking {} by {bytes} bytes, which is more than it holds ({})",
            self.what,
            self.bytes
        );
        self.bytes -= bytes;
        self.budget.give_back(bytes);
    }

    /// Takes over the bytes of `other`, which must come from the same budget.
    ///
    /// # Panics
    ///
    /// If `other` belongs to a different budget.
    pub fn merge(&mut self, mut other: Reservation) {
        assert!(
            Arc::ptr_eq(&self.budget.shared, &other.budget.shared),
            "merging reservations of different budgets ({} and {})",
            self.what,
            other.what
        );
        self.bytes += std::mem::take(&mut other.bytes);
    }

    /// Splits `bytes` off into a new reservation of the same budget, to hand to another
    /// owner.
    ///
    /// # Panics
    ///
    /// If `bytes` is more than the reservation holds.
    pub fn split(&mut self, bytes: u64) -> Reservation {
        assert!(
            bytes <= self.bytes,
            "splitting {bytes} bytes off {}, which holds only {}",
            self.what,
            self.bytes
        );
        self.bytes -= bytes;
        Reservation {
            budget: self.budget.clone(),
            what: self.what,
            bytes,
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.give_back(self.bytes);
    }
}

/// A reservation that does not fit in what the budget has left. Other modules wrap it in
/// the step that asked, such as admitting a query or staging a batch.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(display(
    "memory budget exceeded for {what}: {needed} bytes needed, {available} available"
))]
#[snafu(visibility(pub(crate)))]
pub struct BudgetExceeded {
    pub what: &'static str,
    pub needed: u64,
    pub available: u64,
}

impl BudgetExceeded {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

#[cfg(test)]
mod tests;
