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

/// Per-instance boundary hooks and injected faults, all local to one fixture.
#[derive(Default)]
pub(crate) struct Probes {
    /// Exhaustive recorder, inactive unless a verification fixture explicitly starts it.
    pub(crate) schedule: crate::verification::faults::FaultSchedule,
    /// Before explicitly requested compaction, with the facade's capture gate held.
    pub(super) compact: Hook,
    /// Next compaction returns this error without poisoning the underlying test engine.
    pub(super) compact_fault: Mutex<Option<StorageError>>,
    /// Durability requested at each commit, including attempts that fail.
    pub(super) durabilities: Mutex<Vec<Durability>>,
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
pub(crate) struct Observed<S> {
    /// Taken by Drop so the close hook precisely precedes backend destruction.
    storage: Option<S>,
    /// Explicitly injected test observations.
    probes: Arc<Probes>,
}

impl<S> Observed<S> {
    /// Arms the existing backend conformance failures without cloning backend ownership.
    pub(crate) fn backend(&self) -> &S {
        self.storage.as_ref().unwrap()
    }

    /// Wraps an existing engine without changing its transaction semantics.
    pub(crate) fn new(storage: S, probes: Arc<Probes>) -> Self {
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

    fn fault_schedule(&self) -> Option<crate::verification::faults::FaultSchedule> {
        Some(self.probes.schedule.clone())
    }

    fn snapshot(&self) -> Result<Self::Snapshot> {
        self.probes.schedule.step("port.snapshot")?;
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
        self.probes.schedule.step("port.batch")?;
        self.probes.batch.run();
        Ok(Batch {
            inner: self.storage.as_ref().unwrap().batch()?,
            probes: &self.probes,
        })
    }
}

impl<S: crate::CompactStorage> crate::CompactStorage for Observed<S> {
    fn compact(&mut self) -> Result<crate::Compaction> {
        self.probes.schedule.step("port.compact")?;
        self.probes.compact.run();
        if let Some(error) = self.probes.compact_fault.lock().unwrap().take() {
            return Err(error);
        }
        self.storage.as_mut().unwrap().compact()
    }
}

/// A snapshot that can report a fault independently of the engine's own failed state.
pub(crate) struct Read<R> {
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
    fn fault_schedule(&self) -> Option<crate::verification::faults::FaultSchedule> {
        Some(self.probes.schedule.clone())
    }

    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        self.admit()?;
        observed_get(self.inner.as_ref().unwrap(), &self.probes, ns, key, f)
    }
    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        self.admit()?;
        observed_scan(
            self.inner.as_ref().unwrap(),
            &self.probes,
            ns,
            range,
            direction,
            f,
        )
    }
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        self.admit()?;
        observed_fragment(self.inner.as_ref().unwrap(), &self.probes, id, limit, f)
    }
}

/// A write transaction whose commit boundary can be ordered against a reader's report.
pub(crate) struct Batch<'a, B> {
    /// The actual transaction, with the engine's ordinary rollback-on-drop behavior.
    inner: B,
    /// The owner's observations; no callback holds their mutex.
    probes: &'a Probes,
}

impl<B: StorageRead> StorageRead for Batch<'_, B> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T> {
        observed_get(&self.inner, self.probes, ns, key, f)
    }
    fn scan<T>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<T>,
    ) -> Result<Option<T>> {
        observed_scan(&self.inner, self.probes, ns, range, direction, f)
    }
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T> {
        observed_fragment(&self.inner, self.probes, id, limit, f)
    }
}

impl<B: StorageWrite> StorageWrite for Batch<'_, B> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()> {
        self.probes
            .schedule
            .step(if ns == crate::layout::DIAGNOSTICS {
                "port.put.diagnostics"
            } else {
                "port.put"
            })?;
        self.inner.put(ns, key, value)
    }
    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()> {
        self.probes.schedule.step("port.delete")?;
        self.inner.delete(ns, key)
    }
    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()> {
        self.probes.schedule.step("port.put_fragment")?;
        self.inner.put_fragment(id, bytes)
    }
    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()> {
        self.probes.schedule.step("port.delete_chunk")?;
        self.inner.delete_chunk(chunk)
    }
    fn commit(self, durability: Durability) -> Result<(), CommitFailure> {
        self.probes.durabilities.lock().unwrap().push(durability);
        self.probes.commit.run();
        self.probes
            .schedule
            .step("port.commit.before")
            .map_err(|source| CommitFailure::NotApplied { source })?;
        self.inner.commit(durability)?;
        self.probes
            .schedule
            .step("port.commit.after")
            .map_err(|source| CommitFailure::Unknown { source })
    }
}

/// Reads and the callback's completion are separately injectable boundaries.
fn observed_get<R: StorageRead, T>(
    read: &R,
    probes: &Probes,
    ns: Ns,
    key: &[u8],
    f: impl FnOnce(Option<&[u8]>) -> T,
) -> Result<T> {
    probes.schedule.step("port.get")?;
    read.get(ns, key, |value| {
        probes.schedule.step("port.get.callback")?;
        let result = f(value);
        probes.schedule.step("port.get.return")?;
        Ok(result)
    })?
}

/// Stops the scan on an injected callback failure, preserving ordinary early termination.
fn observed_scan<R: StorageRead, T>(
    read: &R,
    probes: &Probes,
    ns: Ns,
    range: KeyRange<'_>,
    direction: Direction,
    mut f: impl FnMut(&[u8], &[u8]) -> ControlFlow<T>,
) -> Result<Option<T>> {
    probes.schedule.step("port.scan")?;
    read.scan(ns, range, direction, |key, value| {
        if let Err(error) = probes.schedule.step("port.scan.callback") {
            return ControlFlow::Break(Err(error));
        }
        let result = f(key, value);
        if let Err(error) = probes.schedule.step("port.scan.return") {
            return ControlFlow::Break(Err(error));
        }
        result.map_break(Ok)
    })?
    .transpose()
}

/// Fragments are lent by the actual engine, with failures on both sides of the callback.
fn observed_fragment<R: StorageRead, T>(
    read: &R,
    probes: &Probes,
    id: FragmentId,
    limit: usize,
    f: impl FnOnce(Option<&[u8]>) -> T,
) -> Result<T> {
    probes.schedule.step("port.fragment")?;
    read.fragment(id, limit, |value| {
        probes.schedule.step("port.fragment.callback")?;
        let result = f(value);
        probes.schedule.step("port.fragment.return")?;
        Ok(result)
    })?
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
