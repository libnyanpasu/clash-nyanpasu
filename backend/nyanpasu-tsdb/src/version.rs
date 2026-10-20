//! The version registry (§7.1): a read-write lock over the current version and at most one
//! pending version. Every hold of the lock is short, and none spans an fsync.
//!
//! - The writer registers the staged version as pending under the write lock, commits with
//!   no lock held, then publishes or withdraws it under the write lock (`write::commit`).
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
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
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
    /// when it is captured (§7.4).
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

struct Slots {
    current: Arc<Version>,
    pending: Option<Arc<Version>>,
    /// Resident policy credit belongs to the registry, after the version fields in drop order.
    head: Reservation,
    /// Set after a commit whose outcome is unknown, or any error that says the backend
    /// failed, by the owner or by a reader. Every later write and capture fails until the
    /// owner recovers (§7.5). It lives under the lock with the versions, so that withdrawing
    /// a version and flagging the store are one step for a capture.
    recovery: bool,
}

pub(crate) struct Registry {
    slots: RwLock<Slots>,
    /// The dataset's schema, which every snapshot carries for its queries.
    schema: Arc<Schema>,
    /// This open's marker, which every snapshot's [`CatalogToken`] carries.
    open: Arc<OpenMarker>,
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
            slots: RwLock::new(Slots {
                current: Arc::new(current),
                pending: None,
                head,
                recovery: false,
            }),
            schema,
            open,
            #[cfg(test)]
            after_snapshot: Mutex::default(),
            #[cfg(test)]
            after_pin: Mutex::default(),
        }
    }

    pub(crate) fn current(&self) -> Arc<Version> {
        self.read().current.clone()
    }

    /// Registers `next` as pending with its resident growth and temporary-copy charges.
    /// The guard keeps both until publication or withdrawal removes the overlap.
    ///
    /// # Panics
    ///
    /// If a version is pending already: the owner commits one batch at a time.
    pub(crate) fn register(&self, next: StagedVersion) -> Pending<'_> {
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
        self.read().recovery
    }

    pub(crate) fn require_recovery(&self) {
        self.write().recovery = true;
    }

    /// Captures a snapshot of `storage`, reserving the bytes of its version from `budget`
    /// (§7.4). A failure of the backend flags the store for recovery (§7.5).
    pub(crate) fn capture<S: Storage>(
        &self,
        storage: &S,
        budget: &MemoryBudget,
    ) -> Result<Snapshot<S>, CaptureError> {
        let captured = self.pin(storage, budget);
        if let Err(error) = &captured
            && error.kind() == ErrorKind::BackendFailed
        {
            self.require_recovery();
        }
        captured
    }

    /// Opens a storage snapshot, reserves the bytes of the version it shows, and pins that
    /// version, all under the read lock. The reservation comes before the pin: a reader that
    /// held a version it had not reserved yet could keep it alive past the next
    /// publication, uncounted, and one such reader per publication would retain without
    /// bound.
    fn pin<S: Storage>(
        &self,
        storage: &S,
        budget: &MemoryBudget,
    ) -> Result<Snapshot<S>, CaptureError> {
        let slots = self.read();
        // While the read lock is held, nothing can withdraw a version or set the flag, so a
        // store that does not need recovery has the version its storage shows, unless the
        // stored state is damaged.
        ensure!(!slots.recovery, RecoveryNeededSnafu);
        let snapshot = storage.snapshot().context(OpenSnapshotSnafu)?;
        #[cfg(test)]
        Self::run(&self.after_snapshot);
        let publication = read_state(&snapshot)
            .context(ReadPublicationSnafu)?
            .publication;
        let version = [Some(&slots.current), slots.pending.as_ref()]
            .into_iter()
            .flatten()
            .find(|version| version.publication == publication)
            .context(UnknownPublicationSnafu {
                found: publication,
                current: slots.current.publication,
                pending: slots.pending.as_ref().map(|version| version.publication),
            })?;
        let reservation = budget
            .try_reserve("snapshot", version.bytes())
            .context(ReserveQuerySnafu)?;
        let snapshot = Snapshot {
            storage: snapshot,
            version: reservation.hold(|| version.clone()),
            schema: self.schema.clone(),
            open: self.open.clone(),
            budget: budget.clone(),
        };
        #[cfg(test)]
        Self::run(&self.after_pin);
        Ok(snapshot)
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
    /// Resident credit stays with the current registry slot, independently of readers' claims.
    pub(crate) fn head_reserved(&self) -> u64 {
        self.read().head.bytes()
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
        let growth = self.growth.take().expect("a version is published once");
        let mut slots = self.registry.write();
        let version = slots
            .pending
            .take()
            .expect("the pending version stays registered until published or withdrawn");
        let bytes = version.head.bytes();
        // The old version is dropped here unless a snapshot holds it; a snapshot reserved
        // its bytes from the query budget when it was captured.
        slots.current = version;
        slots.head.merge(growth);
        let retired = slots.head.bytes() - bytes;
        slots.head.shrink(retired);
        drop(slots);
        drop(self.staging.take());
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
            slots.recovery = true;
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
/// budget, returned when it is dropped, and the budget itself, from which its queries
/// reserve their working memory (§7.4, §8.2).
pub struct Snapshot<S: Storage> {
    storage: S::Snapshot,
    version: Reserved<Arc<Version>>,
    schema: Arc<Schema>,
    open: Arc<OpenMarker>,
    budget: MemoryBudget,
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

    pub(crate) fn storage(&self) -> &S::Snapshot {
        &self.storage
    }

    pub(crate) fn version(&self) -> &Version {
        self.version.get()
    }

    pub(crate) fn schema(&self) -> &Schema {
        &self.schema
    }

    pub(crate) fn budget(&self) -> &MemoryBudget {
        &self.budget
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
    /// An earlier failure left the backend failed or a batch's outcome unknown. Every
    /// capture fails until the owner recovers (§7.5).
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
        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: staging.try_reserve("staging", 12).unwrap(),
        });
        let withdrawn = Arc::downgrade(&registry.pending().unwrap());
        show(&storage, 1);
        let snapshot = registry.capture(&storage, &queries).unwrap();
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
        let heads = MemoryBudget::new(10);

        let pending = registry.register(StagedVersion {
            version: version(1),
            growth: heads.try_reserve("head", 4).unwrap(),
            staging: MemoryBudget::new(u64::MAX)
                .try_reserve("staging", 0)
                .unwrap(),
        });
        show(&storage, 1);
        // Until it is withdrawn, the pending version is the one storage shows.
        let snapshot = registry.capture(&storage, &queries).unwrap();
        assert_eq!(snapshot.version().publication, 1);

        pending.fail(&CommitFailure::Unknown { source: failed() });
        assert!(registry.needs_recovery());
        assert!(registry.pending().is_none());
        assert_eq!(heads.reserved(), 0);
        let error = registry.capture(&storage, &queries).map(drop).unwrap_err();
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
        let (reserved_tx, reserved_rx) = mpsc::channel();
        let budget = queries.clone();
        registry.after_pin(move || reserved_tx.send(budget.reserved()).unwrap());

        let snapshot = registry.capture(&storage, &queries).unwrap();
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
        let heads = MemoryBudget::new(10);

        let error = registry.capture(&storage, &queries).map(drop).unwrap_err();
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
        let error = registry.capture(&storage, &queries).map(drop).unwrap_err();
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
        let snapshot = registry.capture(&storage, &queries).unwrap();
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
            registry.capture(&storage, &queries).map(drop),
            Err(CaptureError::RecoveryNeeded)
        ));
    }
}
