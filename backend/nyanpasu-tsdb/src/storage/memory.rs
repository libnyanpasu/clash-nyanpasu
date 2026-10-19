//! The in-memory backend (§3.4): one `BTreeMap` per namespace, and fragments as byte
//! buffers. Every conformance and property test runs on it first.
//!
//! It models durability the way redb behaves, so that upper layers can test recovery
//! without a disk. A [`MemoryDisk`] holds two images: the *visible* one, which the last
//! commit left, and the *durable* one, which the last durable commit left. Opening the
//! disk starts from the durable image, so whatever a close did not promote is lost:
//!
//! - closing cleanly (dropping the [`MemoryStorage`]) promotes the visible image, as redb
//!   makes pending non-durable commits durable when it closes
//!   (`redb-4.4.0/src/db.rs:1885-1912`);
//! - closing an instance that has failed promotes nothing, as redb writes nothing after
//!   a latched I/O failure;
//! - a simulated crash (test-only) promotes nothing either.
//!
//! Images share structure: each namespace, chunk and value sits behind its own `Arc`, and
//! a commit copies only the namespaces and chunks it writes. A snapshot is one image, so
//! it stays stable and costs nothing to capture.
//!
//! Tests can also fail the next operation of a class, choose how the next commit ends,
//! and mark stored data as corrupted; see the test-only methods of [`MemoryStorage`].

use std::{
    cell::Cell,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use snafu::{OptionExt, ResultExt, Snafu, ensure};

#[cfg(test)]
use super::conformance::{CommitOutcome, OpClass};
use super::{
    ChunkId, ClosedSnafu, CommitFailure, CorruptedSnafu, Direction, Durability, FragmentId,
    KeyRange, LimitSnafu, MAX_FRAGMENT_LEN, NotAppliedSnafu, Ns, Result, Storage, StorageError,
    StorageRead, StorageWrite, check_lent,
};
use crate::ErrorKind;

/// Why the memory backend failed: the source of [`StorageError::Failed`] or
/// [`StorageError::RolledBack`].
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum MemoryError {
    /// Another storage is open on the disk, like redb's `DatabaseAlreadyOpen`.
    #[snafu(display("the memory disk is already open"))]
    AlreadyOpen,

    /// An earlier failure latched this instance, like redb's `PreviousIo`.
    #[snafu(display("an earlier failure latched the memory storage"))]
    PreviousIo,

    /// An I/O failure a test injected.
    #[snafu(display("injected I/O failure"))]
    InjectedIo,

    /// A rollback a test injected, like redb's `TransactionPoisoned`.
    #[snafu(display("injected rollback"))]
    InjectedRollback,
}

impl MemoryError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::AlreadyOpen | Self::PreviousIo | Self::InjectedIo => ErrorKind::BackendFailed,
            Self::InjectedRollback => ErrorKind::Local,
        }
    }
}

fn failed(error: MemoryError) -> StorageError {
    StorageError::Failed {
        source: Box::new(error),
    }
}

/// A simulated disk. Clones share it, the way two handles name one file.
#[derive(Clone, Default)]
pub struct MemoryDisk {
    disk: Arc<Mutex<Disk>>,
}

impl MemoryDisk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the disk at its durable image. Fails with [`StorageError::Failed`]
    /// ([`MemoryError::AlreadyOpen`]) while another storage has it open.
    pub fn open(&self) -> Result<MemoryStorage> {
        let mut disk = lock(&self.disk);
        if disk.open.is_some() {
            return Err(failed(MemoryError::AlreadyOpen));
        }
        disk.opened += 1;
        let generation = disk.opened;
        disk.open = Some(Instance {
            generation,
            latched: false,
            batch_open: false,
            #[cfg(test)]
            faults: Faults::default(),
        });
        disk.visible = disk.durable.clone();
        Ok(MemoryStorage {
            handle: Handle {
                disk: self.disk.clone(),
                generation,
            },
        })
    }
}

/// No code panics while holding the lock (contract violations panic after releasing it),
/// so a poisoned lock still guards a consistent disk. Recovering keeps a test that panics
/// on purpose from aborting in a `Drop` that locks again during unwinding.
fn lock(disk: &Mutex<Disk>) -> MutexGuard<'_, Disk> {
    disk.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct Disk {
    /// The state after the last durable commit: what a crash returns to.
    durable: Arc<Image>,
    /// The state after the last commit, which snapshots and batches start from.
    visible: Arc<Image>,
    open: Option<Instance>,
    /// How many instances were opened, which numbers them: a snapshot of an earlier
    /// instance reads as closed even after the disk is reopened.
    opened: u64,
    /// Stored data that fails its checksum. Empty outside tests; kept in every build so
    /// that the read paths need no test-only branches.
    marks: Arc<Marks>,
}

impl Disk {
    /// The instance `generation`, while it is open.
    fn open_instance(&mut self, generation: u64) -> Option<&mut Instance> {
        self.open
            .as_mut()
            .filter(|instance| instance.generation == generation)
    }

    /// The instance `generation`, if it can run an operation: open, and not failed.
    fn live_instance(&mut self, generation: u64) -> Result<&mut Instance> {
        let instance = self.open_instance(generation).context(ClosedSnafu)?;
        if instance.latched {
            return Err(failed(MemoryError::PreviousIo));
        }
        Ok(instance)
    }

    fn publish(&mut self, image: Arc<Image>, durability: Durability) {
        if durability == Durability::Durable {
            self.durable = image.clone();
        }
        self.visible = image;
    }
}

/// One opening of the disk.
struct Instance {
    generation: u64,
    /// Set by a failure; every later operation of the instance fails, as after redb's
    /// `PreviousIo`, until the disk is reopened.
    latched: bool,
    batch_open: bool,
    #[cfg(test)]
    faults: Faults,
}

/// The faults a test armed for an instance's next operations.
#[cfg(test)]
#[derive(Default)]
struct Faults {
    /// An I/O failure for the next read or write.
    io: Option<OpClass>,
    commit: Option<CommitOutcome>,
}

#[cfg(test)]
impl Instance {
    fn take_fault(&mut self, class: OpClass) -> Result<()> {
        if self.faults.io.take_if(|armed| *armed == class).is_some() {
            self.latched = true;
            return Err(failed(MemoryError::InjectedIo));
        }
        Ok(())
    }

    /// Plays the armed commit outcome, if any. `Err` ends the commit without applying the
    /// batch; `Ok(true)` applies it, and the commit then reports `Unknown`.
    fn take_commit_fault(&mut self) -> Result<bool, CommitFailure> {
        match self.faults.commit.take() {
            None => Ok(false),
            Some(CommitOutcome::NotApplied) => Err(CommitFailure::NotApplied {
                source: StorageError::RolledBack {
                    source: Box::new(MemoryError::InjectedRollback),
                },
            }),
            Some(CommitOutcome::Unknown { applied }) => {
                self.latched = true;
                if !applied {
                    return Err(CommitFailure::Unknown {
                        source: failed(MemoryError::InjectedIo),
                    });
                }
                Ok(true)
            }
        }
    }
}

#[derive(Clone, Default)]
struct Image {
    namespaces: BTreeMap<Ns, Arc<Entries>>,
    chunks: BTreeMap<ChunkId, Arc<Fragments>>,
}

type Entries = BTreeMap<Arc<[u8]>, Arc<[u8]>>;
type Fragments = BTreeMap<u32, Arc<[u8]>>;
/// A batch's puts, and its deletes as `None`.
type Writes = BTreeMap<Arc<[u8]>, Option<Arc<[u8]>>>;

#[derive(Clone, Default)]
struct Marks {
    entries: BTreeMap<Ns, BTreeSet<Box<[u8]>>>,
    fragments: BTreeSet<FragmentId>,
}

impl Marks {
    fn check_entry(&self, ns: Ns, key: &[u8]) -> Result<()> {
        let damaged = self.entries.get(&ns).is_some_and(|keys| keys.contains(key));
        ensure!(
            !damaged,
            CorruptedSnafu {
                detail: format!("the value of key {key:02x?} in namespace {}", ns.0),
            }
        );
        Ok(())
    }

    fn check_fragment(&self, id: FragmentId) -> Result<()> {
        ensure!(
            !self.fragments.contains(&id),
            CorruptedSnafu {
                detail: format!("fragment {} of chunk {}", id.index, id.chunk.0),
            }
        );
        Ok(())
    }
}

/// A batch's writes, over the image it started from.
#[derive(Default)]
struct Overlay {
    entries: BTreeMap<Ns, Writes>,
    deleted_chunks: BTreeSet<ChunkId>,
    /// Only the fragments put after their chunk's last deletion in this batch: deleting a
    /// chunk drops the puts before it.
    fragments: BTreeMap<FragmentId, Arc<[u8]>>,
}

impl Overlay {
    /// The batch's own write of `key`: `Some(None)` for a delete.
    fn entry(&self, ns: Ns, key: &[u8]) -> Option<Option<&[u8]>> {
        self.entries.get(&ns)?.get(key).map(Option::as_deref)
    }

    fn apply(self, base: &Image) -> Image {
        let mut image = base.clone();
        for (ns, writes) in self.entries {
            // Copies this namespace's map while snapshots still share it; namespaces the
            // batch did not write stay shared.
            let entries = Arc::make_mut(image.namespaces.entry(ns).or_default());
            for (key, value) in writes {
                match value {
                    Some(value) => entries.insert(key, value),
                    None => entries.remove(&key),
                };
            }
        }
        for chunk in self.deleted_chunks {
            image.chunks.remove(&chunk);
        }
        for (id, bytes) in self.fragments {
            Arc::make_mut(image.chunks.entry(id.chunk).or_default()).insert(id.index, bytes);
        }
        image
    }
}

#[derive(Clone)]
struct Handle {
    disk: Arc<Mutex<Disk>>,
    generation: u64,
}

impl Handle {
    fn lock(&self) -> MutexGuard<'_, Disk> {
        lock(&self.disk)
    }

    /// Admits a read: returns the corruption marks it has to honour.
    fn admit_read(&self) -> Result<Arc<Marks>> {
        let mut disk = self.lock();
        #[cfg(not(test))]
        disk.live_instance(self.generation)?;
        #[cfg(test)]
        disk.live_instance(self.generation)?
            .take_fault(OpClass::Read)?;
        Ok(disk.marks.clone())
    }

    fn admit_write(&self) -> Result<()> {
        let mut disk = self.lock();
        #[cfg(not(test))]
        disk.live_instance(self.generation)?;
        #[cfg(test)]
        disk.live_instance(self.generation)?
            .take_fault(OpClass::Write)?;
        Ok(())
    }

    fn close(&self, promote: bool) {
        let mut disk = self.lock();
        let generation = self.generation;
        let Some(instance) = disk.open.take_if(|open| open.generation == generation) else {
            return;
        };
        if promote && !instance.latched {
            disk.durable = disk.visible.clone();
        }
    }
}

/// An open [`MemoryDisk`]. Dropping it closes the disk cleanly.
pub struct MemoryStorage {
    handle: Handle,
}

impl Drop for MemoryStorage {
    fn drop(&mut self) {
        self.handle.close(true);
    }
}

impl Storage for MemoryStorage {
    type Snapshot = MemorySnapshot;
    type Batch<'s> = MemoryBatch<'s>;

    fn snapshot(&self) -> Result<MemorySnapshot> {
        let mut disk = self.handle.lock();
        disk.live_instance(self.handle.generation)?;
        Ok(MemorySnapshot {
            image: disk.visible.clone(),
            handle: self.handle.clone(),
        })
    }

    /// # Panics
    ///
    /// When a batch of this storage is still open: the owner serializes writes.
    fn batch(&self) -> Result<MemoryBatch<'_>> {
        let mut disk = self.handle.lock();
        let instance = disk.live_instance(self.handle.generation)?;
        let nested = std::mem::replace(&mut instance.batch_open, true);
        let base = disk.visible.clone();
        drop(disk);
        assert!(
            !nested,
            "a memory storage batch opened while another is open: the owner serializes writes"
        );
        Ok(MemoryBatch {
            storage: self,
            base,
            overlay: Overlay::default(),
            reading: Cell::new(false),
        })
    }
}

/// Durable-image controls for exhaustive schedules; none owns a running backend.
#[cfg(test)]
impl MemoryDisk {
    /// Forks only committed durable state into an independent, unopened fixture.
    pub(crate) fn fork(&self) -> Self {
        let durable = lock(&self.disk).durable.clone();
        Self {
            disk: Arc::new(Mutex::new(Disk {
                durable: durable.clone(),
                visible: durable,
                ..Disk::default()
            })),
        }
    }

    /// Invalidates handles without promoting visible commits; Drop cannot undo this crash.
    pub(crate) fn crash_now(&self) {
        lock(&self.disk).open = None;
    }

    /// Models a latched port failure in the existing engine, including its close behavior.
    pub(crate) fn fail_now(&self) {
        if let Some(instance) = &mut lock(&self.disk).open {
            instance.latched = true;
        }
    }

    /// Includes every fragment, even an orphan that no manifest entry names.
    pub(crate) fn durable_dump(&self) -> crate::verification::Dataset {
        let image = lock(&self.disk).durable.clone();
        crate::verification::Dataset {
            entries: image
                .namespaces
                .iter()
                .flat_map(|(ns, entries)| {
                    entries
                        .iter()
                        .map(move |(key, value)| (ns.0, key.to_vec(), value.to_vec()))
                })
                .collect(),
            fragments: image
                .chunks
                .iter()
                .flat_map(|(chunk, fragments)| {
                    fragments.iter().map(move |(index, value)| {
                        (
                            FragmentId {
                                chunk: *chunk,
                                index: *index,
                            },
                            value.to_vec(),
                        )
                    })
                })
                .collect(),
        }
    }
}

#[cfg(test)]
impl MemoryStorage {
    /// Closes the disk the way a crash would: nothing is promoted, and the next open
    /// starts from the last durable commit.
    pub(crate) fn crash(self) {
        self.handle.close(false);
    }

    /// Fails the next operation of `class` with an I/O failure that latches the
    /// instance. A failure inside a commit leaves its outcome unknown; the model leaves
    /// the batch out.
    pub(crate) fn fail_next(&self, class: OpClass) {
        let mut disk = self.handle.lock();
        let faults = &mut self.instance(&mut disk).faults;
        match class {
            OpClass::Commit => faults.commit = Some(CommitOutcome::Unknown { applied: false }),
            OpClass::Read | OpClass::Write => faults.io = Some(class),
        }
    }

    /// Makes the next commit end with `outcome`.
    pub(crate) fn inject_commit(&self, outcome: CommitOutcome) {
        let mut disk = self.handle.lock();
        self.instance(&mut disk).faults.commit = Some(outcome);
    }

    /// Marks what is stored under `key` as damaged, for as long as the disk exists:
    /// reading it reports [`StorageError::Corrupted`].
    pub(crate) fn corrupt_entry(&self, ns: Ns, key: &[u8]) {
        let mut disk = self.handle.lock();
        let marks = Arc::make_mut(&mut disk.marks);
        marks.entries.entry(ns).or_default().insert(key.into());
    }

    /// Marks a stored fragment as damaged, like [`Self::corrupt_entry`].
    pub(crate) fn corrupt_fragment(&self, id: FragmentId) {
        let mut disk = self.handle.lock();
        Arc::make_mut(&mut disk.marks).fragments.insert(id);
    }

    fn instance<'d>(&self, disk: &'d mut Disk) -> &'d mut Instance {
        disk.open_instance(self.handle.generation)
            .expect("an open storage's instance is open")
    }
}

/// An immutable image of the disk, readable on any thread until its storage closes.
pub struct MemorySnapshot {
    image: Arc<Image>,
    handle: Handle,
}

impl MemorySnapshot {
    fn view(&self) -> Result<View<'_>> {
        Ok(View {
            image: &self.image,
            overlay: None,
            marks: self.handle.admit_read()?,
        })
    }
}

impl StorageRead for MemorySnapshot {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.view()?.get(ns, key, f)
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.view()?.scan(ns, range, direction, f)
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.view()?.fragment(id, limit, f)
    }
}

/// A write transaction over the image of the last commit.
pub struct MemoryBatch<'s> {
    storage: &'s MemoryStorage,
    base: Arc<Image>,
    overlay: Overlay,
    /// Set while a read callback runs, to refuse a read that re-enters the batch, as redb
    /// does (see [`StorageWrite`]).
    reading: Cell<bool>,
}

impl MemoryBatch<'_> {
    /// Runs one read of the batch.
    ///
    /// # Panics
    ///
    /// If it is called from the callback of another read of the same batch: the port
    /// forbids it, and redb refuses it, so the store must not depend on it.
    fn read<R>(&self, read: impl FnOnce(View<'_>) -> Result<R>) -> Result<R> {
        assert!(
            !self.reading.replace(true),
            "a read callback of a batch read the batch again, which the storage port forbids"
        );
        let result = self.view().and_then(read);
        self.reading.set(false);
        result
    }

    fn view(&self) -> Result<View<'_>> {
        Ok(View {
            image: &self.base,
            overlay: Some(&self.overlay),
            marks: self.storage.handle.admit_read()?,
        })
    }
}

impl Drop for MemoryBatch<'_> {
    fn drop(&mut self) {
        let handle = &self.storage.handle;
        if let Some(instance) = handle.lock().open_instance(handle.generation) {
            instance.batch_open = false;
        }
    }
}

impl StorageRead for MemoryBatch<'_> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.read(|view| view.get(ns, key, f))
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.read(|view| view.scan(ns, range, direction, f))
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.read(|view| view.fragment(id, limit, f))
    }
}

impl StorageWrite for MemoryBatch<'_> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()> {
        self.storage.handle.admit_write()?;
        let writes = self.overlay.entries.entry(ns).or_default();
        writes.insert(key.into(), Some(value.into()));
        Ok(())
    }

    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()> {
        self.storage.handle.admit_write()?;
        self.overlay
            .entries
            .entry(ns)
            .or_default()
            .insert(key.into(), None);
        Ok(())
    }

    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()> {
        // A failed instance reports `Failed` whatever the call, so it goes first; the size
        // check still comes before any change.
        self.storage.handle.admit_write()?;
        ensure!(
            bytes.len() <= MAX_FRAGMENT_LEN,
            LimitSnafu {
                detail: format!(
                    "a fragment of {} bytes, above {MAX_FRAGMENT_LEN}",
                    bytes.len()
                ),
            }
        );
        self.overlay.fragments.insert(id, bytes.into());
        Ok(())
    }

    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()> {
        self.storage.handle.admit_write()?;
        self.overlay.fragments.retain(|id, _| id.chunk != chunk);
        self.overlay.deleted_chunks.insert(chunk);
        Ok(())
    }

    fn commit(mut self, durability: Durability) -> Result<(), CommitFailure> {
        let overlay = std::mem::take(&mut self.overlay);
        let handle = &self.storage.handle;
        let mut disk = handle.lock();
        // A failed instance refuses the commit before it starts: never `Unknown` (§3.2).
        #[cfg(not(test))]
        disk.live_instance(handle.generation)
            .context(NotAppliedSnafu)?;
        #[cfg(test)]
        let unknown = disk
            .live_instance(handle.generation)
            .context(NotAppliedSnafu)?
            .take_commit_fault()?;
        // One batch at a time, so the visible image is still the one the batch started
        // from.
        disk.publish(Arc::new(overlay.apply(&self.base)), durability);
        #[cfg(test)]
        if unknown {
            return Err(CommitFailure::Unknown {
                source: failed(MemoryError::InjectedIo),
            });
        }
        Ok(())
    }
}

/// What one read sees: an image, and in a batch the batch's own writes over it.
struct View<'a> {
    image: &'a Image,
    overlay: Option<&'a Overlay>,
    marks: Arc<Marks>,
}

impl View<'_> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        if let Some(written) = self.overlay.and_then(|overlay| overlay.entry(ns, key)) {
            return Ok(f(written));
        }
        let stored = self
            .image
            .namespaces
            .get(&ns)
            .and_then(|entries| entries.get(key));
        if stored.is_some() {
            self.marks.check_entry(ns, key)?;
        }
        Ok(f(stored.map(|value| &**value)))
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        mut f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        let Some(bounds) = range.bounds() else {
            return Ok(None);
        };
        let range = bounds.as_range();
        let stored = self
            .image
            .namespaces
            .get(&ns)
            .into_iter()
            .flat_map(|entries| entries.range::<[u8], _>(range))
            .map(|(key, value)| (&**key, &**value));
        let writes = self.overlay.and_then(|overlay| overlay.entries.get(&ns));
        let written = writes
            .into_iter()
            .flat_map(|writes| writes.range::<[u8], _>(range))
            .map(|(key, value)| (&**key, value.as_deref()));
        match direction {
            Direction::Forward => self.merge(ns, stored, written, |a, b| a.cmp(b), &mut f),
            Direction::Backward => {
                self.merge(ns, stored.rev(), written.rev(), |a, b| b.cmp(a), &mut f)
            }
        }
    }

    /// Visits the stored entries and the batch's writes as one sequence, in the order
    /// `order` gives; a write replaces the stored entry with its key, and a delete hides
    /// it.
    fn merge<'k, B>(
        &self,
        ns: Ns,
        stored: impl Iterator<Item = (&'k [u8], &'k [u8])>,
        written: impl Iterator<Item = (&'k [u8], Option<&'k [u8]>)>,
        order: impl Fn(&[u8], &[u8]) -> Ordering,
        f: &mut impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        let mut stored = stored.peekable();
        let mut written = written.peekable();
        loop {
            let from_stored = match (stored.peek(), written.peek()) {
                (None, None) => return Ok(None),
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(&(stored_key, _)), Some(&(written_key, _))) => {
                    match order(stored_key, written_key) {
                        Ordering::Less => true,
                        Ordering::Equal => {
                            stored.next();
                            false
                        }
                        Ordering::Greater => false,
                    }
                }
            };
            let (key, value) = if from_stored {
                let (key, value) = stored.next().expect("peeked");
                self.marks.check_entry(ns, key)?;
                (key, value)
            } else {
                match written.next().expect("peeked") {
                    (key, Some(value)) => (key, value),
                    (_, None) => continue,
                }
            };
            if let ControlFlow::Break(value) = f(key, value) {
                return Ok(Some(value));
            }
        }
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        if let Some(overlay) = self.overlay {
            if let Some(bytes) = overlay.fragments.get(&id) {
                check_lent(id, bytes.len(), limit)?;
                return Ok(f(Some(bytes)));
            }
            if overlay.deleted_chunks.contains(&id.chunk) {
                return Ok(f(None));
            }
        }
        let chunk = self.image.chunks.get(&id.chunk);
        let stored = chunk.and_then(|fragments| fragments.get(&id.index));
        if let Some(bytes) = stored {
            self.marks.check_fragment(id)?;
            check_lent(id, bytes.len(), limit)?;
        }
        Ok(f(stored.map(|bytes| &**bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::conformance::{self, Harness};

    struct MemoryHarness {
        disk: MemoryDisk,
        storage: Option<MemoryStorage>,
    }

    impl Harness for MemoryHarness {
        type Storage = MemoryStorage;

        fn fresh() -> Self {
            let disk = MemoryDisk::new();
            let storage = disk.open().expect("open a fresh disk");
            Self {
                disk,
                storage: Some(storage),
            }
        }

        fn storage(&self) -> &MemoryStorage {
            self.storage
                .as_ref()
                .expect("the harness keeps a storage open")
        }

        fn reopen(&mut self) {
            self.storage = None;
            self.storage = Some(self.disk.open().expect("reopen"));
        }

        fn crash_and_reopen(&mut self) {
            let storage = self.storage.take();
            storage.expect("the harness keeps a storage open").crash();
            self.storage = Some(self.disk.open().expect("reopen after a crash"));
        }

        fn fail_next(&self, class: OpClass) {
            self.storage().fail_next(class);
        }

        fn inject_commit(&self, outcome: CommitOutcome) {
            self.storage().inject_commit(outcome);
        }

        fn corrupt_entry(&self, ns: Ns, key: &[u8]) {
            self.storage().corrupt_entry(ns, key);
        }

        fn corrupt_fragment(&self, id: FragmentId) {
            self.storage().corrupt_fragment(id);
        }
    }

    conformance::conformance_tests!(MemoryHarness);

    const A: Ns = Ns(1);
    const B: Ns = Ns(2);

    fn frag(chunk: u64, index: u32) -> FragmentId {
        FragmentId {
            chunk: ChunkId(chunk),
            index,
        }
    }

    fn get(read: &impl StorageRead, ns: Ns, key: &[u8]) -> Option<Vec<u8>> {
        read.get(ns, key, |value| value.map(<[u8]>::to_vec))
            .expect("get")
    }

    #[test]
    fn a_second_open_fails_until_the_first_closes() {
        let disk = MemoryDisk::new();
        let storage = disk.open().expect("first open");
        // A clone names the same disk.
        let Err(error) = disk.clone().open() else {
            panic!("a second open succeeded");
        };
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        let StorageError::Failed { source } = &error else {
            panic!("unexpected error {error:?}");
        };
        assert!(matches!(
            source.downcast_ref::<MemoryError>(),
            Some(MemoryError::AlreadyOpen)
        ));
        drop(storage);
        disk.open().expect("open after the first closed");
    }

    #[test]
    #[should_panic(expected = "the owner serializes writes")]
    fn a_nested_batch_panics() {
        let disk = MemoryDisk::new();
        let storage = disk.open().expect("open");
        let _first = storage.batch().expect("first batch");
        let _second = storage.batch();
    }

    #[test]
    #[should_panic(expected = "which the storage port forbids")]
    fn a_read_callback_that_reads_the_batch_again_panics() {
        let disk = MemoryDisk::new();
        let storage = disk.open().expect("open");
        let mut batch = storage.batch().expect("batch");
        batch.put(A, b"k", b"v").expect("put");
        let _ = batch.get(A, b"k", |_| batch.get(A, b"k", |value| value.is_some()));
    }

    #[test]
    fn closing_a_failed_instance_promotes_nothing() {
        let disk = MemoryDisk::new();
        let storage = disk.open().expect("open");
        let mut batch = storage.batch().expect("batch");
        batch.put(A, b"k", b"consistent").expect("put");
        batch.commit(Durability::Consistent).expect("commit");
        storage.fail_next(OpClass::Read);
        let snapshot = storage.snapshot().expect("snapshot");
        assert!(snapshot.get(A, b"k", |_| ()).is_err());
        drop(storage);
        let storage = disk.open().expect("reopen");
        let snapshot = storage.snapshot().expect("snapshot");
        assert_eq!(get(&snapshot, A, b"k"), None);
    }

    #[test]
    fn a_commit_copies_only_what_it_writes() {
        let disk = MemoryDisk::new();
        let storage = disk.open().expect("open");
        let mut batch = storage.batch().expect("batch");
        batch.put(A, b"a", b"1").expect("put");
        batch.put(B, b"b", b"1").expect("put");
        batch
            .put_fragment(frag(1, 0), b"chunk 1")
            .expect("put fragment");
        batch.commit(Durability::Durable).expect("commit");
        let before = storage.handle.lock().visible.clone();

        let mut batch = storage.batch().expect("batch");
        batch.put(B, b"b", b"2").expect("put");
        batch
            .put_fragment(frag(1, 1), b"more of chunk 1")
            .expect("put fragment");
        batch
            .put_fragment(frag(2, 0), b"chunk 2")
            .expect("put fragment");
        batch.commit(Durability::Durable).expect("commit");
        let after = storage.handle.lock().visible.clone();

        assert!(Arc::ptr_eq(&before.namespaces[&A], &after.namespaces[&A]));
        assert!(!Arc::ptr_eq(&before.namespaces[&B], &after.namespaces[&B]));
        // Chunk 1 gained a fragment, so its map was copied, but its first fragment's
        // bytes were not.
        assert!(!Arc::ptr_eq(
            &before.chunks[&ChunkId(1)],
            &after.chunks[&ChunkId(1)]
        ));
        assert!(Arc::ptr_eq(
            &before.chunks[&ChunkId(1)][&0],
            &after.chunks[&ChunkId(1)][&0]
        ));

        let mut batch = storage.batch().expect("batch");
        batch.put(B, b"c", b"3").expect("put");
        batch.commit(Durability::Durable).expect("commit");
        let last = storage.handle.lock().visible.clone();
        assert!(Arc::ptr_eq(
            &after.chunks[&ChunkId(1)],
            &last.chunks[&ChunkId(1)]
        ));
        assert!(Arc::ptr_eq(
            &after.chunks[&ChunkId(2)],
            &last.chunks[&ChunkId(2)]
        ));
    }
}
