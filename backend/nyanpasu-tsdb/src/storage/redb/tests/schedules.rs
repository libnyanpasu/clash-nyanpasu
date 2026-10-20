//! The operation recorder beneath redb, including applied-then-error sync and close I/O.

use std::{collections::BTreeSet, time::Instant};

use super::*;
use crate::verification::{
    Dataset,
    faults::{FaultSchedule, Injection},
};

/// Exact physical contents, including fragments with no manifest references.
fn dump(storage: &RedbStorage) -> Dataset {
    let snapshot = storage.snapshot().unwrap();
    let mut dataset = Dataset {
        entries: crate::db::tests::dump(&snapshot),
        fragments: Vec::new(),
    };
    if let Some(table) = snapshot.fragments().unwrap() {
        for entry in table.iter().unwrap() {
            let (key, value) = entry.unwrap();
            let (chunk, index) = key.value();
            dataset.fragments.push((
                FragmentId {
                    chunk: ChunkId(chunk),
                    index,
                },
                value.value().to_vec(),
            ));
        }
    }
    dataset
}

/// One physical operation; the same controller is used by the port schedules.
#[derive(Clone, Copy, Debug)]
enum Operation {
    /// Opens the existing durable image, including any redb repair reads.
    Open,
    /// Reads both namespaces and the fragment through an uncached snapshot.
    Read,
    /// Commits an entire new generation durably.
    Write,
    /// Commits a generation that a crash must lose.
    Consistent,
    /// Promotes the pending generation with a Durable commit.
    Checkpoint,
    /// Runs real allocator/header and backend close I/O.
    Close,
    /// Moves live pages out of a small sparse database.
    Compact,
    /// Abandons an allocation that grew the file and must truncate.
    Rollback,
}

/// The disk is invalidated before any Drop used to obtain an oracle image.
fn durable(disk: &FaultDisk) -> Dataset {
    let fork = disk.fork();
    let storage = open(&fork, 0);
    let result = dump(&storage);
    fork.crash();
    drop(storage);
    result
}

/// The existing conformance fixture crosses two namespaces and fragments atomically.
fn base(operation: Operation) -> FaultDisk {
    let disk = FaultDisk::default();
    let storage = open(&disk, 0);
    if matches!(operation, Operation::Compact) {
        let mut batch = storage.batch().unwrap();
        batch.put(A, b"hole", &[0x5a; 4096]).unwrap();
        batch.put(A, b"kept", b"value").unwrap();
        batch.commit(Durability::Durable).unwrap();
        let mut batch = storage.batch().unwrap();
        batch.delete(A, b"hole").unwrap();
        batch.commit(Durability::Durable).unwrap();
    } else {
        generation(&storage, 1, Durability::Durable).unwrap();
    }
    drop(storage);
    disk
}

/// A fresh real redb instance over a copied durable disk, with zero cache for read coverage.
struct Fixture {
    /// An independent durable byte image for this attempt.
    disk: FaultDisk,
    /// The same ordinal controller used by the facade fixtures.
    schedule: FaultSchedule,
    /// The only live database; absent before open and after close.
    store: Option<RedbStorage>,
}

impl Fixture {
    /// Opening the common precondition is outside the operation's trace.
    fn new(base: &FaultDisk, operation: Operation) -> Self {
        let disk = base.fork();
        let schedule = disk.schedule();
        let store = if matches!(operation, Operation::Open) {
            None
        } else {
            Some(open(&disk, 0))
        };
        if matches!(operation, Operation::Checkpoint | Operation::Close) {
            generation(store.as_ref().unwrap(), 2, Durability::Consistent).unwrap();
        }
        Self {
            disk,
            schedule,
            store,
        }
    }

    /// Disk errors propagate through the real redb adapter and its ordinary classification.
    fn run(&mut self, operation: Operation) -> Result<(), ErrorKind> {
        if matches!(operation, Operation::Open) {
            let backend = RedbStorage::open_with_backend(self.disk.backend(), 0)
                .map_err(|error| error.kind())?;
            self.store = Some(backend);
            return Ok(());
        }
        if matches!(operation, Operation::Close) {
            drop(self.store.take());
            return Ok(());
        }
        let storage = self.store.as_mut().unwrap();
        match operation {
            Operation::Read => {
                let snapshot = storage.snapshot().map_err(|error| error.kind())?;
                snapshot
                    .scan(A, KeyRange::All, Direction::Forward, |_, _| {
                        ControlFlow::<()>::Continue(())
                    })
                    .map_err(|error| error.kind())?;
                snapshot
                    .get(B, b"generation", |_| ())
                    .map_err(|error| error.kind())?;
                snapshot
                    .fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())
                    .map_err(|error| error.kind())?;
            }
            Operation::Write | Operation::Consistent => generation(
                storage,
                3,
                if matches!(operation, Operation::Consistent) {
                    Durability::Consistent
                } else {
                    Durability::Durable
                },
            )?,
            Operation::Checkpoint => generation(storage, 2, Durability::Durable)?,
            Operation::Compact => {
                assert_eq!(
                    storage.compact().map_err(|error| error.kind())?,
                    Compaction::Compacted
                );
            }
            Operation::Rollback => {
                let mut batch = storage.batch().map_err(|error| error.kind())?;
                let payload = vec![0xa5; MAX_FRAGMENT_LEN];
                for index in 0..4 {
                    batch
                        .put_fragment(frag(5_000, index), &payload)
                        .map_err(|error| error.kind())?;
                }
                drop(batch);
            }
            Operation::Open | Operation::Close => unreachable!(),
        }
        Ok(())
    }

    /// A reported backend failure remains latched on both old and newly attempted operations.
    fn check_latched(&self) {
        let Some(storage) = self.store.as_ref() else {
            return;
        };
        for _ in 0..2 {
            assert!(matches!(
                storage.snapshot(),
                Err(StorageError::Failed { .. })
            ));
            assert!(matches!(storage.batch(), Err(StorageError::Failed { .. })));
        }
    }

    /// Capture the durable image before teardown and require the same exact bytes on reopen.
    fn finish(mut self) {
        let expected = durable(&self.disk);
        self.disk.crash();
        drop(self.store.take());
        let storage = open(&self.disk, 0);
        assert_eq!(dump(&storage), expected);
        self.disk.crash();
        drop(storage);
    }
}

/// No sampled I/O: every recorded ordinal receives a failure and a separate crash.
#[test]
fn every_redb_io_is_faulted_and_crashed() {
    let mut reached = BTreeSet::new();
    for operation in [
        Operation::Open,
        Operation::Read,
        Operation::Write,
        Operation::Consistent,
        Operation::Checkpoint,
        Operation::Close,
        Operation::Compact,
        Operation::Rollback,
    ] {
        let started = Instant::now();
        let base = base(operation);
        let mut success = Fixture::new(&base, operation);
        let before = durable(&success.disk);
        success.schedule.start(None);
        success.run(operation).unwrap();
        let trace = success.schedule.finish();
        assert!(!trace.is_empty(), "{operation:?} made no I/O");
        let after = durable(&success.disk);
        success.finish();
        for (ordinal, name) in trace.iter().enumerate() {
            for injection in [Injection::Fault, Injection::Crash] {
                let mut fixture = Fixture::new(&base, operation);
                fixture.schedule.start_with_tail((ordinal, injection));
                let result = fixture.run(operation);
                let actual_trace = fixture.schedule.finish();
                assert_eq!(
                    actual_trace[..=ordinal],
                    trace[..=ordinal],
                    "{operation:?} {ordinal} {name} {injection:?}"
                );
                // These operations perform the scheduled I/O in a destructor, not a Result.
                let destructor = matches!(operation, Operation::Close | Operation::Rollback);
                // redb 4.4.0 cached_file.rs:724 absorbs a failed best-effort eviction write:
                // the page stays buffered and the commit flushes it later. Such a write must
                // still end in the committed image, checked below, whichever ordinal it was.
                let absorbed = !destructor
                    && *name == "disk.write"
                    && injection == Injection::Fault
                    && result.is_ok();
                if destructor {
                    assert!(
                        matches!(result, Ok(()) | Err(ErrorKind::BackendFailed)),
                        "{operation:?} {ordinal} {name} {injection:?}: {result:?}"
                    );
                } else if !absorbed {
                    assert_eq!(
                        result,
                        Err(ErrorKind::BackendFailed),
                        "{operation:?} {ordinal} {name} {injection:?} swallowed I/O failure"
                    );
                }
                let actual = durable(&fixture.disk);
                assert!(
                    actual == before || actual == after,
                    "{operation:?} {ordinal} {name} {injection:?} tore data"
                );
                if !actual_trace.contains(&"disk.sync.after") {
                    assert_eq!(
                        actual, before,
                        "{operation:?} {ordinal} {name} {injection:?} advanced before sync"
                    );
                }
                if !absorbed {
                    fixture.check_latched();
                } else {
                    assert!(actual_trace.contains(&"disk.sync.after"));
                    assert_eq!(actual, after, "absorbed {operation:?} {ordinal} {name}");
                    assert!(fixture.store.as_ref().unwrap().snapshot().is_ok());
                    assert!(fixture.store.as_ref().unwrap().batch().is_ok());
                }
                fixture.finish();
            }
        }
        eprintln!(
            "E9 disk {operation:?}: {} ordinals, {} replays, {:?}",
            trace.len(),
            2 * trace.len(),
            started.elapsed()
        );
        reached.extend(trace);
    }
    for name in [
        "disk.len",
        "disk.read",
        "disk.write",
        "disk.set_len",
        "disk.sync.before",
        "disk.sync.after",
        "disk.close",
    ] {
        assert!(reached.contains(name), "unreached boundary {name}");
    }
}

/// Reuses the conformance suite's atomic generation without swallowing an injected error.
fn generation(storage: &RedbStorage, n: u64, durability: Durability) -> Result<(), ErrorKind> {
    let mut batch = storage.batch().map_err(|error| error.kind())?;
    conformance::stage_generation(&mut batch, n).map_err(|error| error.kind())?;
    batch.commit(durability).map_err(|error| error.kind())
}

/// Real file locking, with only the synchronous close boundaries observed by the recorder.
#[derive(Debug)]
struct SyncFile {
    /// redb still owns all actual file I/O and range locks.
    inner: redb::backends::FileBackend,
    /// One recorder provides both the sync ordinals and the barrier observation.
    schedule: FaultSchedule,
}

impl redb::StorageBackend for SyncFile {
    fn len(&self) -> std::io::Result<u64> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, len: u64) -> std::io::Result<()> {
        self.inner.set_len(len)
    }
    fn write(&self, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(offset, bytes)
    }
    fn sync_data(&self) -> std::io::Result<()> {
        self.schedule.io("close.sync")?;
        self.inner.sync_data()
    }
    fn close(&self) -> std::io::Result<()> {
        self.inner.close()
    }
    fn try_lock_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<bool, redb::BackendError> {
        self.inner.try_lock_range(start, end)
    }
    fn try_lock_shared_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<bool, redb::BackendError> {
        self.inner.try_lock_shared_range(start, end)
    }
    fn lock_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<(), redb::BackendError> {
        self.inner.lock_range(start, end)
    }
    fn lock_shared_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<(), redb::BackendError> {
        self.inner.lock_shared_range(start, end)
    }
    fn unlock_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<(), redb::BackendError> {
        self.inner.unlock_range(start, end)
    }
    fn query_lock_range(
        &self,
        start: std::ops::Bound<u64>,
        end: std::ops::Bound<u64>,
    ) -> Result<bool, redb::BackendError> {
        self.inner.query_lock_range(start, end)
    }
}

/// A fresh real file with a pending Consistent commit for allocator and shutdown-header work.
fn closing_file() -> (
    tempfile::NamedTempFile,
    crate::Tsdb<RedbStorage>,
    FaultSchedule,
) {
    let file = tempfile::NamedTempFile::new().unwrap();
    let schedule = FaultSchedule::default();
    let backend = SyncFile {
        inner: redb::backends::FileBackend::new(file.reopen().unwrap()).unwrap(),
        schedule: schedule.clone(),
    };
    let mut db = crate::Tsdb::open(
        RedbStorage::open_with_backend(backend, 0).unwrap(),
        crate::verification::schema(),
        crate::verification::profile(false),
    )
    .unwrap();
    crate::verification::write(&mut db, 1, 0, Durability::Consistent).unwrap();
    (file, db, schedule)
}

/// Every close sync retains the file lock, while the facade has already released G and R.
#[test]
fn close_retains_the_file_through_allocator_and_header_syncs() {
    use std::sync::{Arc, Barrier, mpsc};
    let (_file, db, schedule) = closing_file();
    schedule.start(None);
    drop(db);
    let trace = schedule.finish();
    assert!(
        trace.len() >= 2,
        "allocator and shutdown header did not both sync"
    );
    assert!(trace.iter().all(|name| *name == "close.sync"));
    for ordinal in 0..trace.len() {
        let (file, db, schedule) = closing_file();
        let shared = db.shared().clone();
        let reader = db.reader();
        let (arrived, progress) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let parked = arrived.clone();
        let released = release.clone();
        schedule.start(None);
        schedule.observe(ordinal, move || {
            parked.send("parked").unwrap();
            released.wait();
        });
        // A panicking close still reports "finished", so the main thread never waits forever.
        let worker = std::thread::spawn(move || {
            let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(db)));
            arrived.send("finished").unwrap();
            if let Err(panic) = closed {
                std::panic::resume_unwind(panic);
            }
        });
        if progress.recv().unwrap() == "finished" {
            worker.join().unwrap();
            panic!("close finished before sync observation {ordinal}");
        }
        // Record every probe before releasing, but assert only after joining the parked worker.
        let locks_free = shared.gate_available() && shared.registry.publication_available();
        let peer = file
            .reopen()
            .map_err(DatabaseError::from)
            .and_then(redb::backends::FileBackend::new);
        let lock = peer.as_ref().ok().map(|peer| {
            peer.try_lock_range(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
        });
        let refused = matches!(lock, Some(Ok(false)));
        let released_peer = peer.as_ref().ok().map(redb::StorageBackend::close);
        drop(peer);
        release.wait();
        worker.join().unwrap();
        assert!(locks_free, "shared lock held at close sync {ordinal}");
        assert!(
            refused,
            "file lock released before close sync {ordinal}: {lock:?}"
        );
        assert!(
            matches!(released_peer, Some(Ok(()))),
            "probe close: {released_peer:?}"
        );
        assert_eq!(schedule.finish(), trace);
        assert!(matches!(
            reader.capture(),
            Err(crate::CaptureError::CheckOpen { .. })
        ));
        let reopened = RedbStorage::open(file.path(), 0).unwrap();
        let state = crate::layout::meta::read_state(&reopened.snapshot().unwrap()).unwrap();
        assert_eq!(state.last_batch, Some(1));
    }
    eprintln!("E9 real-file close: {} ordered sync probes", trace.len());
}
