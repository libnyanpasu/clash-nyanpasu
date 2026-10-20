//! The redb backend (§3.4): one table per namespace, and fragments in a table keyed by
//! (chunk, index). redb source references are to `redb-4.4.0/src/`.
//!
//! # Layout
//!
//! - Namespace `n` is the table `namespace/n`, of byte-string keys and values. redb orders
//!   `&[u8]` keys bytewise (`types.rs:543-546`), as the port does.
//! - Fragments are the table `fragments`, keyed by `(chunk, index)` as `(u64, u32)`, which
//!   redb orders element by element, numerically. One chunk's fragments are therefore one key
//!   range, and `delete_chunk` removes `(chunk, 0)..=(chunk, u32::MAX)` with the table's
//!   range delete, `retain_in` (`table.rs:285-312`).
//! - A table that does not exist reads as empty, and no read creates one. A snapshot's
//!   `open_table` reports a missing table (`transactions.rs:2772-2779`); a batch's would
//!   create it (`transactions.rs:1615-1621`, `table_tree.rs:639-648`), so a batch lists the
//!   tables once, before the first read or delete that needs them.
//!
//! # Snapshots and batches
//!
//! A snapshot is a `ReadTransaction` and the tables it has opened, which are both `'static`
//! (`transactions.rs:2751`, `table.rs:859`) and hold no `Database`. Only the storage owns the
//! `Database`, so dropping the storage really closes it (§7.5 step 2).
//!
//! A batch is a `WriteTransaction`. It lends a value while the value's table is open, and a
//! write transaction opens a table once at a time (`transactions.rs:653-661`). A callback
//! that reads the same table of the same batch again violates this backend's contract, and
//! panics.
//!
//! # Durability
//!
//! `Durable` commits with redb's `Durability::Immediate`, `Consistent` with
//! `Durability::None` (`transactions.rs:374-381`). A durable commit writes its pages and the
//! header, then syncs once (`tree_store/page_store/page_manager.rs:1695-1757`).
//!
//! # Failures
//!
//! | redb error                                             | Port error                         |
//! | ------------------------------------------------------ | ---------------------------------- |
//! | `Io`, `PreviousIo`                                     | `Failed`, and latch                |
//! | `Corrupted`                                            | `Corrupted`                        |
//! | `ValueTooLarge`                                        | `Limit`                            |
//! | `DatabaseClosed`                                       | `Closed`                           |
//! | `LockPoisoned`, `Unsupported`, any later variant       | `Failed`, and latch                |
//! | a `TableError` other than a missing table (types)      | `Corrupted`                        |
//! | `DatabaseAlreadyOpen` on open                          | `Failed`, as the memory backend    |
//! | `UpgradeRequired` on open                              | `Corrupted`                        |
//! | `CommitError::TransactionPoisoned`                     | `NotApplied(RolledBack)`; usable   |
//! | `CommitError::Storage`                                 | `Unknown(Failed)`, and latch       |
//! | a commit on a latched instance                         | `NotApplied(Failed)`, before redb  |
//!
//! A poisoned commit has rolled back and new transactions may begin; after any other commit
//! error the batch may already be durable, and redb refuses new writes until it is reopened
//! (`transactions.rs:1744-1753`). Every error that becomes `Failed` latches the instance, so
//! that `Failed` always means "reopen".
//!
//! A batch dropped without a commit rolls back explicitly. A rollback can do I/O: it
//! truncates what the batch grew the file by, and a failure there latches redb with
//! `PreviousIo` (`transactions.rs:2039-2058`, `tree_store/page_store/page_manager.rs:2206-2216`).
//! redb's own drop of the transaction discards that error (`transactions.rs:2719-2726`), so
//! the instance would never learn of it. While the thread panics, the batch leaves the drop
//! to redb, which then skips the rollback (`transactions.rs:2721`, `:2733-2736`).
//!
//! redb serves cached pages without checking for an earlier I/O failure or a close:
//! `PagedCachedFile::read` returns hits from the write buffer and the read cache
//! (`tree_store/page_store/cached_file.rs:649-710`) before reaching `CheckedBackend::read`,
//! whose `check_failure` (`:195`, called at `:224`) reports `PreviousIo` or `DatabaseClosed`.
//! A table also keeps its root page (`tree_store/btree.rs:1014-1032`). A snapshot captured
//! before a failure or a close could therefore go on reading. The backend keeps its own
//! instance state instead, shared by the storage, its snapshots and its batch, and checks it
//! before every operation: the first error that becomes `Failed` latches it, and dropping
//! the storage closes it. A closed instance reports `Closed`, a latched one `Failed`.
//!
//! # Closing
//!
//! Dropping the storage closes the database: redb makes pending non-durable commits durable,
//! unless an I/O failure latched it (`db.rs:1885-1938`), and later reads of its snapshots
//! report `Closed`.
//!
//! # Test hooks
//!
//! The tests drive redb into most states for real, through a disk image in memory that fails
//! the I/O they choose. Two states need a `#[cfg(test)]` hook here instead:
//!
//! - A poisoned transaction. redb poisons one when an operation panics, or fails part way on
//!   a storage error (`table.rs:297-312`, `transactions.rs:741-804`). This backend catches no
//!   panic, and the storage errors the test disk can provoke are I/O errors, which latch the
//!   instance as well. The hook makes the next commit do what redb's commit does with a
//!   poisoned transaction: abort it and report `TransactionPoisoned`
//!   (`transactions.rs:1765-1768`).
//! - Damage that redb detects. redb verifies page checksums only when it repairs or checks
//!   the database (`db.rs:795-829`), not on reads, and one damaged page would damage every
//!   entry on it. The hook marks single entries and fragments, and reading one fails with the
//!   error redb reports for damage, `Corrupted`. A batch's own writes are not exempt.
//!
//! Both errors then go through the same mapping as real ones.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ops::ControlFlow,
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use redb::{
    AccessGuard, Builder, CommitError, CompactionError, Database, DatabaseError, Key,
    ReadOnlyTable, ReadTransaction, ReadableDatabase, ReadableTable, StorageBackend, Table,
    TableDefinition, TableError, TableHandle, TransactionError, Value, WriteTransaction,
};
use snafu::{ResultExt, Snafu, ensure};

use super::{
    ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyBounds, KeyRange, LimitSnafu,
    MAX_FRAGMENT_LEN, NotAppliedSnafu, Ns, Result, Storage, StorageError, StorageRead,
    StorageWrite, check_lent,
};
use crate::ErrorKind;

#[cfg(test)]
mod disk;
#[cfg(test)]
mod tests;

type Bytes = &'static [u8];
type FragmentKey = (u64, u32);
type NamespaceTable = ReadOnlyTable<Bytes, Bytes>;
type FragmentTable = ReadOnlyTable<FragmentKey, Bytes>;

const FRAGMENTS: TableDefinition<FragmentKey, Bytes> = TableDefinition::new("fragments");

fn namespace_name(ns: Ns) -> String {
    format!("namespace/{}", ns.0)
}

fn fragment_key(id: FragmentId) -> FragmentKey {
    (id.chunk.0, id.index)
}

/// Why the redb backend failed: the source of [`StorageError::Failed`] and
/// [`StorageError::RolledBack`]. A variant names the step that failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum RedbError {
    #[snafu(display("failed to open the redb database"))]
    Open { source: DatabaseError },

    #[snafu(display("failed to begin a read transaction"))]
    BeginRead { source: TransactionError },

    #[snafu(display("failed to begin a write transaction"))]
    BeginWrite { source: TransactionError },

    #[snafu(display("failed to list the tables of a write transaction"))]
    ListTables { source: redb::StorageError },

    #[snafu(display("failed to open table `{table}`"))]
    OpenTable { table: String, source: TableError },

    #[snafu(display("failed to read key {key:02x?} of namespace {}", ns.0))]
    Get {
        ns: Ns,
        key: Box<[u8]>,
        source: redb::StorageError,
    },

    #[snafu(display("failed to scan namespace {}", ns.0))]
    Scan { ns: Ns, source: redb::StorageError },

    #[snafu(display("failed to read fragment {} of chunk {}", id.index, id.chunk.0))]
    ReadFragment {
        id: FragmentId,
        source: redb::StorageError,
    },

    #[snafu(display("failed to write key {key:02x?} of namespace {}", ns.0))]
    Put {
        ns: Ns,
        key: Box<[u8]>,
        source: redb::StorageError,
    },

    #[snafu(display("failed to delete key {key:02x?} of namespace {}", ns.0))]
    Delete {
        ns: Ns,
        key: Box<[u8]>,
        source: redb::StorageError,
    },

    #[snafu(display("failed to write fragment {} of chunk {}", id.index, id.chunk.0))]
    PutFragment {
        id: FragmentId,
        source: redb::StorageError,
    },

    #[snafu(display("failed to delete chunk {}", chunk.0))]
    DeleteChunk {
        chunk: ChunkId,
        source: redb::StorageError,
    },

    #[snafu(display("failed to commit a batch"))]
    Commit { source: CommitError },

    #[snafu(display("failed to roll back an abandoned batch"))]
    Abort { source: redb::StorageError },

    #[snafu(display("failed to compact the database"))]
    Compact { source: CompactionError },

    /// An earlier failure latched this instance, like redb's `PreviousIo`.
    #[snafu(display("an earlier failure latched the redb storage"))]
    Latched,
}

/// Which port error a redb failure becomes: the table in the [module docs](self#failures).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Failed,
    Corrupted,
    Limit,
    Closed,
    RolledBack,
}

impl Outcome {
    fn of(error: &redb::StorageError) -> Self {
        match error {
            redb::StorageError::Corrupted(_) => Self::Corrupted,
            redb::StorageError::ValueTooLarge(_) => Self::Limit,
            redb::StorageError::DatabaseClosed => Self::Closed,
            // `Io` and `PreviousIo` latch redb itself. `LockPoisoned` means a redb thread
            // panicked holding an internal lock, `Unsupported` that the backend lacks an
            // operation redb needs; failing the instance is also the safe reading of any
            // variant a later redb adds.
            _ => Self::Failed,
        }
    }
}

impl RedbError {
    fn outcome(&self) -> Outcome {
        match self {
            Self::Open { source } => match source {
                DatabaseError::Storage(error) => Outcome::of(error),
                // A file format this redb does not read: the data is unusable, and reopening
                // does not help.
                DatabaseError::UpgradeRequired(_) => Outcome::Corrupted,
                _ => Outcome::Failed,
            },
            Self::BeginRead { source } | Self::BeginWrite { source } => match source {
                TransactionError::Storage(error) => Outcome::of(error),
                _ => Outcome::Failed,
            },
            Self::OpenTable { source, .. } => match source {
                TableError::Storage(error) => Outcome::of(error),
                // A table under a layout name with other types, say: not what this backend
                // wrote.
                _ => Outcome::Corrupted,
            },
            Self::ListTables { source }
            | Self::Get { source, .. }
            | Self::Scan { source, .. }
            | Self::ReadFragment { source, .. }
            | Self::Put { source, .. }
            | Self::Delete { source, .. }
            | Self::PutFragment { source, .. }
            | Self::DeleteChunk { source, .. }
            | Self::Abort { source } => Outcome::of(source),
            Self::Commit { source } => match source {
                CommitError::TransactionPoisoned => Outcome::RolledBack,
                // Whatever the cause, redb may have made the batch durable and refuses new
                // writes until it is reopened.
                _ => Outcome::Failed,
            },
            Self::Compact { source } => match source {
                CompactionError::Storage(error) => Outcome::of(error),
                _ => Outcome::Failed,
            },
            Self::Latched => Outcome::Failed,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self.outcome() {
            Outcome::Failed => ErrorKind::BackendFailed,
            Outcome::Corrupted => ErrorKind::Corrupted,
            Outcome::Limit | Outcome::Closed | Outcome::RolledBack => ErrorKind::Local,
        }
    }

    /// The port error, without the latch that [`Instance::report`] adds.
    fn into_port(self) -> StorageError {
        match self.outcome() {
            Outcome::Failed => StorageError::Failed {
                source: Box::new(self),
            },
            Outcome::Corrupted => StorageError::Corrupted {
                detail: chain(&self),
            },
            Outcome::Limit => StorageError::Limit {
                detail: chain(&self),
            },
            Outcome::Closed => StorageError::Closed,
            Outcome::RolledBack => StorageError::RolledBack {
                source: Box::new(self),
            },
        }
    }
}

/// The error and its sources on one line, for the port errors that carry a detail instead
/// of a source.
fn chain(error: &RedbError) -> String {
    std::iter::successors(Some(error as &dyn Error), |&error| error.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

/// One opening of the database, shared by the storage, its snapshots and its batch (see
/// [Failures](self#failures)).
#[derive(Default)]
struct Instance {
    /// Set by the first error that becomes `Failed`; every later operation fails, until
    /// the database is reopened as a new instance.
    failed: AtomicBool,
    /// Set when the storage drops.
    closed: AtomicBool,
    batch_open: AtomicBool,
    #[cfg(test)]
    hooks: Mutex<Hooks>,
}

impl Instance {
    /// Admits an operation of an instance that is open and has not failed.
    fn admit(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(StorageError::Closed);
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(RedbError::Latched.into_port());
        }
        Ok(())
    }

    /// Maps a redb failure onto the port, latching the instance when it becomes `Failed`.
    fn report(&self, error: RedbError) -> StorageError {
        if error.outcome() == Outcome::Failed {
            self.failed.store(true, Ordering::Release);
        }
        error.into_port()
    }

    /// Commits `txn`, or, once a test armed the hook, does what redb does with a poisoned
    /// transaction (see [Test hooks](self#test-hooks)).
    fn commit(&self, txn: WriteTransaction) -> Result<(), CommitError> {
        #[cfg(test)]
        if std::mem::take(&mut self.hooks().poison_next_commit) {
            return match txn.abort() {
                Ok(()) => Err(CommitError::TransactionPoisoned),
                Err(error) => Err(CommitError::Storage(error)),
            };
        }
        txn.commit()
    }

    /// Fails as redb does on damage it detects, where a test marked the entry as damaged.
    #[cfg_attr(not(test), expect(unused_variables))]
    fn check_entry(&self, ns: Ns, key: &[u8]) -> redb::Result<()> {
        #[cfg(test)]
        if self
            .hooks()
            .damaged_entries
            .get(&ns)
            .is_some_and(|keys| keys.contains(key))
        {
            return Err(redb::StorageError::Corrupted(
                "an entry a test marked as damaged".to_owned(),
            ));
        }
        Ok(())
    }

    /// Like [`Self::check_entry`], for a fragment.
    #[cfg_attr(not(test), expect(unused_variables))]
    fn check_fragment(&self, id: FragmentId) -> redb::Result<()> {
        #[cfg(test)]
        if self.hooks().damaged_fragments.contains(&id) {
            return Err(redb::StorageError::Corrupted(
                "a fragment a test marked as damaged".to_owned(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    fn hooks(&self) -> MutexGuard<'_, Hooks> {
        self.hooks.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the [test hooks](self#test-hooks) are armed with.
#[cfg(test)]
#[derive(Default)]
struct Hooks {
    poison_next_commit: bool,
    damaged_entries: BTreeMap<Ns, BTreeSet<Box<[u8]>>>,
    damaged_fragments: BTreeSet<FragmentId>,
}

/// What [`RedbStorage::compact`] did (§9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compaction {
    /// Pages moved toward the start of the file, and the free space this left at its end
    /// was trimmed.
    Compacted,
    /// No page could move. The commits before the attempt may still have trimmed free space
    /// at the end of the file.
    Unchanged,
    /// A snapshot was alive, so nothing happened. Compact again once every snapshot has
    /// been dropped; marking them revoked is not enough.
    Deferred,
}

/// A redb database, open. Dropping it closes the database (see [Closing](self#closing)).
pub struct RedbStorage {
    db: Database,
    instance: Arc<Instance>,
}

impl RedbStorage {
    /// Opens the database file at `path`, creating it when missing, with a page cache of
    /// `cache_bytes`: the profile's `backend_cache`. A database that was not closed cleanly is
    /// repaired first, in time that grows with its size (§10.2). Fails with
    /// [`StorageError::Failed`] while another storage has the file open.
    pub fn open(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        Self::opened(builder(cache_bytes).create(path))
    }

    /// Like [`Self::open`], on a backend of the caller's, such as an image in memory.
    pub fn open_with_backend(backend: impl StorageBackend, cache_bytes: u64) -> Result<Self> {
        Self::opened(builder(cache_bytes).create_with_backend(backend))
    }

    fn opened(db: Result<Database, DatabaseError>) -> Result<Self> {
        let db = db.context(OpenSnafu).map_err(RedbError::into_port)?;
        Ok(Self {
            db,
            instance: Arc::default(),
        })
    }

    /// Moves pages toward the start of the file and shrinks it (§9.3), when no snapshot is
    /// alive: redb refuses while a read transaction is (`db.rs:1086-1116`), and taking `self`
    /// mutably rules out a batch. It runs several durable commits, which also make pending
    /// non-durable commits durable, and takes time that grows with the file.
    pub fn compact(&mut self) -> Result<Compaction> {
        self.instance.admit()?;
        match self.db.compact() {
            Ok(true) => Ok(Compaction::Compacted),
            Ok(false) => Ok(Compaction::Unchanged),
            Err(CompactionError::TransactionInProgress) => Ok(Compaction::Deferred),
            Err(source) => Err(self.instance.report(RedbError::Compact { source })),
        }
    }
}

fn builder(cache_bytes: u64) -> Builder {
    let mut builder = Builder::new();
    // Never redb's default of 1 GiB (`db.rs:2045`). The crate builds for 64-bit targets only,
    // so the cast is lossless.
    builder.set_cache_size(cache_bytes as usize);
    builder
}

#[cfg(test)]
impl RedbStorage {
    /// Makes the next commit roll back, as a poisoned one does.
    fn poison_next_commit(&self) {
        self.instance.hooks().poison_next_commit = true;
    }

    /// Marks what is stored under `key` as damaged, for this instance.
    fn corrupt_entry(&self, ns: Ns, key: &[u8]) {
        let mut hooks = self.instance.hooks();
        hooks
            .damaged_entries
            .entry(ns)
            .or_default()
            .insert(key.into());
    }

    /// Marks a stored fragment as damaged, like [`Self::corrupt_entry`].
    fn corrupt_fragment(&self, id: FragmentId) {
        self.instance.hooks().damaged_fragments.insert(id);
    }
}

impl Drop for RedbStorage {
    fn drop(&mut self) {
        // Before the database closes, so that no snapshot reads a page redb still caches.
        self.instance.closed.store(true, Ordering::Release);
    }
}

impl Storage for RedbStorage {
    type Snapshot = RedbSnapshot;
    type Batch<'s> = RedbBatch<'s>;

    fn snapshot(&self) -> Result<RedbSnapshot> {
        self.instance.admit()?;
        let txn = self
            .db
            .begin_read()
            .context(BeginReadSnafu)
            .map_err(|error| self.instance.report(error))?;
        Ok(RedbSnapshot {
            txn,
            instance: self.instance.clone(),
            tables: Mutex::default(),
        })
    }

    /// # Panics
    ///
    /// When a batch of this storage is still open: the owner serializes writes, and redb
    /// would block forever on the second.
    fn batch(&self) -> Result<RedbBatch<'_>> {
        self.instance.admit()?;
        let nested = self.instance.batch_open.swap(true, Ordering::AcqRel);
        assert!(
            !nested,
            "a redb storage batch opened while another is open: the owner serializes writes"
        );
        let slot = BatchSlot(&self.instance.batch_open);
        let txn = self
            .db
            .begin_write()
            .context(BeginWriteSnafu)
            .map_err(|error| self.instance.report(error))?;
        Ok(RedbBatch {
            txn: Some(txn),
            instance: &self.instance,
            tables: RefCell::default(),
            _slot: slot,
        })
    }
}

/// Holds the storage's one batch slot until dropped.
struct BatchSlot<'s>(&'s AtomicBool);

impl Drop for BatchSlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A read transaction, readable on any thread until its storage closes.
pub struct RedbSnapshot {
    txn: ReadTransaction,
    instance: Arc<Instance>,
    /// The tables opened so far. They are shared out so that no lock is held while a value
    /// is lent: a callback may read the snapshot again.
    tables: Mutex<SnapshotTables>,
}

/// `None` for a table the snapshot does not have.
#[derive(Default)]
struct SnapshotTables {
    namespaces: BTreeMap<Ns, Option<Arc<NamespaceTable>>>,
    /// `None` until opened.
    fragments: Option<Option<Arc<FragmentTable>>>,
}

impl RedbSnapshot {
    fn namespace(&self, ns: Ns) -> Result<Option<Arc<NamespaceTable>>> {
        if let Some(table) = self.tables().namespaces.get(&ns) {
            return Ok(table.clone());
        }
        let name = namespace_name(ns);
        let table = self.open(TableDefinition::new(&name))?;
        self.tables().namespaces.insert(ns, table.clone());
        Ok(table)
    }

    fn fragments(&self) -> Result<Option<Arc<FragmentTable>>> {
        if let Some(table) = &self.tables().fragments {
            return Ok(table.clone());
        }
        let table = self.open(FRAGMENTS)?;
        self.tables().fragments = Some(table.clone());
        Ok(table)
    }

    fn open<K: Key + 'static, V: Value + 'static>(
        &self,
        definition: TableDefinition<K, V>,
    ) -> Result<Option<Arc<ReadOnlyTable<K, V>>>> {
        match self.txn.open_table(definition) {
            Ok(table) => Ok(Some(Arc::new(table))),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(source) => Err(self.instance.report(RedbError::OpenTable {
                table: definition.name().to_owned(),
                source,
            })),
        }
    }

    /// No code panics while holding the lock, so a poisoned one still guards consistent
    /// tables.
    fn tables(&self) -> MutexGuard<'_, SnapshotTables> {
        self.tables.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl StorageRead for RedbSnapshot {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.instance.admit()?;
        match self.namespace(ns)? {
            Some(table) => read_value(&*table, &self.instance, ns, key, f),
            None => Ok(f(None)),
        }
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.instance.admit()?;
        let Some(bounds) = range.bounds() else {
            return Ok(None);
        };
        match self.namespace(ns)? {
            Some(table) => scan_entries(&*table, &self.instance, ns, &bounds, direction, f),
            None => Ok(None),
        }
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.instance.admit()?;
        match self.fragments()? {
            Some(table) => read_fragment(&*table, &self.instance, id, limit, f),
            None => Ok(f(None)),
        }
    }
}

/// A write transaction over the state of the last commit.
pub struct RedbBatch<'s> {
    /// Taken by the commit, so that only an abandoned batch rolls back when dropped.
    txn: Option<WriteTransaction>,
    instance: &'s Instance,
    /// The tables the batch's view has: those of the last commit, listed on first need,
    /// and those the batch has created since (see [Layout](self#layout)).
    tables: RefCell<Option<BTreeSet<String>>>,
    /// Held until the batch drops, after the rollback of an abandoned one.
    _slot: BatchSlot<'s>,
}

impl RedbBatch<'_> {
    fn txn(&self) -> &WriteTransaction {
        self.txn
            .as_ref()
            .expect("only the commit takes the transaction, and it consumes the batch")
    }

    /// Opens a table the batch's view has, or returns `None`, without creating it.
    fn open_existing<K: Key + 'static, V: Value + 'static>(
        &self,
        definition: TableDefinition<K, V>,
    ) -> Result<Option<Table<'_, K, V>>> {
        if self.has_table(definition.name())? {
            self.open(definition).map(Some)
        } else {
            Ok(None)
        }
    }

    fn has_table(&self, name: &str) -> Result<bool> {
        if let Some(tables) = &*self.tables.borrow() {
            return Ok(tables.contains(name));
        }
        let tables: BTreeSet<String> = self
            .txn()
            .list_tables()
            .context(ListTablesSnafu)
            .map_err(|error| self.instance.report(error))?
            .map(|table| table.name().to_owned())
            .collect();
        let found = tables.contains(name);
        *self.tables.borrow_mut() = Some(tables);
        Ok(found)
    }

    /// Opens a table, creating it when the batch's view lacks it.
    fn open_or_create<K: Key + 'static, V: Value + 'static>(
        &self,
        definition: TableDefinition<K, V>,
    ) -> Result<Table<'_, K, V>> {
        let table = self.open(definition)?;
        if let Some(tables) = &mut *self.tables.borrow_mut() {
            tables.insert(definition.name().to_owned());
        }
        Ok(table)
    }

    /// # Panics
    ///
    /// When the table is already open: a callback read the table it was lent a value of
    /// (see [Snapshots and batches](self#snapshots-and-batches)).
    fn open<K: Key + 'static, V: Value + 'static>(
        &self,
        definition: TableDefinition<K, V>,
    ) -> Result<Table<'_, K, V>> {
        self.txn()
            .open_table(definition)
            .map_err(|source| match source {
                TableError::TableAlreadyOpen(table, opened_at) => panic!(
                    "a redb batch callback read table `{table}`, which is lending it a value \
                     (opened at {opened_at}): a write transaction opens a table once at a time"
                ),
                source => self.instance.report(RedbError::OpenTable {
                    table: definition.name().to_owned(),
                    source,
                }),
            })
    }
}

impl StorageRead for RedbBatch<'_> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.instance.admit()?;
        let name = namespace_name(ns);
        match self.open_existing(TableDefinition::new(&name))? {
            Some(table) => read_value(&table, self.instance, ns, key, f),
            None => Ok(f(None)),
        }
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.instance.admit()?;
        let Some(bounds) = range.bounds() else {
            return Ok(None);
        };
        let name = namespace_name(ns);
        match self.open_existing(TableDefinition::new(&name))? {
            Some(table) => scan_entries(&table, self.instance, ns, &bounds, direction, f),
            None => Ok(None),
        }
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.instance.admit()?;
        match self.open_existing(FRAGMENTS)? {
            Some(table) => read_fragment(&table, self.instance, id, limit, f),
            None => Ok(f(None)),
        }
    }
}

impl StorageWrite for RedbBatch<'_> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()> {
        self.instance.admit()?;
        let name = namespace_name(ns);
        let mut table = self.open_or_create(TableDefinition::<Bytes, Bytes>::new(&name))?;
        table
            .insert(key, value)
            .context(PutSnafu { ns, key })
            .map_err(|error| self.instance.report(error))?;
        Ok(())
    }

    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()> {
        self.instance.admit()?;
        let name = namespace_name(ns);
        let Some(mut table) = self.open_existing(TableDefinition::<Bytes, Bytes>::new(&name))?
        else {
            return Ok(());
        };
        table
            .remove(key)
            .context(DeleteSnafu { ns, key })
            .map_err(|error| self.instance.report(error))?;
        Ok(())
    }

    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()> {
        // A failed instance reports `Failed` whatever the call, so it goes first.
        self.instance.admit()?;
        // Before redb sees it, so that the batch is left as it was.
        ensure!(
            bytes.len() <= MAX_FRAGMENT_LEN,
            LimitSnafu {
                detail: format!(
                    "a fragment of {} bytes, above {MAX_FRAGMENT_LEN}",
                    bytes.len()
                ),
            }
        );
        let mut table = self.open_or_create(FRAGMENTS)?;
        table
            .insert(fragment_key(id), bytes)
            .context(PutFragmentSnafu { id })
            .map_err(|error| self.instance.report(error))?;
        Ok(())
    }

    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()> {
        self.instance.admit()?;
        let Some(mut table) = self.open_existing(FRAGMENTS)? else {
            return Ok(());
        };
        table
            .retain_in((chunk.0, 0)..=(chunk.0, u32::MAX), |_, _| false)
            .context(DeleteChunkSnafu { chunk })
            .map_err(|error| self.instance.report(error))
    }

    fn commit(mut self, durability: Durability) -> Result<(), CommitFailure> {
        let instance = self.instance;
        // A failed instance refuses the commit before it starts: never `Unknown` (§3.2).
        instance.admit().context(NotAppliedSnafu)?;
        let mut txn = self
            .txn
            .take()
            .expect("only the commit takes the transaction");
        let durability = match durability {
            Durability::Durable => redb::Durability::Immediate,
            Durability::Consistent => redb::Durability::None,
        };
        txn.set_durability(durability).expect(
            "redb refuses a durability only to a transaction that changed a persistent \
             savepoint, and this backend makes none",
        );
        let committed = instance.commit(txn);
        // Frees the batch slot; the transaction is gone, so nothing rolls back.
        drop(self);
        committed.map_err(|source| {
            let error = instance.report(RedbError::Commit { source });
            if matches!(error, StorageError::RolledBack { .. }) {
                CommitFailure::NotApplied { source: error }
            } else {
                CommitFailure::Unknown { source: error }
            }
        })
    }
}

/// Rolls an abandoned batch back, latching the instance when that fails (see
/// [Failures](self#failures)).
impl Drop for RedbBatch<'_> {
    fn drop(&mut self) {
        let Some(txn) = self.txn.take() else {
            return;
        };
        if std::thread::panicking() {
            return;
        }
        if let Err(source) = txn.abort() {
            self.instance.report(RedbError::Abort { source });
        }
    }
}

// The reads of a snapshot and a batch, on a table either has opened.

fn read_value<T>(
    table: &impl ReadableTable<Bytes, Bytes>,
    instance: &Instance,
    ns: Ns,
    key: &[u8],
    f: impl FnOnce(Option<&[u8]>) -> T,
) -> Result<T> {
    let value = table
        .get(key)
        .and_then(|value| {
            if value.is_some() {
                instance.check_entry(ns, key)?;
            }
            Ok(value)
        })
        .context(GetSnafu { ns, key })
        .map_err(|error| instance.report(error))?;
    Ok(f(value.as_ref().map(|value| value.value())))
}

fn scan_entries<B>(
    table: &impl ReadableTable<Bytes, Bytes>,
    instance: &Instance,
    ns: Ns,
    bounds: &KeyBounds<'_>,
    direction: Direction,
    f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
) -> Result<Option<B>> {
    table
        .range::<&[u8]>(bounds.as_range())
        .and_then(|entries| match direction {
            Direction::Forward => visit(entries, instance, ns, f),
            Direction::Backward => visit(entries.rev(), instance, ns, f),
        })
        .context(ScanSnafu { ns })
        .map_err(|error| instance.report(error))
}

/// Lends each entry to `f` until it breaks.
fn visit<'a, B>(
    entries: impl Iterator<Item = redb::Result<(AccessGuard<'a, Bytes>, AccessGuard<'a, Bytes>)>>,
    instance: &Instance,
    ns: Ns,
    mut f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
) -> redb::Result<Option<B>> {
    for entry in entries {
        let (key, value) = entry?;
        instance.check_entry(ns, key.value())?;
        if let ControlFlow::Break(broke) = f(key.value(), value.value()) {
            return Ok(Some(broke));
        }
    }
    Ok(None)
}

/// Reads fragment `id` and lends it to `f` when it holds at most `limit` bytes (see
/// [`StorageRead::fragment`]).
///
/// redb learns a value's length only from the leaf that holds it, so it reads that leaf into
/// its page cache, where `set_cache_size` (the profile's `backend_cache`) counts it, before the
/// [`AccessGuard`] gives the length. A fragment past `limit` is refused on the guard and the
/// guard dropped: its bytes stay in the cache, counted, or are freed, and are never lent. One
/// within `limit` is lent through the guard, which keeps the page alive should the cache evict
/// it meanwhile: that is what the reader reserved (§7.4, "后端在缓存之外的开销").
fn read_fragment<T>(
    table: &impl ReadableTable<FragmentKey, Bytes>,
    instance: &Instance,
    id: FragmentId,
    limit: usize,
    f: impl FnOnce(Option<&[u8]>) -> T,
) -> Result<T> {
    let bytes = table
        .get(fragment_key(id))
        .and_then(|bytes| {
            if bytes.is_some() {
                instance.check_fragment(id)?;
            }
            Ok(bytes)
        })
        .context(ReadFragmentSnafu { id })
        .map_err(|error| instance.report(error))?;
    let lent = bytes.as_ref().map(AccessGuard::value);
    if let Some(lent) = lent {
        check_lent(id, lent.len(), limit)?;
    }
    Ok(f(lent))
}
