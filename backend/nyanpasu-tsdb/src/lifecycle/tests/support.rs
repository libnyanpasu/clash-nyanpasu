//! Per-instance port probes; callbacks run after the probe mutex has been released.

use crate::storage::{
    ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, Ns, Result, Storage,
    StorageError, StorageRead, StorageWrite,
};
use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

/// One explicitly armed observation of a synchronous storage boundary.
#[derive(Default)]
pub(super) struct Hook(Mutex<Option<Box<dyn FnOnce() + Send>>>);

impl Hook {
    /// Replaces the callback before the operation under test starts.
    pub(super) fn arm(&self, run: impl FnOnce() + Send + 'static) {
        *self.0.lock().unwrap() = Some(Box::new(run));
    }

    /// Removes the callback before invoking it, so it can probe all product locks.
    fn run(&self) {
        let run = self.0.lock().unwrap().take();
        if let Some(run) = run {
            run();
        }
    }
}

/// Independent hooks and one read fault; neither survives beyond this fixture.
#[derive(Default)]
pub(super) struct Probes {
    /// Before the port commit, including its durable work.
    pub(super) commit: Hook,
    /// Before backend Drop, after the facade has detached it.
    pub(super) close: Hook,
    /// After the underlying read transaction drops, before the facade returns its capture credit.
    pub(super) snapshot_dropped: Hook,
    /// Before any batch is opened, to detect an admission that should have been refused.
    pub(super) batch: Hook,
    /// Before opening a storage snapshot, used to select the private restoration snapshot.
    pub(super) snapshot: Hook,
    /// Next snapshot opening fails before creating a read transaction.
    pub(super) snapshot_fault: Mutex<Option<StorageError>>,
    /// After choosing a read's result, before returning it to the facade.
    pub(super) read: Hook,
    /// Next snapshot read returns this error without poisoning the real engine.
    pub(super) fault: Mutex<Option<StorageError>>,
}

/// A backend owned only by the facade; probes do not own it.
pub(super) struct Observed<S> {
    /// Taken by Drop so the close hook precisely precedes backend destruction.
    storage: Option<S>,
    /// Explicitly injected test observations.
    probes: Arc<Probes>,
}

impl<S> Observed<S> {
    /// Wraps an existing engine without changing its transaction semantics.
    pub(super) fn new(storage: S, probes: Arc<Probes>) -> Self {
        Self {
            storage: Some(storage),
            probes,
        }
    }
}

impl<S> Drop for Observed<S> {
    fn drop(&mut self) {
        self.probes.close.run();
        drop(self.storage.take());
    }
}

impl<S: Storage> Storage for Observed<S> {
    type Snapshot = Read<S::Snapshot>;
    type Batch<'a>
        = Batch<'a, S::Batch<'a>>
    where
        Self: 'a;

    fn snapshot(&self) -> Result<Self::Snapshot> {
        self.probes.snapshot.run();
        if let Some(error) = self.probes.snapshot_fault.lock().unwrap().take() {
            return Err(error);
        }
        Ok(Read {
            inner: Some(self.storage.as_ref().unwrap().snapshot()?),
            probes: self.probes.clone(),
        })
    }

    fn batch(&self) -> Result<Self::Batch<'_>> {
        self.probes.batch.run();
        Ok(Batch {
            inner: self.storage.as_ref().unwrap().batch()?,
            probes: &self.probes,
        })
    }
}

/// A snapshot that can report a fault independently of the engine's own failed state.
pub(super) struct Read<R> {
    /// Dropped before the transaction-drop observation runs.
    inner: Option<R>,
    /// Shared test hooks, independent of the facade's query budget.
    probes: Arc<Probes>,
}

impl<R> Read<R> {
    /// Consumes the next injected read failure.
    fn admit(&self) -> Result<()> {
        let result = match self.probes.fault.lock().unwrap().take() {
            Some(error) => Err(error),
            None => Ok(()),
        };
        self.probes.read.run();
        result
    }
}

impl<R> Drop for Read<R> {
    fn drop(&mut self) {
        drop(self.inner.take());
        self.probes.snapshot_dropped.run();
    }
}

impl<R: StorageRead> StorageRead for Read<R> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.admit()?;
        self.inner.as_ref().unwrap().get(ns, key, f)
    }
    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.admit()?;
        self.inner.as_ref().unwrap().scan(ns, range, direction, f)
    }
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.admit()?;
        self.inner.as_ref().unwrap().fragment(id, limit, f)
    }
}

/// A write transaction whose commit boundary can be ordered against a reader's report.
pub(super) struct Batch<'a, B> {
    /// The actual transaction, with the engine's ordinary rollback-on-drop behavior.
    inner: B,
    /// The owner's observations; no callback holds their mutex.
    probes: &'a Probes,
}

impl<B: StorageRead> StorageRead for Batch<'_, B> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.inner.get(ns, key, f)
    }
    fn scan<T>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<T>,
    ) -> Result<Option<T>> {
        self.inner.scan(ns, range, direction, f)
    }
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.inner.fragment(id, limit, f)
    }
}

impl<B: StorageWrite> StorageWrite for Batch<'_, B> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.put(ns, key, value)
    }
    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()> {
        self.inner.delete(ns, key)
    }
    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()> {
        self.inner.put_fragment(id, bytes)
    }
    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()> {
        self.inner.delete_chunk(chunk)
    }
    fn commit(self, durability: Durability) -> Result<(), CommitFailure> {
        self.probes.commit.run();
        self.inner.commit(durability)
    }
}

/// redb's real sync path; clones share its disk so recovery can open a fresh database.
#[cfg(feature = "redb")]
#[derive(Clone)]
pub(super) struct SyncDisk {
    /// The ordinary redb memory backend, with redb still driving every sync call.
    inner: Arc<redb::backends::InMemoryBackend>,
    /// One explicitly armed sync observation.
    hook: Arc<Hook>,
}

#[cfg(feature = "redb")]
impl SyncDisk {
    /// Starts an empty disk whose sync boundary is externally observable.
    pub(super) fn new(hook: Arc<Hook>) -> Self {
        Self {
            inner: Arc::new(redb::backends::InMemoryBackend::new()),
            hook,
        }
    }
}

#[cfg(feature = "redb")]
impl std::fmt::Debug for SyncDisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncDisk").finish_non_exhaustive()
    }
}

#[cfg(feature = "redb")]
impl redb::StorageBackend for SyncDisk {
    fn len(&self) -> std::io::Result<u64> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, len: u64) -> std::io::Result<()> {
        self.inner.set_len(len)
    }
    fn sync_data(&self) -> std::io::Result<()> {
        self.hook.run();
        self.inner.sync_data()
    }
    fn write(&self, offset: u64, data: &[u8]) -> std::io::Result<()> {
        self.inner.write(offset, data)
    }
    fn close(&self) -> std::io::Result<()> {
        self.inner.close()
    }
}
