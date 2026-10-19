mod model;
mod schedules;

use std::{fs, ops::ControlFlow, panic::Location};

use redb::ReadableDatabase;

use super::{disk::FaultDisk, *};
use crate::{
    Profile,
    storage::conformance::{self, CommitOutcome, Harness, OpClass},
};

const A: Ns = Ns(1);
const B: Ns = Ns(2);
/// A page cache larger than any test database.
const CACHE: u64 = 4 << 20;

/// The conformance suite's view of the backend, on a [`FaultDisk`].
///
/// Its storage has no page cache, so every page an operation reads comes from the disk,
/// unless the batch itself has it dirty. A failure armed for the next I/O therefore hits the
/// operation that comes next, whatever its class. Snapshots still keep their tables' root
/// pages, which the instance state covers (see the module docs).
struct RedbHarness {
    disk: FaultDisk,
    storage: Option<RedbStorage>,
}

fn open(disk: &FaultDisk, cache_bytes: u64) -> RedbStorage {
    RedbStorage::open_with_backend(disk.backend(), cache_bytes).expect("open the disk")
}

impl Harness for RedbHarness {
    type Storage = RedbStorage;

    fn fresh() -> Self {
        let disk = FaultDisk::default();
        let storage = open(&disk, 0);
        Self {
            disk,
            storage: Some(storage),
        }
    }

    fn storage(&self) -> &RedbStorage {
        self.storage
            .as_ref()
            .expect("the harness keeps a storage open")
    }

    fn reopen(&mut self) {
        self.storage = None;
        self.disk.assert_fault_fired();
        self.storage = Some(open(&self.disk, 0));
    }

    fn crash_and_reopen(&mut self) {
        self.disk.crash();
        self.storage = None;
        self.disk.assert_fault_fired();
        self.storage = Some(open(&self.disk, 0));
    }

    /// Fails the next disk call, whichever kind of call the operation of `class` makes first.
    fn fail_next(&self, _class: OpClass) {
        self.disk.fail_next_io();
    }

    fn inject_commit(&self, outcome: CommitOutcome) {
        match outcome {
            CommitOutcome::NotApplied => self.storage().poison_next_commit(),
            CommitOutcome::Unknown { applied } => self.disk.fail_commit_sync(applied),
        }
    }

    fn corrupt_entry(&self, ns: Ns, key: &[u8]) {
        self.storage().corrupt_entry(ns, key);
    }

    fn corrupt_fragment(&self, id: FragmentId) {
        self.storage().corrupt_fragment(id);
    }
}

conformance::conformance_tests!(RedbHarness);

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

fn fragment(read: &impl StorageRead, id: FragmentId) -> Option<Vec<u8>> {
    read.fragment(id, MAX_FRAGMENT_LEN, |bytes| bytes.map(<[u8]>::to_vec))
        .expect("read a fragment")
}

fn keys(read: &impl StorageRead, ns: Ns) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    read.scan(ns, KeyRange::All, Direction::Forward, |key, _| {
        keys.push(key.to_vec());
        ControlFlow::<()>::Continue(())
    })
    .expect("scan");
    keys
}

fn commit<'s>(
    storage: &'s RedbStorage,
    durability: Durability,
    write: impl FnOnce(&mut RedbBatch<'s>),
) {
    let mut batch = storage.batch().expect("open a batch");
    write(&mut batch);
    batch.commit(durability).expect("commit");
}

fn failure<T>(result: Result<T>) -> StorageError {
    match result {
        Ok(_) => panic!("the operation succeeded"),
        Err(error) => error,
    }
}

fn temporary_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("tsdb.redb");
    (dir, path)
}

#[test]
fn a_file_round_trips_through_a_clean_close() {
    let (_dir, path) = temporary_file();
    let storage = RedbStorage::open(&path, CACHE).expect("create the file");
    // Only the clean close makes a consistent commit durable.
    commit(&storage, Durability::Consistent, |batch| {
        batch.put(A, b"k", b"v").unwrap();
        batch.put_fragment(frag(3, 1), b"fragment").unwrap();
    });
    drop(storage);

    let storage = RedbStorage::open(&path, CACHE).expect("reopen the file");
    let view = storage.snapshot().unwrap();
    assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
    assert_eq!(fragment(&view, frag(3, 1)), Some(b"fragment".to_vec()));
}

#[test]
fn a_second_open_fails_while_the_first_is_open() {
    let (_dir, path) = temporary_file();
    let first = RedbStorage::open(&path, CACHE).expect("first open");
    let Err(error) = RedbStorage::open(&path, CACHE) else {
        panic!("a second open succeeded");
    };
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    let StorageError::Failed { source } = &error else {
        panic!("unexpected error {error:?}");
    };
    assert!(
        matches!(
            source.downcast_ref::<RedbError>(),
            Some(RedbError::Open {
                source: DatabaseError::DatabaseAlreadyOpen
            })
        ),
        "{source:?}"
    );
    drop(first);
    RedbStorage::open(&path, CACHE).expect("open after the first closed");
}

/// redb reports no cache size back without its `cache_metrics` feature, so this shows the
/// argument's effect: with no cache, every snapshot reads its tables' root pages from the
/// disk again; with a cache, no snapshot after the first reads the disk.
#[test]
fn the_cache_size_is_applied() {
    let disk_reads = |cache_bytes| {
        let disk = FaultDisk::default();
        let storage = open(&disk, cache_bytes);
        commit(&storage, Durability::Durable, |batch| {
            batch.put(A, b"k", b"v").unwrap();
        });
        let read = || {
            let view = storage.snapshot().unwrap();
            assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
        };
        read();
        let start = disk.stats().reads;
        read();
        let once = disk.stats().reads - start;
        for _ in 1..10 {
            read();
        }
        (once, disk.stats().reads - start)
    };

    let (once, ten_times) = disk_reads(0);
    assert_ne!(once, 0, "an uncached snapshot read nothing from the disk");
    assert_eq!(ten_times, 10 * once);
    assert_eq!(disk_reads(CACHE), (0, 0));
}

/// A snapshot whose pages redb serves from memory: its tables' roots, and the cache.
fn cached_snapshot(disk: &FaultDisk, storage: &RedbStorage) -> RedbSnapshot {
    commit(storage, Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
        batch.put_fragment(frag(1, 0), b"f").unwrap();
    });
    let view = storage.snapshot().unwrap();
    assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
    assert_eq!(fragment(&view, frag(1, 0)), Some(b"f".to_vec()));
    let reads = disk.stats().reads;
    assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
    assert_eq!(fragment(&view, frag(1, 0)), Some(b"f".to_vec()));
    assert_eq!(
        disk.stats().reads,
        reads,
        "the reads are served from memory"
    );
    view
}

#[test]
fn a_failure_ends_reads_redb_serves_from_memory() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let view = cached_snapshot(&disk, &storage);

    let mut batch = storage.batch().unwrap();
    batch.put(B, b"k", b"v").unwrap();
    disk.fail_next_io();
    let outcome = batch.commit(Durability::Durable);
    assert!(
        matches!(outcome, Err(CommitFailure::Unknown { .. })),
        "{outcome:?}"
    );
    disk.assert_fault_fired();

    let errors = [
        failure(view.get(A, b"k", |_| ())),
        failure(view.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())),
    ];
    for error in errors {
        assert_eq!(error.kind(), ErrorKind::BackendFailed, "{error:?}");
    }
}

#[test]
fn a_close_ends_reads_redb_serves_from_memory() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let view = cached_snapshot(&disk, &storage);
    drop(storage);

    let errors = [
        failure(view.get(A, b"k", |_| ())),
        failure(view.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())),
    ];
    for error in errors {
        assert!(matches!(error, StorageError::Closed), "{error:?}");
    }
}

/// redb's rollback of an abandoned batch truncates what the batch grew the file by. When
/// that fails, redb latches itself, and the instance has to as well, or a snapshot would go
/// on reading the pages redb serves from memory.
#[test]
fn a_failed_rollback_of_an_abandoned_batch_latches_the_instance() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let view = cached_snapshot(&disk, &storage);

    let mut batch = storage.batch().unwrap();
    let before = disk.len();
    let bytes = vec![0xA5; MAX_FRAGMENT_LEN];
    for index in 0..8 {
        batch.put_fragment(frag(2, index), &bytes).unwrap();
    }
    assert!(disk.len() > before, "the batch did not grow the file");
    disk.fail_truncate();
    drop(batch);
    disk.assert_fault_fired();

    let errors = [
        failure(view.get(A, b"k", |_| ())),
        failure(view.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())),
        failure(storage.snapshot()),
        failure(storage.batch()),
    ];
    for error in errors {
        assert_eq!(error.kind(), ErrorKind::BackendFailed, "{error:?}");
    }
}

/// Without a fault, an abandoned batch rolls back and gives back what it grew the file by.
#[test]
fn an_abandoned_batch_rolls_back_its_growth() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let view = cached_snapshot(&disk, &storage);
    let before = disk.len();

    let mut batch = storage.batch().unwrap();
    for index in 0..8 {
        batch
            .put_fragment(frag(2, index), &vec![0xA5; MAX_FRAGMENT_LEN])
            .unwrap();
    }
    drop(batch);
    assert_eq!(disk.len(), before);
    assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
    assert_eq!(fragment(&storage.snapshot().unwrap(), frag(2, 0)), None);
}

#[test]
fn reading_or_deleting_creates_no_table() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let unused = Ns(200);

    let mut batch = storage.batch().unwrap();
    assert_eq!(get(&batch, unused, b"k"), None);
    assert_eq!(keys(&batch, unused), Vec::<Vec<u8>>::new());
    assert_eq!(fragment(&batch, frag(1, 0)), None);
    batch.delete(unused, b"k").unwrap();
    batch.delete_chunk(ChunkId(1)).unwrap();
    batch.put(A, b"k", b"v").unwrap();
    batch.commit(Durability::Durable).unwrap();

    let view = storage.snapshot().unwrap();
    assert_eq!(get(&view, unused, b"k"), None);
    assert_eq!(fragment(&view, frag(1, 0)), None);
    let txn = storage.db.begin_read().unwrap();
    let tables: Vec<String> = txn
        .list_tables()
        .unwrap()
        .map(|table| table.name().to_owned())
        .collect();
    assert_eq!(tables, ["namespace/1"]);
}

/// Damage that redb itself detects, for real: a table under a layout name with other types.
#[test]
fn a_table_of_other_types_reads_as_corrupted() {
    let (_dir, path) = temporary_file();
    {
        let db = redb::Database::builder()
            .set_cache_size(0)
            .create(&path)
            .unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(TableDefinition::<u64, u64>::new("namespace/1"))
            .unwrap()
            .insert(1, 1)
            .unwrap();
        txn.commit().unwrap();
    }

    let storage = RedbStorage::open(&path, CACHE).unwrap();
    let read = failure(storage.snapshot().unwrap().get(A, b"k", |_| ()));
    let written = failure(storage.batch().unwrap().put(A, b"k", b"v"));
    for error in [read, written] {
        assert!(matches!(error, StorageError::Corrupted { .. }), "{error:?}");
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
    // The instance has not failed.
    commit(&storage, Durability::Durable, |batch| {
        batch.put(B, b"k", b"v").unwrap();
    });
}

fn variant(error: &StorageError) -> &'static str {
    match error {
        StorageError::Failed { .. } => "Failed",
        StorageError::Corrupted { .. } => "Corrupted",
        StorageError::Limit { .. } => "Limit",
        StorageError::Closed => "Closed",
        StorageError::RolledBack { .. } => "RolledBack",
    }
}

/// The rows of the error table that no other test reaches: redb cannot be driven into these
/// errors through this backend.
#[test]
fn errors_no_operation_reaches_map_as_documented() {
    let get = |source| RedbError::Get {
        ns: A,
        key: Box::from(&b"k"[..]),
        source,
    };
    let cases = [
        (get(redb::StorageError::PreviousIo), "Failed", true),
        (
            get(redb::StorageError::LockPoisoned(Location::caller())),
            "Failed",
            true,
        ),
        (get(redb::StorageError::Unsupported), "Failed", true),
        (get(redb::StorageError::ValueTooLarge(1)), "Limit", false),
        (get(redb::StorageError::DatabaseClosed), "Closed", false),
        (
            RedbError::Open {
                source: DatabaseError::UpgradeRequired(1),
            },
            "Corrupted",
            false,
        ),
    ];
    for (error, expected, latches) in cases {
        let case = format!("{error:?}");
        let instance = Instance::default();
        let reported = instance.report(error);
        assert_eq!(variant(&reported), expected, "{case}");
        assert_eq!(instance.admit().is_err(), latches, "{case}");
    }
}

#[test]
#[should_panic(expected = "the owner serializes writes")]
fn a_nested_batch_panics() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let _first = storage.batch().unwrap();
    let _second = storage.batch();
}

#[test]
#[should_panic(expected = "which is lending it a value")]
fn a_batch_callback_cannot_read_the_table_it_is_lent() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    let mut batch = storage.batch().unwrap();
    batch.put(A, b"k", b"v").unwrap();
    let _ = batch.scan(A, KeyRange::All, Direction::Forward, |_, _| {
        let _ = batch.get(A, b"k", |_| ());
        ControlFlow::<()>::Continue(())
    });
}

#[test]
fn a_callback_can_read_its_snapshot_and_other_tables_of_its_batch() {
    let disk = FaultDisk::default();
    let storage = open(&disk, CACHE);
    commit(&storage, Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
        batch.put(B, b"k", b"w").unwrap();
    });
    let view = storage.snapshot().unwrap();
    let batch = storage.batch().unwrap();
    let mut seen = Vec::new();
    view.scan(A, KeyRange::All, Direction::Forward, |key, _| {
        seen.push(get(&view, A, key));
        seen.push(get(&batch, B, key));
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    assert_eq!(seen, [Some(b"v".to_vec()), Some(b"w".to_vec())]);
}

#[test]
fn compaction_gives_back_the_space_of_deleted_chunks() {
    const FRAGMENTS: u32 = 32;
    let (_dir, path) = temporary_file();
    let mut storage = RedbStorage::open(&path, CACHE).unwrap();
    let bytes = vec![0xA5; MAX_FRAGMENT_LEN];
    // Chunk 2 is written after chunk 1, so deleting chunk 1 frees pages in front of chunk 2,
    // which only moving chunk 2 gives back.
    for chunk in [1, 2] {
        commit(&storage, Durability::Durable, |batch| {
            for index in 0..FRAGMENTS {
                batch.put_fragment(frag(chunk, index), &bytes).unwrap();
            }
        });
    }
    commit(&storage, Durability::Durable, |batch| {
        batch.delete_chunk(ChunkId(1)).unwrap();
    });

    let file_len = || fs::metadata(&path).expect("the file's metadata").len();
    let before = file_len();
    assert_eq!(storage.compact().unwrap(), Compaction::Compacted);
    let after = file_len();
    let deleted = u64::from(FRAGMENTS) * MAX_FRAGMENT_LEN as u64;
    assert!(
        after + deleted / 2 <= before,
        "{before} bytes became {after} after deleting {deleted}"
    );
    assert_eq!(storage.compact().unwrap(), Compaction::Unchanged);

    let view = storage.snapshot().unwrap();
    for index in 0..FRAGMENTS {
        assert_eq!(fragment(&view, frag(1, index)), None);
        assert_eq!(fragment(&view, frag(2, index)).as_ref(), Some(&bytes));
    }
}

#[test]
fn compaction_waits_until_every_snapshot_is_dropped() {
    let disk = FaultDisk::default();
    let mut storage = open(&disk, CACHE);
    commit(&storage, Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
    });
    let view = storage.snapshot().unwrap();
    let second = storage.snapshot().unwrap();
    assert_eq!(storage.compact().unwrap(), Compaction::Deferred);
    assert_eq!(get(&view, A, b"k"), Some(b"v".to_vec()));
    drop(view);
    assert_eq!(storage.compact().unwrap(), Compaction::Deferred);
    assert_eq!(get(&second, A, b"k"), Some(b"v".to_vec()));
    drop(second);
    assert_eq!(storage.compact().unwrap(), Compaction::Compacted);
}

/// A real compaction I/O failure uses the ordinary backend mapping and blocks later operations.
#[test]
fn compaction_io_failure_is_reported_and_latched_until_reopen() {
    let disk = FaultDisk::default();
    let mut storage = open(&disk, 0);
    commit(&storage, Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
    });
    disk.fail_next_io();
    let error = storage.compact().unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(matches!(error, StorageError::Failed { .. }));
    disk.assert_fault_fired();
    assert!(matches!(
        storage.snapshot(),
        Err(StorageError::Failed { .. })
    ));
    assert!(matches!(
        storage.compact(),
        Err(StorageError::Failed { .. })
    ));
    drop(storage);
    let storage = open(&disk, CACHE);
    assert_eq!(
        get(&storage.snapshot().unwrap(), A, b"k"),
        Some(b"v".to_vec())
    );
}

/// A journal record, the batch marker and four connections' auxiliary state: about what the
/// traffic consumer writes every 30 s (§0.1).
fn small_batch(batch: &mut RedbBatch<'_>, n: u64) -> usize {
    let record = [0x11; 512];
    let connection = [n as u8; 1100];
    batch.put(Ns(1), &n.to_be_bytes(), &record).unwrap();
    batch.put(Ns(2), b"batch", &n.to_be_bytes()).unwrap();
    for id in 0..4_u64 {
        batch.put(Ns(3), &id.to_be_bytes(), &connection).unwrap();
    }
    record.len() + 8 + 5 + 8 + 4 * (8 + connection.len())
}

/// Prints what a small batch costs in disk writes at each durability, for the
/// write-amplification question of §12; run with `--nocapture` to see it. Only the syncs are
/// checked: the durability has to reach redb.
#[test]
fn bytes_written_per_commit() {
    const COMMITS: u64 = 200;
    for (profile, cache_bytes) in [
        ("mobile", Profile::mobile().backend_cache),
        ("desktop", Profile::desktop().backend_cache),
    ] {
        // Every commit at `durability`, except the last, which is durable.
        let measure = |durability| {
            let disk = FaultDisk::default();
            let storage = open(&disk, cache_bytes);
            let start = disk.stats();
            let mut payload = 0;
            for n in 0..COMMITS {
                let durability = if n + 1 == COMMITS {
                    Durability::Durable
                } else {
                    durability
                };
                commit(&storage, durability, |batch| {
                    payload = small_batch(batch, n)
                });
            }
            let end = disk.stats();
            (
                payload,
                end.bytes_written - start.bytes_written,
                end.syncs - start.syncs,
            )
        };
        let (payload, durable_bytes, durable_syncs) = measure(Durability::Durable);
        let (_, consistent_bytes, consistent_syncs) = measure(Durability::Consistent);
        println!(
            "redb, {profile} cache of {cache_bytes} bytes, {COMMITS} commits of {payload} bytes \
             of keys and values: durable {} bytes and {:.2} syncs per commit; consistent with \
             a durable last {} bytes per commit and {consistent_syncs} syncs in all",
            durable_bytes / COMMITS,
            durable_syncs as f64 / COMMITS as f64,
            consistent_bytes / COMMITS,
        );
        assert!(
            consistent_syncs < durable_syncs,
            "{profile}: {consistent_syncs} syncs for consistent commits, {durable_syncs} for \
             durable ones"
        );
    }
}
