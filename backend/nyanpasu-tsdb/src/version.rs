//! The version registry (§7.1): a read-write lock over the current version and at most one
//! pending version. Every hold of the lock is short, and none spans an fsync.
//!
//! - The writer registers the staged version as pending under the write lock, commits with
//!   no registry lock held, then publishes or withdraws it under the write lock (`write::commit`).
//! - A reader captures a snapshot under the read lock: it opens the storage snapshot,
//!   reads its publication Q, reserves the bytes of the version with Q from the query
//!   budget, and only then pins that version. While it holds the read lock the writer can
//!   neither publish nor drop a version, and Q is the current version's or the pending
//!   one's, so the version is always there. A snapshot therefore never pairs new chunks with
//!   an old head, or the reverse, and no reader holds a version the budget does not count.
//!   Only damaged metadata can show another publication, which is an error, not a panic.
//! - The one exception is a commit whose outcome is unknown: storage may show P+1 although
//!   the writer withdraws it. Withdrawing it sets the recovery flag in the same critical
//!   section, and a capture checks the flag under the read lock before it looks for its
//!   version, so it fails with `RecoveryNeeded` instead of finding none (§7.5).
//!
//! A version that no snapshot holds any more is freed when the registry lets go of it.

#[cfg(test)]
use std::sync::Mutex;
use std::{
    fmt,
    sync::{
        Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::{
    ErrorKind, Schema,
    budget::{BudgetExceeded, MemoryBudget, Reservation, Reserved},
    catalog::Catalog,
    head::Head,
    layout::meta::{MetaError, read_state},
    storage::{CommitFailure, Storage, StorageError},
};

/// The catalog and head of one publication.
pub(crate) struct Version {
    pub(crate) publication: u64,
    pub(crate) catalog: Catalog,
    pub(crate) head: Head,
}

impl Version {
    /// The memory a snapshot of this version can keep alive: at most every byte that exists
    /// when it is captured (§7.4). Even an empty catalog and head include their fixed graphs,
    /// so this is nonzero: zero query reservations also establish that retirement has drained.
    fn bytes(&self) -> u64 {
        self.catalog.bytes().saturating_add(self.head.bytes())
    }
}

/// A constructed publication and the resident growth and temporary copies that cover it.
/// Its payload drops before either charge if registration fails or its caller unwinds.
pub(crate) struct StagedVersion {
    /// The completed shared graph, dropped before either construction charge.
    pub(crate) version: Version,
    /// Resident head growth beyond the current head, consumed only on publication.
    pub(crate) growth: Reservation,
    /// Temporary head and catalog copies, kept until publication or withdrawal.
    pub(crate) staging: Reservation,
}

/// Admission state, changed under the publication lock so withdrawal and failure are atomic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    /// The owner and readers may start operations.
    Open,
    /// An unknown commit or backend failure requires owner-driven recovery.
    RecoveryNeeded,
    /// The owner detached the old instance and is reopening or restoring it.
    Recovering,
    /// Old snapshots or charged results must drop before the owner may reopen.
    WaitingForReaders,
    /// The last recovery attempt failed; the host owns its error and decides when to retry.
    Unavailable,
    /// The owner attempted a checkpoint and retired the backend; recovery may reopen it.
    Suspended,
    /// The owner retired the backend; readers retain only their captured state.
    Closed,
}

impl LifecycleState {
    /// Recovery states ask the host to recover; Suspended and Closed use the Local refusal.
    fn needs_recovery(self) -> bool {
        !matches!(self, Self::Open | Self::Suspended | Self::Closed)
    }
}

struct Slots {
    current: Option<Arc<Version>>,
    pending: Option<Arc<Version>>,
    /// Resident policy credit belongs to the registry, after the version fields in drop order.
    head: Option<Reservation>,
    /// Set after a commit whose outcome is unknown, or any error that says the backend
    /// failed, by the owner or by a reader. Every later write and capture fails until the
    /// owner recovers (§7.5). It lives under the lock with the versions, so that withdrawing
    /// a version and flagging the store are one step for a capture.
    state: LifecycleState,
    /// Ephemeral owner backlog, observed under R alongside capture.
    behind: bool,
    /// A new open replaces this marker together with its publication.
    open: Arc<OpenMarker>,
}

/// The instance shared by its owner and readers, including the only backend owner.
///
/// Lock order: G (storage gate) -> R (registry slots) -> backend locks -> budget mutexes.
/// Backend locks are MemoryDisk's disk and RedbSnapshot's tables (plus redb test hooks).
/// They release before callbacks; budgets never call back into storage, G or R. Registry
/// test-hook mutexes follow R; query-hook mutexes are used only at query boundaries in tests.
/// All hook mutexes release before callbacks. Production query checks use only the atomic.
/// Captures hold G-read and R-read through reserve-before-pin. Writes and maintenance hold
/// G-read once and pass the backend down; only short register/publish/withdraw steps take R.
/// No R guard spans commit, rollback, close or fsync. Retirement takes G-write then R-write,
/// detaches ownership, and drops the backend and roots after both guards are released.
/// Recovery reopens and restores without G or R, with admission closed. Installation takes
/// G-write then R-write only after query credit drained and all fallible work completed.
/// Suspend/close first close admission under G-write then R-write, checkpoint under G-read
/// without R, then retire. Neither the checkpoint commit nor backend Drop holds R.
/// Status captures the ephemeral backlog under R with its publication, then reads metadata
/// without G or R. Its retained result holds no capture or version.
/// Compaction holds G-write, checks admission briefly under R, then invokes the backend
/// without R. It leaves the revocation flag and publication unchanged; captures wait on G.
pub(crate) struct Shared<S> {
    /// Capture gate; a reader never retains this guard after capture.
    storage: RwLock<Option<S>>,
    /// Publication and admission state.
    pub(crate) registry: Registry,
    /// Shared by capture reservations, query workspaces and retained results.
    pub(crate) query_budget: MemoryBudget,
    /// Set on retirement; reset only after the old query budget has drained in recovery.
    revoked: AtomicBool,
    /// True when staging gates off at least one windowed event stream.
    pub(crate) event_consolidation_gated_off: bool,
    /// One ordered query boundary hook; never present in production.
    #[cfg(test)]
    query_hook: Mutex<Option<QueryHook>>,
}

impl<S: Storage> Shared<S> {
    /// Installs the initial backend and registry in one capture gate.
    pub(crate) fn new(
        storage: S,
        registry: Registry,
        query_budget: MemoryBudget,
        event_consolidation_gated_off: bool,
    ) -> Self {
        #[cfg(test)]
        query_budget.schedule(storage.fault_schedule());
        Self {
            storage: RwLock::new(Some(storage)),
            registry,
            query_budget,
            event_consolidation_gated_off,
            revoked: AtomicBool::new(false),
            #[cfg(test)]
            query_hook: Mutex::new(None),
        }
    }

    /// Borrows the backend for one owner operation, without excluding captures.
    pub(crate) fn storage(&self) -> RwLockReadGuard<'_, Option<S>> {
        self.storage.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Borrows the backend exclusively for physical compaction, excluding new captures.
    pub(crate) fn exclusive_storage(&self) -> RwLockWriteGuard<'_, Option<S>> {
        self.storage.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Closes admissions and detaches the backend. The caller drops it outside both locks.
    pub(crate) fn retire(&self, state: LifecycleState) -> Retired<S> {
        let mut storage = self.storage.write().unwrap_or_else(PoisonError::into_inner);
        let mut slots = self.registry.write();
        slots.state = state;
        self.revoked.store(true, Ordering::Release);
        Retired {
            _storage: storage.take(),
            _current: slots.current.take(),
            _pending: slots.pending.take(),
            _head: slots.head.take(),
        }
    }

    /// Closes admission and revokes before a lifecycle checkpoint. Returns false when already
    /// suspended or closed; close may make Suspended terminal, but Closed never changes.
    pub(crate) fn stop(&self, state: LifecycleState) -> bool {
        let _storage = self.storage.write().unwrap_or_else(PoisonError::into_inner);
        let mut slots = self.registry.write();
        let previous = slots.state;
        if previous != LifecycleState::Closed {
            slots.state = state;
        }
        self.revoked.store(true, Ordering::Release);
        !matches!(previous, LifecycleState::Suspended | LifecycleState::Closed)
    }

    /// Installs a fully restored candidate after retirement drained all old query credit.
    /// The owner has already reset its private bookkeeping; captures see this entire swap.
    pub(crate) fn install(&self, backend: S, version: Version, head: Reservation) {
        let version = Arc::new(version);
        let open = Arc::new(OpenMarker);
        let mut storage = self.storage.write().unwrap_or_else(PoisonError::into_inner);
        let mut slots = self.registry.write();
        debug_assert!(storage.is_none() && slots.current.is_none() && slots.pending.is_none());
        debug_assert!(slots.head.is_none());
        *storage = Some(backend);
        slots.current = Some(version);
        slots.head = Some(head);
        slots.open = open;
        self.revoked.store(false, Ordering::Release);
        slots.state = LifecycleState::Open;
        slots.behind = false;
    }

    /// Captures under G then R; a failure is reported only after R's read guard unwinds.
    pub(crate) fn capture(self: &Arc<Self>) -> Result<Snapshot<S>, CaptureError> {
        let storage = self.storage();
        let captured = self.pin(storage.as_ref());
        if let Err(error) = &captured {
            self.report(error.kind());
        }
        captured
    }

    /// Reports a backend failure without changing a state already retired by the owner.
    pub(crate) fn report(&self, kind: ErrorKind) {
        if kind == ErrorKind::BackendFailed {
            self.registry.require_recovery();
        }
    }

    /// The coarse query cancellation check; it takes no publication or backend lock.
    pub(crate) fn check_open(&self) -> Result<(), StorageError> {
        check_open(&self.revoked)
    }

    /// The flag borrowed by the query's scan and decode contexts.
    pub(crate) fn revoked(&self) -> QueryCheck<'_> {
        QueryCheck {
            flag: &self.revoked,
            #[cfg(test)]
            hook: Some(&self.query_hook),
        }
    }

    /// Arms a specific query boundary after `skip` visits, releasing the hook lock first.
    #[cfg(test)]
    pub(crate) fn at_query(
        &self,
        point: &'static str,
        skip: usize,
        run: impl FnOnce() + Send + 'static,
    ) {
        *self.query_hook.lock().unwrap() = Some(QueryHook {
            point,
            skip,
            run: Box::new(run),
        });
    }

    /// Nonblocking capture-gate probe at a barrier ordered test boundary.
    #[cfg(test)]
    pub(crate) fn gate_available(&self) -> bool {
        self.storage.try_write().is_ok()
    }

    /// Whether a capture could enter G-read at an explicitly ordered test boundary.
    #[cfg(test)]
    pub(crate) fn capture_gate_available(&self) -> bool {
        self.storage.try_read().is_ok()
    }

    /// Reserves a nonzero capture claim before pinning the matching publication under R.
    fn pin(self: &Arc<Self>, storage: Option<&S>) -> Result<Snapshot<S>, CaptureError> {
        let slots = self.registry.read();
        ensure!(!slots.state.needs_recovery(), RecoveryNeededSnafu);
        self.check_open().context(CheckOpenSnafu)?;
        let storage = storage.expect("an open instance owns its backend");
        let snapshot = storage.snapshot().context(OpenSnapshotSnafu)?;
        #[cfg(test)]
        Registry::run(&self.registry.after_snapshot);
        let publication = read_state(&snapshot)
            .context(ReadPublicationSnafu)?
            .publication;
        let current = slots
            .current
            .as_ref()
            .expect("an open instance has a version");
        let version = [Some(current), slots.pending.as_ref()]
            .into_iter()
            .flatten()
            .find(|version| version.publication == publication)
            .context(UnknownPublicationSnafu {
                found: publication,
                current: current.publication,
                pending: slots.pending.as_ref().map(|version| version.publication),
            })?;
        let bytes = version.bytes();
        assert!(
            bytes > 0,
            "every version includes its fixed catalog and head graphs"
        );
        let reservation = self
            .query_budget
            .try_reserve("snapshot", bytes)
            .context(ReserveQuerySnafu)?;
        let snapshot = Snapshot {
            storage: snapshot,
            version: reservation.hold(|| version.clone()),
            open: slots.open.clone(),
            shared: self.clone(),
            status_behind: slots.behind,
        };
        #[cfg(test)]
        Registry::run(&self.registry.after_pin);
        Ok(snapshot)
    }
}

/// Detached owners in drop order: backend, graphs, then resident head credit.
/// Returned only after the capture and publication guards have been released.
pub(crate) struct Retired<S> {
    /// Kept separately so test crash paths can take the backend without a clean close.
    pub(crate) _storage: Option<S>,
    /// Detached published root, dropped after backend close.
    _current: Option<Arc<Version>>,
    /// Detached pending root; absent during ordinary owner retirement.
    _pending: Option<Arc<Version>>,
    /// Resident credit returned only after both registry roots.
    _head: Option<Reservation>,
}

/// Checks the instance flag at a query block, stripe, piece or row-batch boundary.
pub(crate) fn check_open(revoked: &AtomicBool) -> Result<(), StorageError> {
    if revoked.load(Ordering::Acquire) {
        Err(StorageError::Closed)
    } else {
        Ok(())
    }
}

/// A borrowed flag, with a per-instance boundary hook only in tests. It owns no epoch or lease.
#[derive(Clone, Copy)]
pub(crate) struct QueryCheck<'a> {
    /// The single flag owned by Shared.
    flag: &'a AtomicBool,
    /// Test synchronization only; released before invoking a boundary callback.
    #[cfg(test)]
    hook: Option<&'a Mutex<Option<QueryHook>>>,
}

impl QueryCheck<'_> {
    /// An unarmed check for tests constructing private decoders directly.
    #[cfg(test)]
    pub(crate) fn unarmed(flag: &AtomicBool) -> QueryCheck<'_> {
        QueryCheck { flag, hook: None }
    }

    /// Checks after the ordered test boundary, using one Acquire load in production.
    pub(crate) fn check(self, _point: &'static str) -> Result<(), StorageError> {
        #[cfg(test)]
        if let Some(hook) = self.hook {
            let run = {
                let mut hook = hook.lock().unwrap();
                if let Some(armed) = hook
                    .as_mut()
                    .filter(|armed| armed.point == _point || armed.point == "*")
                {
                    if armed.skip == 0 {
                        hook.take().map(|armed| armed.run)
                    } else {
                        armed.skip -= 1;
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(run) = run {
                run();
            }
        }
        check_open(self.flag)
    }
}

/// One boundary to pause, after an explicit number of visits, without clocks or global state.
#[cfg(test)]
struct QueryHook {
    /// The checkpoint named by its function and work boundary.
    point: &'static str,
    /// Visits to pass before running the hook.
    skip: usize,
    /// Removed from the mutex before execution.
    run: Box<dyn FnOnce() + Send>,
}

/// The short publication gate, nested inside Shared's capture gate when both are needed.
pub(crate) struct Registry {
    /// Internal transition boundaries that make no storage calls.
    #[cfg(test)]
    faults: crate::verification::faults::FaultSchedule,
    slots: RwLock<Slots>,
    /// The dataset's schema, which every snapshot carries for its queries.
    schema: Arc<Schema>,
    /// Runs once inside the next capture, after it opened the storage snapshot.
    #[cfg(test)]
    after_snapshot: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Runs once inside the next capture, once it holds its version.
    #[cfg(test)]
    after_pin: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Registry {
    pub(crate) fn new(
        current: Version,
        schema: Arc<Schema>,
        open: Arc<OpenMarker>,
        head: Reservation,
    ) -> Self {
        Self {
            #[cfg(test)]
            faults: head.fault_schedule(),
            slots: RwLock::new(Slots {
                current: Some(Arc::new(current)),
                pending: None,
                head: Some(head),
                state: LifecycleState::Open,
                behind: false,
                open,
            }),
            schema,
            #[cfg(test)]
            after_snapshot: Mutex::default(),
            #[cfg(test)]
            after_pin: Mutex::default(),
        }
    }

    pub(crate) fn current(&self) -> Arc<Version> {
        self.read()
            .current
            .as_ref()
            .expect("the owner checked admission")
            .clone()
    }

    /// Registers `next` as pending with its resident growth and temporary-copy charges.
    /// The guard keeps both until publication or withdrawal removes the overlap.
    ///
    /// # Panics
    ///
    /// If a version is pending already: the owner commits one batch at a time.
    pub(crate) fn register(&self, next: StagedVersion) -> Pending<'_> {
        #[cfg(test)]
        self.faults.boundary("pending.register");
        let mut slots = self.write();
        assert!(
            slots.pending.is_none(),
            "a version registered while another is pending: the owner commits one batch at a time"
        );
        let StagedVersion {
            version,
            growth,
            staging,
        } = next;
        slots.pending = Some(Arc::new(version));
        Pending {
            registry: self,
            growth: Some(growth),
            staging: Some(staging),
        }
    }

    pub(crate) fn needs_recovery(&self) -> bool {
        self.read().state.needs_recovery()
    }

    /// Admission state for owner calls, which are serialized by the facade's mutable borrow.
    pub(crate) fn state(&self) -> LifecycleState {
        self.read().state
    }

    /// The owner's single ephemeral backlog flag, read under R.
    pub(crate) fn behind(&self) -> bool {
        self.read().behind
    }

    /// Updates the owner backlog after a completed operation, under R.
    pub(crate) fn set_behind(&self, behind: bool) {
        self.write().behind = behind;
    }

    /// Records a recovery call that returned without installing a candidate.
    pub(crate) fn recovery_stopped(&self, state: LifecycleState) {
        self.write().state = state;
    }

    pub(crate) fn require_recovery(&self) {
        let mut slots = self.write();
        if slots.state == LifecycleState::Open {
            slots.state = LifecycleState::RecoveryNeeded;
        }
    }

    /// A poisoned lock still guards consistent slots: every critical section is a few
    /// assignments that cannot panic part-way.
    fn read(&self) -> RwLockReadGuard<'_, Slots> {
        self.slots.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Slots> {
        self.slots.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
impl Registry {
    /// Whether retirement detached every registry owner, including resident head credit.
    pub(crate) fn is_empty(&self) -> bool {
        let slots = self.read();
        slots.current.is_none() && slots.pending.is_none() && slots.head.is_none()
    }

    /// A nonblocking witness that no publication guard is held at an ordered test boundary.
    pub(crate) fn publication_available(&self) -> bool {
        self.slots.try_write().is_ok()
    }

    /// Resident credit stays with the current registry slot, independently of readers' claims.
    pub(crate) fn head_reserved(&self) -> u64 {
        self.read().head.as_ref().map_or(0, Reservation::bytes)
    }

    /// Makes the next capture run `hook` while it holds the read lock, right after it
    /// opened the storage snapshot.
    pub(crate) fn after_snapshot(&self, hook: impl FnOnce() + Send + 'static) {
        *self.after_snapshot.lock().unwrap() = Some(Box::new(hook));
    }

    /// Makes the next capture run `hook` while it holds the read lock, right after it
    /// pinned its version.
    pub(crate) fn after_pin(&self, hook: impl FnOnce() + Send + 'static) {
        *self.after_pin.lock().unwrap() = Some(Box::new(hook));
    }

    fn run(hook: &Mutex<Option<Box<dyn FnOnce() + Send>>>) {
        let hook = hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    pub(crate) fn pending(&self) -> Option<Arc<Version>> {
        self.read().pending.clone()
    }
}

/// A registered version that is not published yet. Dropping it withdraws the version and
/// returns its reservation; after a failed commit, [`fail`](Self::fail) does.
pub(crate) struct Pending<'r> {
    registry: &'r Registry,
    /// `None` once published or failed.
    growth: Option<Reservation>,
    /// Returned only after the pending version is published or removed from the registry.
    staging: Option<Reservation>,
}

impl Pending<'_> {
    /// Makes the pending version current and settles resident credit inside the same owner.
    pub(crate) fn publish(mut self) {
        #[cfg(test)]
        self.registry.faults.boundary("pending.publish");
        let growth = self.growth.take().expect("a version is published once");
        let mut slots = self.registry.write();
        let version = slots
            .pending
            .take()
            .expect("the pending version stays registered until published or withdrawn");
        let bytes = version.head.bytes();
        // The old version is dropped here unless a snapshot holds it; a snapshot reserved
        // its bytes from the query budget when it was captured.
        slots.current = Some(version);
        let head = slots
            .head
            .as_mut()
            .expect("the owner has not retired the registry");
        head.merge(growth);
        let retired = head.bytes() - bytes;
        head.shrink(retired);
        drop(slots);
        drop(self.staging.take());
        #[cfg(test)]
        self.registry.faults.boundary("pending.published");
    }

    /// Withdraws the version after its commit failed with `failure`, and returns its
    /// reservation. When the batch may have applied, or the backend failed, it flags the
    /// store for recovery in the same critical section: storage may already show this
    /// version's publication, and a capture must not look for it once it is gone.
    pub(crate) fn fail(mut self, failure: &CommitFailure) {
        let growth = self.growth.take();
        let mut slots = self.registry.write();
        slots.pending = None;
        if matches!(failure, CommitFailure::Unknown { .. })
            || failure.kind() == ErrorKind::BackendFailed
        {
            slots.state = LifecycleState::RecoveryNeeded;
        }
        drop(slots);
        drop(growth);
        drop(self.staging.take());
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if self.growth.is_some() {
            self.registry.write().pending = None;
        }
    }
}

/// A consistent view of one publication: the storage snapshot, and the catalog and head of
/// the version it shows. It holds a reservation of the version's bytes from the query
/// budget, returned when it is dropped; its shared instance supplies query working memory.
/// Fields drop in order: storage transaction, version payload then capture credit, open marker,
/// shared instance. A zero query budget therefore cannot precede the last transaction's drop.
pub struct Snapshot<S: Storage> {
    storage: S::Snapshot,
    version: Reserved<Arc<Version>>,
    open: Arc<OpenMarker>,
    /// Drops after the storage transaction and the charged version, never before them.
    shared: Arc<Shared<S>>,
    /// Backlog observed together with this capture, rather than a later publication.
    pub(crate) status_behind: bool,
}

/// What the ids of a snapshot's catalog mean (§5.3 rule 5): the label and combination ids it
/// resolves, those of the results read from it, and the filters built from them. Equal tokens
/// mean the same open store and the same generation of its catalog: two snapshots with equal
/// tokens give every id they both hold the same value, and the later one may hold ids the
/// earlier did not. A release changes the generation, since a later write may give the id to
/// another value. Every open starts at generation 0, but its tokens equal no other open's, of
/// the same store or another.
///
/// A query result keeps the token of the snapshot it was read from
/// ([`SeriesGroups::catalog`](crate::SeriesGroups::catalog),
/// [`EventPage::catalog`](crate::EventPage::catalog)). Its ids mean what they say only through
/// a snapshot whose [`catalog`](Snapshot::catalog) equals it.
#[derive(Clone)]
pub struct CatalogToken {
    /// The marker of the open the snapshot was captured in, compared by address. The token
    /// keeps it alive, so no other open's marker can take its address while the token exists.
    open: Arc<OpenMarker>,
    /// The catalog's generation within the open (`Catalog::generation`).
    generation: u64,
}

impl PartialEq for CatalogToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.open, &other.open) && self.generation == other.generation
    }
}

impl Eq for CatalogToken {}

impl fmt::Debug for CatalogToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogToken")
            .field("open", &Arc::as_ptr(&self.open))
            .field("generation", &self.generation)
            .finish()
    }
}

/// What tells one open apart from every other, of the same store or another: each open makes
/// one, and a [`CatalogToken`] compares it by address. An `Arc` allocates its counts even for
/// a unit type, so every live marker has an address of its own.
pub(crate) struct OpenMarker;

impl<S: Storage> Snapshot<S> {
    /// The token of this snapshot's catalog, which the results read from it keep.
    pub fn catalog(&self) -> CatalogToken {
        CatalogToken {
            open: self.open.clone(),
            generation: self.version.get().catalog.generation(),
        }
    }

    /// Borrows the instance's single revocation flag for a query's coarse checks.
    pub(crate) fn revoked(&self) -> QueryCheck<'_> {
        self.shared.revoked()
    }

    /// Normalizes a failed query after retirement without entering another scan checkpoint.
    pub(crate) fn check_open(&self) -> Result<(), StorageError> {
        self.shared.check_open()
    }

    /// Propagates a query's backend failure after its storage callbacks have unwound.
    pub(crate) fn report(&self, kind: ErrorKind) {
        self.shared.report(kind);
    }

    pub(crate) fn storage(&self) -> &S::Snapshot {
        &self.storage
    }

    pub(crate) fn version(&self) -> &Version {
        self.version.get()
    }

    pub(crate) fn schema(&self) -> &Schema {
        &self.shared.registry.schema
    }

    /// Current lifecycle admission; status returns no data outside Open.
    pub(crate) fn lifecycle(&self) -> LifecycleState {
        self.shared.registry.state()
    }

    /// Immutable profile gate shared across this facade's recoveries.
    pub(crate) fn event_consolidation_gated_off(&self) -> bool {
        self.shared.event_consolidation_gated_off
    }

    pub(crate) fn budget(&self) -> &MemoryBudget {
        &self.shared.query_budget
    }
}

#[cfg(test)]
impl<S: Storage> Snapshot<S> {
    pub(crate) fn reserved(&self) -> u64 {
        self.version.charge_bytes()
    }
}

/// Capturing a snapshot failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum CaptureError {
    /// The owner retired this instance before a new snapshot could be admitted.
    #[snafu(display("the store is closed to captures"))]
    CheckOpen { source: StorageError },

    /// The store needs recovery, is recovering, is waiting for old readers, or is unavailable.
    /// Every capture fails until the owner successfully recovers (§7.5).
    #[snafu(display("the store needs recovery before it can be read"))]
    RecoveryNeeded,

    #[snafu(display("failed to open a storage snapshot"))]
    OpenSnapshot { source: StorageError },

    #[snafu(display("failed to read the snapshot's publication"))]
    ReadPublication { source: MetaError },

    /// The state record decodes, but names a publication that is neither the current
    /// version's nor the pending one's. Only the owner commits, only after registering, and
    /// it flags an unknown outcome as it withdraws, so the record is damaged.
    #[snafu(display(
        "storage shows publication {found}, but the current version is {current} and the \
         pending one {pending:?}"
    ))]
    UnknownPublication {
        found: u64,
        current: u64,
        pending: Option<u64>,
    },

    /// The query budget has no room for the version the snapshot would keep alive.
    #[snafu(display("no room in the query budget for the snapshot"))]
    ReserveQuery { source: BudgetExceeded },
}

impl CaptureError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::RecoveryNeeded => ErrorKind::BackendFailed,
            Self::CheckOpen { source } => source.kind(),
            Self::OpenSnapshot { source } => source.kind(),
            Self::ReadPublication { source } => source.kind(),
            Self::UnknownPublication { .. } => ErrorKind::Corrupted,
            Self::ReserveQuery { source } => source.kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::{
        db::tests::schema,
        layout::meta::{StateRecord, write_state},
        storage::{
            Durability, StorageWrite,
            memory::{MemoryDisk, MemoryStorage},
        },
    };

    fn version(publication: u64) -> Version {
        let schema = schema();
        Version {
            publication,
            catalog: Catalog::new(&schema),
            head: Head::empty(schema.events.len()),
        }
    }

    /// A registry's resident claim is independent of each test's pending-growth budget.
    fn registry(current: Version) -> Registry {
        let head = MemoryBudget::new(u64::MAX)
            .try_reserve("head", current.head.bytes())
            .unwrap();
        Registry::new(current, Arc::new(schema()), Arc::new(OpenMarker), head)
    }

    /// Commits a state record that shows `publication`, as the owner's commit of that
    /// version does.
    fn show(storage: &MemoryStorage, publication: u64) {
        let mut batch = storage.batch().unwrap();
        let state = StateRecord {
            publication,
            ..StateRecord::INITIAL
        };
        write_state(&mut batch, &state).unwrap();
        batch.commit(Durability::Durable).unwrap();
    }

    fn failed() -> StorageError {
        StorageError::Failed {
            source: "disk gone".into(),
        }
    }

    /// Pending payloads and both charges unwind together, without changing the current slot.
    #[test]
    fn unwinding_with_a_pending_version_withdraws_it_and_returns_both_charges() {
        let registry = registry(version(0));
        let heads = MemoryBudget::new(10);
        let staging = MemoryBudget::new(20);
        let mut withdrawn = None;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pending = registry.register(StagedVersion {
                version: version(1),
                growth: heads.try_reserve("head", 4).unwrap(),
                staging: staging.try_reserve("staging", 12).unwrap(),
            });
            withdrawn = Some(Arc::downgrade(&registry.pending().unwrap()));
            assert_eq!(heads.reserved(), 4);
            assert_eq!(staging.reserved(), 12);
            panic!("the owner unwound before storage committed");
        }));
        assert!(panic.is_err());
        assert!(registry.pending().is_none());
        assert!(withdrawn.unwrap().upgrade().is_none());
        assert_eq!(registry.current().publication, 0);
        assert!(!registry.needs_recovery());
        assert_eq!(heads.reserved(), 0);
        assert_eq!(staging.reserved(), 0);
    }

    /// A captured pending graph keeps its query charge after a failed publication withdraws it.
    #[test]
    fn withdrawing_a_captured_pending_version_leaves_its_snapshot_as_the_owner() {
        let storage = MemoryDisk::new().open().unwrap();
        show(&storage, 0);
        let registry = registry(version(0));
        let heads = MemoryBudget::new(10);
        let staging = MemoryBudget::new(20);
        let queries = MemoryBudget::new(u64::MAX);
        let shared = Arc::new(Shared::new(storage, registry, queries.clone(), false));
        let registry = &shared.registry;
        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: staging.try_reserve("staging", 12).unwrap(),
        });
        let withdrawn = Arc::downgrade(&registry.pending().unwrap());
        show(shared.storage().as_ref().unwrap(), 1);
        let snapshot = shared.capture().unwrap();
        let bytes = snapshot.version().bytes();
        pending.fail(&CommitFailure::Unknown { source: failed() });
        assert!(registry.pending().is_none());
        assert_eq!(heads.reserved(), 0);
        assert_eq!(staging.reserved(), 0);
        assert!(withdrawn.upgrade().is_some());
        assert_eq!(queries.reserved(), bytes);
        assert_eq!(snapshot.version().publication, 1);
        drop(snapshot);
        assert!(withdrawn.upgrade().is_none());
        assert_eq!(queries.reserved(), 0);
    }

    // A commit whose outcome is unknown may have applied, so storage shows P+1 while the
    // writer withdraws P+1. A capture after that reports the recovery instead of looking
    // for a version that is gone.
    #[test]
    fn a_version_withdrawn_after_an_unknown_outcome_fails_captures_instead_of_panicking() {
        let storage = MemoryDisk::new().open().unwrap();
        show(&storage, 0);
        let registry = registry(version(0));
        let queries = MemoryBudget::new(u64::MAX);
        let shared = Arc::new(Shared::new(storage, registry, queries.clone(), false));
        let registry = &shared.registry;
        let heads = MemoryBudget::new(10);

        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: MemoryBudget::new(u64::MAX)
                .try_reserve("staging", 0)
                .unwrap(),
        });
        show(shared.storage().as_ref().unwrap(), 1);
        // Until it is withdrawn, the pending version is the one storage shows.
        let snapshot = shared.capture().unwrap();
        assert_eq!(snapshot.version().publication, 1);

        pending.fail(&CommitFailure::Unknown { source: failed() });
        assert!(registry.needs_recovery());
        assert!(registry.pending().is_none());
        assert_eq!(heads.reserved(), 0);
        let error = shared.capture().map(drop).unwrap_err();
        assert!(matches!(error, CaptureError::RecoveryNeeded));
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        // The snapshot captured before stays as it was.
        assert_eq!(snapshot.version().publication, 1);
    }

    // A reader paused while it holds its version keeps it alive past any later publication,
    // so the version's bytes are reserved by then, not after (§7.4).
    #[test]
    fn a_reader_holding_its_version_has_reserved_its_bytes() {
        let storage = MemoryDisk::new().open().unwrap();
        show(&storage, 0);
        let registry = registry(version(0));
        let bytes = registry.current().bytes();
        assert_ne!(bytes, 0);
        let queries = MemoryBudget::new(u64::MAX);
        let shared = Arc::new(Shared::new(storage, registry, queries.clone(), false));
        let registry = &shared.registry;
        let (reserved_tx, reserved_rx) = mpsc::channel();
        let budget = queries.clone();
        registry.after_pin(move || reserved_tx.send(budget.reserved()).unwrap());

        let snapshot = shared.capture().unwrap();
        assert_eq!(reserved_rx.try_recv(), Ok(bytes));
        assert_eq!(snapshot.reserved(), bytes);
        drop(snapshot);
        assert_eq!(queries.reserved(), 0);
    }

    // A state record that decodes but names a publication the registry does not hold is
    // damage: the capture reports it, reserves nothing, and does not ask for recovery.
    #[test]
    fn a_publication_the_registry_does_not_hold_is_corrupted() {
        let storage = MemoryDisk::new().open().unwrap();
        show(&storage, 99);
        let registry = registry(version(1));
        let queries = MemoryBudget::new(u64::MAX);
        let shared = Arc::new(Shared::new(storage, registry, queries.clone(), false));
        let registry = &shared.registry;
        let heads = MemoryBudget::new(10);

        let error = shared.capture().map(drop).unwrap_err();
        assert!(
            matches!(
                error,
                CaptureError::UnknownPublication {
                    found: 99,
                    current: 1,
                    pending: None
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);

        let _pending = registry.register(StagedVersion {
            version: version(2),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: MemoryBudget::new(u64::MAX)
                .try_reserve("staging", 0)
                .unwrap(),
        });
        let error = shared.capture().map(drop).unwrap_err();
        assert!(
            matches!(
                error,
                CaptureError::UnknownPublication {
                    found: 99,
                    current: 1,
                    pending: Some(2)
                }
            ),
            "{error:?}"
        );
        assert!(!registry.needs_recovery());
        assert_eq!(queries.reserved(), 0);
    }

    #[test]
    fn a_version_withdrawn_after_a_rollback_leaves_captures_working() {
        let storage = MemoryDisk::new().open().unwrap();
        show(&storage, 0);
        let registry = registry(version(0));
        let queries = MemoryBudget::new(u64::MAX);
        let shared = Arc::new(Shared::new(storage, registry, queries.clone(), false));
        let registry = &shared.registry;
        let heads = MemoryBudget::new(10);

        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: MemoryBudget::new(u64::MAX)
                .try_reserve("staging", 0)
                .unwrap(),
        });
        let rolled_back = StorageError::RolledBack {
            source: "poisoned".into(),
        };
        pending.fail(&CommitFailure::NotApplied {
            source: rolled_back,
        });
        assert!(!registry.needs_recovery());
        assert!(registry.pending().is_none());
        assert_eq!(heads.reserved(), 0);
        let snapshot = shared.capture().unwrap();
        assert_eq!(snapshot.version().publication, 0);
        drop(snapshot);

        // A commit refused because the backend had already failed applied nothing, but the
        // backend has to be reopened all the same.
        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: MemoryBudget::new(u64::MAX)
                .try_reserve("staging", 0)
                .unwrap(),
        });
        pending.fail(&CommitFailure::NotApplied { source: failed() });
        assert!(registry.needs_recovery());
        assert!(matches!(
            shared.capture().map(drop),
            Err(CaptureError::RecoveryNeeded)
        ));
    }
}
