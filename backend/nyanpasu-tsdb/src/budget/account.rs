//! An operation envelope, its scoped leases, and ownership transfer to retained results.

use std::cell::{Cell, RefCell};

use super::{BudgetExceeded, BudgetVec, MemoryBudget, MemoryPlan, Reservation};

/// An account-local callback for independently observing allocation lifetimes in tests.
#[cfg(test)]
pub(super) type Observer = Box<dyn Fn(Observation)>;

/// One query's or one round's allocation envelope, backed by one root reservation.
///
/// Leases borrow this account, so it cannot drop while their storage remains owned. Retired
/// leases return local credit; reusing it avoids the root budget's lock until more is needed.
/// Live charges never exceed the reservation, and replacement peaks include both allocations.
pub(crate) struct MemoryAccount {
    /// The envelope, including reusable credit from retired buffers.
    held: RefCell<Reservation>,
    /// Bytes charged to live leases, excluding credit transferred to retained results.
    pub(super) live: Cell<u64>,
    /// The greatest simultaneous live charge, recorded before any retirement.
    peak: Cell<u64>,
    /// Cumulative observed allocation capacity, saturating if the counter fills.
    pub(super) allocated: Cell<u64>,
    /// The remaining successful admissions before injected failure.
    #[cfg(test)]
    fail_after: Cell<Option<usize>>,
    /// Receives allocation, retirement and transfer checkpoints for this account.
    #[cfg(test)]
    pub(super) observer: RefCell<Option<Observer>>,
}

impl std::fmt::Debug for MemoryAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryAccount")
            .field("held", &self.held)
            .field("live", &self.live)
            .field("peak", &self.peak)
            .field("allocated", &self.allocated)
            .finish()
    }
}

impl MemoryAccount {
    /// Reserves the initial plan before constructing the operation envelope.
    pub(crate) fn new(
        budget: &MemoryBudget,
        what: &'static str,
        plan: MemoryPlan,
    ) -> Result<Self, BudgetExceeded> {
        Ok(Self {
            held: RefCell::new(budget.try_reserve(what, plan.0)?),
            live: Cell::new(0),
            peak: Cell::new(0),
            allocated: Cell::new(0),
            #[cfg(test)]
            fail_after: Cell::new(None),
            #[cfg(test)]
            observer: RefCell::new(None),
        })
    }

    /// Admits and allocates a buffer whose lease borrows this envelope.
    pub(crate) fn buffer<T>(&self, capacity: usize) -> Result<BudgetVec<'_, T>, BudgetExceeded> {
        BudgetVec::new(self, capacity)
    }

    /// Consumes charged buffers into one immutable result and moves their reservation credit.
    /// The result owns no account borrow, and no transferred bytes are released or re-reserved.
    pub(crate) fn finish<T, B: Retain<T>>(
        &self,
        builder: B,
    ) -> Result<Reserved<super::FrozenVec<T>>, BudgetExceeded> {
        let mut retained = Retained {
            account: self,
            held: self.held.borrow_mut().split(0),
        };
        let payload = builder.retain(&mut retained)?;
        Ok(Reserved {
            payload,
            held: retained.held,
        })
    }

    /// Charges a scoped allocation before storage is created, reusing available local credit.
    /// Only a request beyond the envelope's credit grows the root reservation under its lock.
    pub(super) fn lease(&self, plan: MemoryPlan) -> Result<Lease<'_>, BudgetExceeded> {
        let mut held = self.held.borrow_mut();
        #[cfg(test)]
        if let Some(remaining) = self.fail_after.get() {
            if remaining == 0 {
                return Err(BudgetExceeded {
                    what: held.what,
                    needed: plan.0,
                    available: 0,
                });
            }
            self.fail_after.set(Some(remaining - 1));
        }
        let live = self.live.get().checked_add(plan.0).ok_or(BudgetExceeded {
            what: held.what,
            needed: plan.0,
            available: held.bytes() - self.live.get(),
        })?;
        let extra = live.saturating_sub(held.bytes());
        if extra != 0 {
            held.try_grow(extra)?;
        }
        self.live.set(live);
        self.peak.set(self.peak.get().max(live));
        debug_assert!(self.live.get() <= held.bytes());
        drop(held);
        Ok(Lease {
            account: self,
            bytes: plan.0,
            actual: 0,
        })
    }

    /// Injects admission failure after the given number of successful lease requests.
    #[cfg(test)]
    pub(crate) fn fail_after(&self, allocations: usize) {
        self.fail_after.set(Some(allocations));
    }

    /// Reports a storage-lifetime checkpoint to the account-local test observer.
    fn observe(&self, kind: Change, bytes: u64) {
        #[cfg(test)]
        if let Some(observer) = &*self.observer.borrow() {
            observer(Observation {
                kind,
                bytes,
                live: self.live.get(),
                peak: self.peak.get(),
            });
        }
        #[cfg(not(test))]
        let _ = (kind, bytes);
    }
}

/// A scoped charge that borrows its envelope and returns local credit when dropped.
/// Its storage owner must drop storage before dropping this lease.
#[derive(Debug)]
pub(super) struct Lease<'a> {
    /// The envelope kept alive by the storage owner's borrow.
    pub(super) account: &'a MemoryAccount,
    /// Reserved bytes covering the allocation capacity.
    pub(super) bytes: u64,
    /// Observed allocation capacity for retirement and transfer checkpoints.
    actual: u64,
}

/// The kind of storage-lifetime checkpoint observed by an account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    /// New storage has been allocated under its charge.
    Allocate,
    /// Storage has dropped and its local charge has returned.
    Retire,
    /// Live storage and its credit have moved to a retained result.
    Transfer,
}

/// An observed storage layout and the envelope counters at that lifetime boundary.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Observation {
    /// The allocation-lifetime transition.
    pub(super) kind: Change,
    /// Actual storage capacity involved in the transition.
    pub(super) bytes: u64,
    /// The envelope's live charge at this checkpoint.
    pub(super) live: u64,
    /// The greatest live charge seen before this checkpoint.
    pub(super) peak: u64,
}

impl Lease<'_> {
    /// Records actual capacity after allocation, asserting that the lease covers it.
    pub(super) fn allocated(&mut self, bytes: u64) {
        debug_assert!(bytes <= self.bytes);
        self.actual = bytes;
        self.account
            .allocated
            .set(self.account.allocated.get().saturating_add(bytes));
        self.account.observe(Change::Allocate, bytes);
    }

    /// Moves this lease's credit into the same account's retained claim without releasing it.
    /// Storage remains live under that claim instead of returning reusable local credit.
    pub(super) fn retain(mut self, retained: &mut Retained<'_>) {
        assert!(std::ptr::eq(self.account, retained.account));
        retained
            .held
            .merge(self.account.held.borrow_mut().split(self.bytes));
        self.account.live.set(self.account.live.get() - self.bytes);
        self.account.observe(Change::Transfer, self.actual);
        self.bytes = 0;
        self.actual = 0;
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.account.live.set(self.account.live.get() - self.bytes);
        debug_assert!(self.account.live.get() <= self.account.held.borrow().bytes());
        self.account.observe(Change::Retire, self.actual);
    }
}

/// A consuming builder that returns only frozen storage constructed under allocation charges.
pub(crate) trait Retain<T> {
    /// Moves storage and its credit into `retained`, keeping partial payloads covered on failure.
    fn retain(self, retained: &mut Retained<'_>) -> Result<super::FrozenVec<T>, BudgetExceeded>;
}

/// The aggregate claim assembled while a builder freezes its buffers.
pub(crate) struct Retained<'a> {
    /// The account whose leases may transfer credit into this claim.
    account: &'a MemoryAccount,
    /// Transferred credit, held throughout construction of the frozen payload.
    held: Reservation,
}

/// A retained payload and its aggregate reservation, independent of the construction account.
#[derive(Debug)]
pub(crate) struct Reserved<T> {
    /// Must stay before `held`: fields drop in declaration order, so storage drops before its
    /// reservation is returned. Do not reorder these fields.
    payload: T,
    /// The payload's charge, returned only after its storage drops.
    held: Reservation,
}

impl<T> Reserved<T> {
    /// Borrows the payload without exposing mutation or separating it from its charge.
    pub(crate) fn get(&self) -> &T {
        debug_assert!(self.held.bytes() <= self.held.budget.reserved());
        &self.payload
    }
}
