//! An operation envelope, its scoped leases, and ownership transfer to retained results.

use std::{
    cell::{Cell, RefCell},
    ops::Deref,
    rc::Rc,
};

use super::{BudgetExceeded, BudgetVec, MemoryBudget, MemoryPlan, Reservation};

/// An account-local callback for independently observing allocation lifetimes in tests.
#[cfg(test)]
pub(super) type Observer = Box<dyn Fn(Observation)>;

/// A concrete child-layout observer used only during graph construction in tests.
#[cfg(test)]
type ConstructionObserver = Box<dyn Fn(u64)>;

/// One query's or one round's allocation envelope, backed by one root reservation.
///
/// Leases keep a local handle, so the envelope lives until its storage drops. Retired
/// leases return local credit; reusing it avoids the root budget's lock until more is needed.
/// Live charges never exceed the reservation, and replacement peaks include both allocations.
#[derive(Clone)]
pub(crate) struct MemoryAccount {
    /// Fixed local control metadata shared without atomic operations.
    state: Rc<AccountState>,
}

/// Shared local bookkeeping for one envelope; buffer handles keep it alive without atomics.
struct AccountState {
    /// The envelope, including reusable credit from retired buffers.
    held: RefCell<Reservation>,
    /// Bytes charged to live leases, excluding credit transferred to retained results.
    live: Cell<u64>,
    /// Standing credit dedicated to one fragment lent at a time.
    lent: Cell<u64>,
    /// Whether a fragment callback currently uses the standing allowance.
    lending: Cell<bool>,
    /// The greatest simultaneous live charge, recorded before any retirement.
    peak: Cell<u64>,
    /// Cumulative observed allocation capacity, saturating if the counter fills.
    allocated: Cell<u64>,
    /// The remaining successful admissions before injected failure.
    #[cfg(test)]
    fail_after: Cell<Option<usize>>,
    /// Existing root reservations excluded from an exclusive operation's test peak.
    #[cfg(test)]
    retention_base: Cell<Option<u64>>,
    /// Receives allocation, retirement and transfer checkpoints for this account.
    #[cfg(test)]
    observer: RefCell<Option<Observer>>,
    /// Independent concrete-layout checkpoints inside an opaque graph constructor.
    #[cfg(test)]
    construction: RefCell<Option<ConstructionObserver>>,
}

impl std::fmt::Debug for MemoryAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryAccount")
            .field("held", &self.state.held)
            .field("live", &self.state.live)
            .field("peak", &self.state.peak)
            .field("allocated", &self.state.allocated)
            .finish()
    }
}

impl MemoryAccount {
    /// Whether two local handles share the same operation envelope.
    pub(super) fn same_envelope(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    /// Reserves the initial plan before constructing the operation envelope.
    pub(crate) fn new(
        budget: &MemoryBudget,
        what: &'static str,
        plan: MemoryPlan,
    ) -> Result<Self, BudgetExceeded> {
        Ok(Self::from_reservation(budget.try_reserve(what, plan.0)?))
    }

    /// Reserves a complete round once; execution cannot grow beyond its admitted credit.
    /// Retired buffers return local credit for later phases of the same round.
    pub(crate) fn planned(
        budget: &MemoryBudget,
        what: &'static str,
        plan: MemoryPlan,
    ) -> Result<Self, BudgetExceeded> {
        let round = MemoryBudget::within(budget.try_reserve(what, plan.0)?);
        Self::new(&round, what, plan)
    }

    /// Takes ownership of credit already admitted by an operation, without debiting it again.
    pub(crate) fn from_reservation(held: Reservation) -> Self {
        Self {
            state: Rc::new(AccountState {
                held: RefCell::new(held),
                live: Cell::new(0),
                lent: Cell::new(0),
                lending: Cell::new(false),
                peak: Cell::new(0),
                allocated: Cell::new(0),
                #[cfg(test)]
                fail_after: Cell::new(None),
                #[cfg(test)]
                observer: RefCell::new(None),
                #[cfg(test)]
                retention_base: Cell::new(None),
                #[cfg(test)]
                construction: RefCell::new(None),
            }),
        }
    }

    /// Admits capacity before allocating storage; a zero-capacity buffer allocates nothing.
    pub(crate) fn buffer<T>(&self, capacity: usize) -> Result<BudgetVec<T>, BudgetExceeded> {
        BudgetVec::allocate(self, capacity)
    }

    /// Admits an opaque allocation before calling its constructor and transfers its claim.
    /// Use for exact immutable graphs and dependency contexts; growable buffers use `BudgetVec`.
    pub(crate) fn value<T>(
        &self,
        what: &'static str,
        plan: MemoryPlan,
        create: impl FnOnce() -> T,
    ) -> Result<Reserved<T>, BudgetExceeded> {
        let mut lease = self.lease(plan).map_err(|mut error| {
            error.what = what;
            error
        })?;
        let payload = create();
        lease.allocated(plan.0);
        let mut retained = Retained {
            account: self,
            held: self.state.held.borrow_mut().split(0),
        };
        lease.retain(&mut retained);
        Ok(Reserved {
            payload,
            held: retained.held,
        })
    }

    /// Consumes charged buffers into one immutable result and moves their reservation credit.
    /// The result owns no account borrow, and no transferred bytes are released or re-reserved.
    /// Finalizers preserve their domain errors while partial payloads drop under their charge.
    pub(crate) fn finish<T, E, B: Retain<T, E>>(
        &self,
        builder: B,
    ) -> Result<Reserved<super::FrozenVec<T>>, E> {
        let mut retained = Retained {
            account: self,
            held: self.state.held.borrow_mut().split(0),
        };
        let payload = builder.retain(&mut retained)?;
        Ok(Reserved {
            payload,
            held: retained.held,
        })
    }

    /// Charges a scoped allocation before storage is created, reusing available local credit.
    /// Only a request beyond the envelope's credit grows the root reservation under its lock.
    pub(crate) fn lease(&self, plan: MemoryPlan) -> Result<Lease, BudgetExceeded> {
        let mut held = self.state.held.borrow_mut();
        #[cfg(test)]
        if let Some(remaining) = self.state.fail_after.get().filter(|_| plan.0 != 0) {
            if remaining == 0 {
                return Err(BudgetExceeded {
                    what: held.what,
                    needed: plan.0,
                    available: 0,
                });
            }
            self.state.fail_after.set(Some(remaining - 1));
        }
        let live = self
            .state
            .live
            .get()
            .checked_add(plan.0)
            .ok_or(BudgetExceeded {
                what: held.what,
                needed: plan.0,
                available: held.bytes() - self.state.live.get(),
            })?;
        let extra = live.saturating_sub(held.bytes());
        if extra != 0 {
            held.try_grow(extra)?;
        }
        self.state.live.set(live);
        self.state.peak.set(self.state.peak.get().max(live));
        debug_assert!(self.state.live.get() <= held.bytes());
        drop(held);
        #[cfg(test)]
        self.sample_round_peak();
        Ok(Lease {
            account: self.clone(),
            bytes: plan.0,
            actual: 0,
        })
    }

    /// Makes reusable credit available before a foreign allocation or lent fragment is accessed.
    pub(crate) fn admit(&self, bytes: u64) -> Result<(), BudgetExceeded> {
        let mut held = self.state.held.borrow_mut();
        let needed = self
            .state
            .live
            .get()
            .checked_add(bytes)
            .ok_or(BudgetExceeded {
                what: held.what,
                needed: u64::MAX,
                available: 0,
            })?;
        let extra = needed.saturating_sub(held.bytes());
        if extra != 0 {
            held.try_grow(extra)?;
        }
        Ok(())
    }

    /// Room for a new plan beside live storage, including this envelope's reusable credit.
    pub(crate) fn available(&self) -> u64 {
        let held = self.state.held.borrow();
        held.budget.shared.limit - held.budget.reserved() + held.bytes() - self.state.live.get()
    }

    /// Keeps the round's existing fragment allowance beside all outputs and directory storage.
    /// Loan callbacks reuse this credit instead of charging the same allowance twice.
    pub(crate) fn reserve_lent(&self, bytes: u64) -> Result<(), BudgetExceeded> {
        let extra = bytes.saturating_sub(self.state.lent.get());
        self.admit(extra)?;
        self.state.live.set(self.state.live.get() + extra);
        self.state.lent.set(self.state.lent.get().max(bytes));
        self.state
            .peak
            .set(self.state.peak.get().max(self.state.live.get()));
        #[cfg(test)]
        self.sample_round_peak();
        Ok(())
    }

    /// Covers a fragment before invoking its source; only excess over standing credit is leased.
    pub(crate) fn lend<T>(
        &self,
        bytes: u64,
        read: impl FnOnce() -> T,
    ) -> Result<T, BudgetExceeded> {
        let extra = self.lease(MemoryPlan::bytes(
            bytes.saturating_sub(self.state.lent.get()),
        ))?;
        assert!(
            !self.state.lending.replace(true),
            "fragment loans cannot nest"
        );
        debug_assert!(bytes <= self.state.lent.get() + extra.bytes);
        let guard = Lending {
            account: self,
            extra,
            bytes,
        };
        self.state
            .allocated
            .set(self.state.allocated.get().saturating_add(bytes));
        self.observe(Change::Allocate, bytes);
        let result = read();
        drop(guard);
        Ok(result)
    }

    /// The greatest live charge observed during this account's actual execution.
    #[cfg(test)]
    pub(crate) fn peak_bytes(&self) -> u64 {
        self.sample_round_peak();
        self.state.peak.get()
    }

    /// The independently observed sum of live lease charges, excluding retained dependency owners.
    #[cfg(test)]
    pub(crate) fn live_bytes(&self) -> u64 {
        self.state.live.get()
    }

    /// Standing fragment allowance, for capacity-versus-charge fixture checks.
    #[cfg(test)]
    pub(crate) fn lent_bytes(&self) -> u64 {
        self.state.lent.get()
    }

    /// Installs a detailed lifetime probe used by the budget invariants.
    #[cfg(test)]
    pub(super) fn set_observer(&self, observer: Observer) {
        *self.state.observer.borrow_mut() = Some(observer);
    }

    /// Total capacity observed across allocations, for independent replacement checks.
    #[cfg(test)]
    pub(super) fn allocated_bytes(&self) -> u64 {
        self.state.allocated.get()
    }

    /// Registers an account-local allocation-lifetime probe without global allocator hooks.
    #[cfg(test)]
    pub(crate) fn observe_allocations(&self, observe: impl Fn(bool, u64, u64) + 'static) {
        *self.state.observer.borrow_mut() = Some(Box::new(move |event| match event.kind {
            Change::Allocate => observe(true, event.bytes, event.live),
            Change::Retire | Change::Transfer => observe(false, event.bytes, event.live),
        }));
    }

    /// Injects admission failure after the given number of successful lease requests.
    #[cfg(test)]
    pub(crate) fn fail_after(&self, allocations: usize) {
        self.state.fail_after.set(Some(allocations));
    }

    /// Installs a concrete-layout probe independent of opaque graph allowance estimates.
    #[cfg(test)]
    pub(crate) fn observe_construction(&self, observe: impl Fn(u64) + 'static) {
        *self.state.construction.borrow_mut() = Some(Box::new(observe));
    }

    /// Reports an actual child layout while its enclosing graph is still being constructed.
    #[cfg(test)]
    pub(crate) fn constructed(&self, bytes: u64) {
        if let Some(observe) = &*self.state.construction.borrow() {
            observe(bytes);
        }
    }

    /// Live round leases plus reservations retained outside the account's reusable credit.
    /// Cached idle credit is excluded, so reserving a plan does not itself count as a peak.
    #[cfg(test)]
    pub(crate) fn round_live_bytes(&self) -> u64 {
        let held = self.state.held.borrow();
        let retained = if held.budget.shared._parent.is_some() {
            held.budget.reserved() - held.bytes()
        } else if let Some(base) = self.state.retention_base.get() {
            held.budget.reserved() - held.bytes() - base
        } else {
            0
        };
        self.state.live.get() + retained
    }

    /// Includes retained children when this operation exclusively uses a root budget.
    /// Existing reservations are excluded; no other operation may reserve until it finishes.
    #[cfg(test)]
    pub(crate) fn observe_root_retention(&self) {
        let held = self.state.held.borrow();
        self.state
            .retention_base
            .set(Some(held.budget.reserved() - held.bytes()));
    }

    /// Samples actual round retention at admission and allocation checkpoints, only in tests.
    #[cfg(test)]
    fn sample_round_peak(&self) {
        self.state
            .peak
            .set(self.state.peak.get().max(self.round_live_bytes()));
    }

    /// Reports a storage-lifetime checkpoint to the account-local test observer.
    fn observe(&self, kind: Change, bytes: u64) {
        #[cfg(test)]
        self.sample_round_peak();
        #[cfg(test)]
        if let Some(observer) = &*self.state.observer.borrow() {
            observer(Observation {
                kind,
                bytes,
                live: self.state.live.get(),
                peak: self.state.peak.get(),
            });
        }
        #[cfg(not(test))]
        let _ = (kind, bytes);
    }
}

/// A scoped charge that keeps its envelope alive and returns local credit when dropped.
/// Its storage owner must drop storage before dropping this lease.
#[derive(Debug)]
pub(crate) struct Lease {
    /// The envelope kept alive by the storage owner's local handle.
    pub(super) account: MemoryAccount,
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

impl Lease {
    /// The envelope that owns this lease, for related transient allocations of the same draft.
    pub(crate) fn account(&self) -> &MemoryAccount {
        &self.account
    }

    /// Records actual capacity after allocation, asserting that the lease covers it.
    pub(super) fn allocated(&mut self, bytes: u64) {
        debug_assert!(bytes <= self.bytes);
        self.actual = bytes;
        self.account
            .state
            .allocated
            .set(self.account.state.allocated.get().saturating_add(bytes));
        self.account.observe(Change::Allocate, bytes);
    }

    /// Extends a transient graph's high-water allowance before its next allocation.
    /// The graph's policy counts replacements and retirement; this lease keeps the greatest
    /// simultaneous footprint until publication, without installing claims on shared pages.
    pub(crate) fn try_cover(&mut self, plan: MemoryPlan) -> Result<(), BudgetExceeded> {
        let extra = plan.0.saturating_sub(self.bytes);
        if extra != 0 {
            let mut added = self.account.lease(MemoryPlan::bytes(extra))?;
            added.allocated(extra);
            self.bytes += added.bytes;
            self.actual += added.actual;
            added.bytes = 0;
            added.actual = 0;
        }
        Ok(())
    }

    /// Seals a replay source under its construction allowance, before obsolete workspace retires.
    /// The source finalizer measures its retained graph once, after all record merges finish.
    pub(crate) fn seal<T>(self, payload: T) -> Reserved<T> {
        let account = self.account.clone();
        let mut retained = Retained::new(&account);
        self.retain(&mut retained);
        retained.finish(payload)
    }

    /// Hands a completed draft's high-water charge to its caller until publication or withdrawal.
    /// This transfers credit without returning it to the root budget between the two owners.
    pub(crate) fn into_reservation(self) -> Reservation {
        let account = self.account.clone();
        let mut retained = Retained::new(&account);
        self.retain(&mut retained);
        retained.held
    }

    /// Moves this lease's credit into the same account's retained claim without releasing it.
    /// Storage remains live under that claim instead of returning reusable local credit.
    pub(crate) fn retain(mut self, retained: &mut Retained<'_>) {
        assert!(Rc::ptr_eq(&self.account.state, &retained.account.state));
        retained
            .held
            .merge(self.account.state.held.borrow_mut().split(self.bytes));
        self.account
            .state
            .live
            .set(self.account.state.live.get() - self.bytes);
        self.account.observe(Change::Transfer, self.actual);
        self.bytes = 0;
        self.actual = 0;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.account
            .state
            .live
            .set(self.account.state.live.get() - self.bytes);
        debug_assert!(self.account.state.live.get() <= self.account.state.held.borrow().bytes());
        self.account.observe(Change::Retire, self.actual);
    }
}

/// A consuming builder that returns only frozen storage constructed under allocation charges.
/// Its error type lets graph construction retain the caller's existing read and budget errors.
pub(crate) trait Retain<T, E = BudgetExceeded> {
    /// Moves storage and its credit into `retained`, keeping partial payloads covered on failure.
    fn retain(self, retained: &mut Retained<'_>) -> Result<super::FrozenVec<T>, E>;
}

/// The aggregate claim assembled while a builder freezes its buffers.
pub(crate) struct Retained<'a> {
    /// The account whose leases may transfer credit into this claim.
    pub(super) account: &'a MemoryAccount,
    /// Transferred credit, held throughout construction of the frozen payload.
    held: Reservation,
}

/// A retained payload and its aggregate reservation, independent of the construction account.
#[derive(Debug)]
pub(crate) struct Reserved<T> {
    /// Must stay before `held`: fields drop in declaration order, so storage drops before its
    /// reservation is returned. Do not reorder these fields.
    pub(super) payload: T,
    /// The payload's charge, returned only after its storage drops.
    held: Reservation,
}

impl Reservation {
    /// Pins a retained view only after its capture reservation has been admitted.
    pub(crate) fn hold<T>(self, create: impl FnOnce() -> T) -> Reserved<T> {
        Reserved {
            payload: create(),
            held: self,
        }
    }
}

impl<T> Reserved<T> {
    /// The concrete retained charge, for capture and handover assertions in tests.
    #[cfg(test)]
    pub(crate) fn charge_bytes(&self) -> u64 {
        self.held.bytes()
    }

    /// Hands a transient graph and its complete charge to the publication or resident owner.
    /// The receiving owner must drop its graph before returning this credit; published shared
    /// graphs themselves carry no claim, since their registry and snapshots own retention.
    pub(crate) fn into_parts(self) -> (T, Reservation) {
        (self.payload, self.held)
    }

    /// Returns obsolete dependency workspace after its concrete owner measures retained storage.
    pub(crate) fn retain_bytes(mut self, bytes: u64) -> Self {
        assert!(
            bytes <= self.held.bytes(),
            "retained capacity exceeds its allowance"
        );
        self.held.shrink(self.held.bytes() - bytes);
        self
    }

    /// Borrows the payload without exposing mutation or separating it from its charge.
    pub(crate) fn get(&self) -> &T {
        debug_assert!(self.held.bytes() <= self.held.budget.reserved());
        &self.payload
    }
}

impl<T, E> Reserved<Result<T, E>> {
    /// Moves a successful dependency with its charge; failed construction returns unused credit.
    pub(crate) fn transpose(self) -> Result<Reserved<T>, E> {
        let Self { payload, held } = self;
        payload.map(|payload| Reserved { payload, held })
    }
}

impl Reserved<zstd::bulk::Compressor<'static>> {
    /// Measures the dependency's current workspace without exposing its mutable context.
    #[cfg(test)]
    pub(crate) fn workspace_size(&mut self) -> usize {
        self.payload.context_mut().sizeof()
    }
}

impl<'a> Retained<'a> {
    /// Starts a transfer claim without releasing any envelope credit.
    pub(crate) fn new(account: &'a MemoryAccount) -> Self {
        Self {
            account,
            held: account.state.held.borrow_mut().split(0),
        }
    }

    /// Seals the graph assembled from charged children and construction allowances.
    /// The finalizer keeps those children inside the payload until its aggregate charge drops.
    pub(crate) fn finish<T>(self, payload: T) -> Reserved<T> {
        Reserved::from_retained(payload, self)
    }

    /// The account whose construction probe observes this graph.
    #[cfg(test)]
    pub(crate) fn account(&self) -> &'a MemoryAccount {
        self.account
    }

    /// Consumes an already charged child into the graph being sealed, without a byte count.
    /// The finalizer must keep the child inside its payload, which drops before this claim.
    /// A zero-byte child carries no credit to transfer between budget roots.
    pub(crate) fn absorb<T>(&mut self, child: Reserved<T>) -> T {
        let Reserved { payload, held } = child;
        if held.bytes() != 0 {
            self.held.merge(held);
        }
        payload
    }
}

impl<T> Reserved<T> {
    /// Seals already-charged storage with the transfer claim assembled by its budget factory.
    pub(super) fn from_retained(payload: T, retained: Retained<'_>) -> Self {
        Self {
            payload,
            held: retained.held,
        }
    }
}

/// Retires the opaque loan allowance after the backend callback and borrowed guard have ended.
struct Lending<'a> {
    /// The standing fragment allowance remains live until this scope retires its loan.
    account: &'a MemoryAccount,
    /// Extra credit beyond standing room, held through the retirement checkpoint.
    extra: Lease,
    /// The opaque loan allowance, bounded before its source can lend storage.
    bytes: u64,
}

impl Drop for Lending<'_> {
    fn drop(&mut self) {
        debug_assert!(self.bytes <= self.account.state.lent.get() + self.extra.bytes);
        self.account.observe(Change::Retire, self.bytes);
        self.account.state.lending.set(false);
    }
}

impl<T> Deref for Reserved<T> {
    type Target = T;
    /// Borrows retained data without separating it from the charge or exposing mutation.
    fn deref(&self) -> &T {
        self.get()
    }
}
