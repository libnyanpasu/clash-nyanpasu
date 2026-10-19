//! Explicit compaction admission, capture ordering and backend-owned exclusivity.

use std::{
    sync::{Arc, Barrier, mpsc},
    thread,
};

use super::{
    Query, db, maintenance, read_fault,
    support::{Observed, Probes},
};
use crate::{
    CaptureError, CompactError, Compaction, ErrorKind, FieldMask, GroupKey, GroupRequest, Maintain,
    MaintainError, Profile, RecoveryProgress, SeriesGroups, SeriesQuery, Snapshot, TimeRange,
    Timestamp, Tsdb, WriteError,
    storage::{Durability, Storage, memory::MemoryDisk},
    version::LifecycleState,
};

/// Queries retain their credit but never a storage transaction.
fn series<S: Storage>(snapshot: &Snapshot<S>) -> SeriesGroups {
    snapshot
        .series(&SeriesQuery {
            range: TimeRange::new(Timestamp(0), Timestamp(100 * db::HOUR_MS)).unwrap(),
            tier: db::HOUR,
            requests: &[GroupRequest {
                filters: &[],
                key: GroupKey::Total,
            }],
            fields: FieldMask(3),
        })
        .unwrap()
}

/// Unsupported is a successful capability answer, even with live queries on a memory backend.
#[test]
fn memory_compaction_is_unsupported_and_preserves_queries() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    db::write_simple(&mut store, 1, Durability::Consistent);
    let reader = store.reader();
    let snapshot = reader.capture().unwrap();
    let result = series(&snapshot);
    let publication = snapshot.version().publication;
    assert_eq!(store.compact().unwrap(), Compaction::Unsupported);
    assert_eq!(store.registry().state(), LifecycleState::Open);
    assert_eq!(store.current().publication, publication);
    assert!(Query::Series { partial: false }.run(&snapshot).is_ok());
    assert!(Query::Events { empty: false }.run(&snapshot).is_ok());
    drop(snapshot);
    assert!(reader.query_reserved() > 0);
    assert_eq!(store.compact().unwrap(), Compaction::Unsupported);
    drop(result);
    assert_eq!(reader.query_reserved(), 0);
    db::write_simple(&mut store, 2, Durability::Durable);
}

/// A skipped backend boundary still reports completion instead of hanging the barrier test.
enum Progress {
    /// Compaction owns G and is parked inside the backend capability.
    Entered,
    /// The owner call returned, whether or not it visited the backend.
    Finished,
}

/// A capture started during compaction cannot reach storage until G-write is released.
#[test]
fn racing_capture_waits_for_compaction_without_revoking_healthy_queries() {
    let disk = MemoryDisk::new();
    let probes = Arc::new(Probes::default());
    let mut store = Tsdb::open(
        Observed::new(disk.open().unwrap(), probes.clone()),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    db::write_simple(&mut store, 1, Durability::Consistent);
    let shared = store.shared().clone();
    let reader = store.reader();
    let old = reader.capture().unwrap();
    let publication = old.version().publication;
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    let (tx, progress) = mpsc::channel();
    let entered = tx.clone();
    probes.compact.arm(move || {
        entered.send(Progress::Entered).unwrap();
        release.wait();
    });
    let compacting = thread::spawn(move || {
        let result = store.compact();
        tx.send(Progress::Finished).unwrap();
        (store, result)
    });
    let reached = matches!(progress.recv().unwrap(), Progress::Entered);
    let g_read_free = shared.capture_gate_available();
    let g_write_free = shared.gate_available();
    let r_free = shared.registry.publication_available();
    let revoked = shared.check_open().is_err();
    let old_query = Query::Series { partial: false }.run(&old);
    let (visited, visit) = mpsc::channel();
    probes.snapshot.arm(move || {
        let _ = visited.send(());
    });
    let capturing = reader.clone();
    let (started, start) = mpsc::channel();
    let capture = thread::spawn(move || {
        started.send(()).unwrap();
        capturing.capture()
    });
    start.recv().unwrap();
    if reached {
        resume.wait();
    }
    let (mut store, result) = compacting.join().unwrap();
    let captured = capture.join().unwrap();
    // No probe assertion may unwind and retire while either worker is parked.
    assert!(reached);
    assert!(!g_read_free && !g_write_free && r_free);
    assert!(!revoked && old_query.is_ok());
    assert_eq!(result.unwrap(), Compaction::Unsupported);
    let snapshot = captured.unwrap();
    visit.try_recv().unwrap();
    assert_eq!(snapshot.version().publication, publication);
    assert_eq!(snapshot.catalog(), old.catalog());
    assert!(Query::Events { empty: false }.run(&snapshot).is_ok());
    drop((old, snapshot));
    assert_eq!(reader.query_reserved(), 0);
    db::write_simple(&mut store, 2, Durability::Durable);
}

/// Error classification drives recovery; Local and Corrupted do not poison an admitted facade.
#[test]
fn compaction_failures_latch_only_backend_failed_and_recover_explicitly() {
    for kind in [
        ErrorKind::Local,
        ErrorKind::Corrupted,
        ErrorKind::BackendFailed,
    ] {
        let disk = MemoryDisk::new();
        let probes = Arc::new(Probes::default());
        let mut store = Tsdb::open(
            Observed::new(disk.open().unwrap(), probes.clone()),
            db::schema(),
            Profile::desktop(),
        )
        .unwrap();
        db::write_simple(&mut store, 1, Durability::Durable);
        *probes.compact_fault.lock().unwrap() = Some(read_fault(kind));
        let error = store.compact().unwrap_err();
        assert!(matches!(error, CompactError::Compact { .. }));
        assert_eq!(error.kind(), kind);
        assert!(store.shared().gate_available() && store.registry().publication_available());
        if kind == ErrorKind::BackendFailed {
            assert_eq!(store.registry().state(), LifecycleState::RecoveryNeeded);
            assert!(matches!(store.compact(), Err(CompactError::RecoveryNeeded)));
            assert!(matches!(
                store.write(&db::batch(2, &[], &[]), Durability::Durable),
                Err(WriteError::RecoveryNeeded)
            ));
            assert!(matches!(
                store.maintain(&Maintain {
                    now: Timestamp(0),
                    retention: &maintenance(),
                    rounds: 1
                }),
                Err(MaintainError::RecoveryNeeded)
            ));
            assert!(matches!(
                store.reader().capture(),
                Err(CaptureError::RecoveryNeeded)
            ));
            assert!(matches!(
                store
                    .recover_with(
                        || disk.open().map(|s| Observed::new(s, probes.clone())),
                        |_| Ok(())
                    )
                    .unwrap(),
                RecoveryProgress::Recovered { .. }
            ));
        }
        assert_eq!(store.registry().state(), LifecycleState::Open);
        assert_eq!(store.compact().unwrap(), Compaction::Unsupported);
        assert!(
            Query::Events { empty: false }
                .run(&store.reader().capture().unwrap())
                .is_ok()
        );
        db::write_simple(&mut store, 2, Durability::Durable);
    }
}

/// Non-Open admission mirrors write, and other owner calls never request the capability.
#[test]
fn compaction_admission_covers_every_state_and_no_owner_call_invokes_it_implicitly() {
    for state in [
        LifecycleState::Open,
        LifecycleState::RecoveryNeeded,
        LifecycleState::Recovering,
        LifecycleState::WaitingForReaders,
        LifecycleState::Unavailable,
        LifecycleState::Suspended,
        LifecycleState::Closed,
    ] {
        let disk = MemoryDisk::new();
        let probes = Arc::new(Probes::default());
        let mut store = Tsdb::open(
            Observed::new(disk.open().unwrap(), probes.clone()),
            db::schema(),
            Profile::desktop(),
        )
        .unwrap();
        let (called, calls) = mpsc::channel();
        probes.compact.arm(move || {
            let _ = called.send(());
        });
        db::write_simple(&mut store, 1, Durability::Consistent);
        store
            .maintain(&Maintain {
                now: Timestamp(0),
                retention: &maintenance(),
                rounds: 1,
            })
            .unwrap();
        match state {
            LifecycleState::Open => (),
            LifecycleState::RecoveryNeeded => store.registry().require_recovery(),
            LifecycleState::Suspended => store.suspend(Timestamp(1)).unwrap(),
            LifecycleState::Closed => store.close(Timestamp(1)).unwrap(),
            _ => drop(store.shared().retire(state)),
        }
        assert!(
            calls.try_recv().is_err(),
            "implicit compaction entering {state:?}"
        );
        let compacted = store.compact();
        if state == LifecycleState::Open {
            assert_eq!(compacted.unwrap(), Compaction::Unsupported);
            calls.try_recv().unwrap();
        } else {
            let error = compacted.unwrap_err();
            if matches!(state, LifecycleState::Suspended | LifecycleState::Closed) {
                assert!(matches!(error, CompactError::Closed));
                assert_eq!(error.kind(), ErrorKind::Local);
            } else {
                assert!(matches!(error, CompactError::RecoveryNeeded));
                assert_eq!(error.kind(), ErrorKind::BackendFailed);
            }
            assert!(calls.try_recv().is_err());
        }
        assert_eq!(store.registry().state(), state);
    }
}

/// All test queries retain their ordinary charged results, rather than an old read transaction.
#[cfg(feature = "redb")]
struct Answers {
    /// Full and partial scans of each series tier, with total, label and mapped grouping.
    series: Vec<SeriesGroups>,
    /// Forward/backward pages, filters and cursor continuations for every event stream.
    events: Vec<crate::EventPage>,
}

#[cfg(feature = "redb")]
impl Answers {
    /// Runs each public query shape against one captured publication.
    fn read<S: Storage>(snapshot: &Snapshot<S>) -> Self {
        use crate::{
            EventCondition, EventFilter, EventPredicate, EventQuery, LabelMap, storage::Direction,
        };
        let mut series = Vec::new();
        for tier in [db::MINUTE, db::HOUR] {
            for start in [0, 48 * db::HOUR_MS + 1] {
                series.push(
                    snapshot
                        .series(&SeriesQuery {
                            range: TimeRange::new(Timestamp(start), Timestamp(100 * db::HOUR_MS))
                                .unwrap(),
                            tier,
                            requests: &[
                                GroupRequest {
                                    filters: &[],
                                    key: GroupKey::Total,
                                },
                                GroupRequest {
                                    filters: &[],
                                    key: GroupKey::Labels(&[db::TARGET]),
                                },
                                GroupRequest {
                                    filters: &[],
                                    key: GroupKey::Mapped(&[LabelMap {
                                        dimension: db::TARGET,
                                        nodes: &[],
                                        skip: None,
                                    }]),
                                },
                            ],
                            fields: FieldMask(3),
                        })
                        .unwrap(),
                );
            }
        }
        let mut events = Vec::new();
        for stream in [db::CLOSED, db::LOGS] {
            for direction in [Direction::Forward, Direction::Backward] {
                let query = EventQuery {
                    stream,
                    range: TimeRange::new(Timestamp(0), Timestamp(100 * db::HOUR_MS)).unwrap(),
                    direction,
                    cursor: None,
                    predicate: &EventPredicate::default(),
                    limit: 1,
                };
                let first = snapshot.scan_events(&query).unwrap();
                events.push(
                    snapshot
                        .scan_events(&EventQuery {
                            cursor: first.cursor(),
                            limit: 100,
                            ..query
                        })
                        .unwrap(),
                );
                events.push(first);
                events.push(
                    snapshot
                        .scan_events(&EventQuery {
                            predicate: &EventPredicate {
                                filters: &[EventFilter {
                                    column: if stream == db::CLOSED { 6 } else { 0 },
                                    condition: EventCondition::Contains(b"keep"),
                                }],
                            },
                            limit: 100,
                            ..query
                        })
                        .unwrap(),
                );
            }
        }
        Self { series, events }
    }

    /// Compares payloads, catalog tokens, cursor positions and all public result metadata.
    fn assert_equal(&self, after: &Self) {
        assert_eq!(self.series.len(), after.series.len());
        for (before, after) in self.series.iter().zip(&after.series) {
            assert_eq!(before.catalog(), after.catalog());
            assert_eq!(before.covered(), after.covered());
            assert_eq!(
                before.truncated_by_retention(),
                after.truncated_by_retention()
            );
            assert_eq!(before.retention_watermark(), after.retention_watermark());
            assert_eq!(before.folded(), after.folded());
            assert_eq!(before.lists().len(), after.lists().len());
            for (before, after) in before.lists().iter().zip(after.lists()) {
                assert_eq!(before.len(), after.len());
                for group in before.iter() {
                    assert!(after.iter().any(|other| other == group));
                }
            }
        }
        assert_eq!(self.events.len(), after.events.len());
        for (before, after) in self.events.iter().zip(&after.events) {
            assert_eq!(before.events(), after.events());
            assert_eq!(before.catalog(), after.catalog());
            assert_eq!(before.cursor(), after.cursor());
            assert_eq!(
                before.truncated_by_retention(),
                after.truncated_by_retention()
            );
            assert_eq!(before.retention_watermark(), after.retention_watermark());
        }
    }
}

/// Backend read transactions, not Reader clones or result credit, decide Deferred.
#[cfg(feature = "redb")]
#[test]
fn redb_live_snapshot_defers_but_readers_and_results_do_not() {
    use crate::storage::redb::RedbStorage;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Tsdb::open(
        RedbStorage::open(dir.path().join("readers.redb"), 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    assert!(matches!(
        store.compact().unwrap(),
        Compaction::Executed { .. }
    ));
    assert_eq!(
        store.compact().unwrap(),
        Compaction::Executed { changed: false }
    );
    db::write_simple(&mut store, 1, Durability::Consistent);
    let reader = store.reader();
    let clone = reader.clone();
    let first = reader.capture().unwrap();
    let second = clone.capture().unwrap();
    let result = series(&first);
    assert_eq!(store.compact().unwrap(), Compaction::Deferred);
    assert_eq!(store.registry().state(), LifecycleState::Open);
    assert!(Query::Events { empty: false }.run(&first).is_ok());
    db::write_simple(&mut store, 2, Durability::Consistent);
    drop(first);
    assert_eq!(store.compact().unwrap(), Compaction::Deferred);
    drop(second);
    assert!(reader.query_reserved() > 0);
    assert!(matches!(
        store.compact().unwrap(),
        Compaction::Executed { .. }
    ));
    assert_eq!(result.lists()[0].iter().next().unwrap().sums, &[1, 2]);
    let new = clone.capture().unwrap();
    assert_eq!(series(&new).lists()[0].iter().next().unwrap().sums, &[2, 4]);
    drop((new, result));
    assert_eq!(reader.query_reserved(), 0);
}

/// Capture already inside G pins a real transaction before compaction can ask redb to proceed.
#[cfg(feature = "redb")]
#[test]
fn capture_started_first_makes_redb_compaction_defer() {
    use crate::storage::redb::RedbStorage;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Tsdb::open(
        RedbStorage::open(dir.path().join("race.redb"), 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    db::write_simple(&mut store, 1, Durability::Durable);
    let reader = store.reader();
    let shared = store.shared().clone();
    let release = Arc::new(Barrier::new(2));
    let paused = release.clone();
    let (tx, pinned) = mpsc::channel();
    shared.registry.after_pin(move || {
        tx.send(()).unwrap();
        paused.wait();
    });
    let capturing = reader.clone();
    let capture = thread::spawn(move || capturing.capture());
    pinned.recv().unwrap();
    let g_busy = !shared.gate_available();
    let r_busy = !shared.registry.publication_available();
    let (tx, started) = mpsc::channel();
    let compactor = thread::spawn(move || {
        tx.send(()).unwrap();
        let result = store.compact();
        (store, result)
    });
    started.recv().unwrap();
    release.wait();
    let snapshot = capture.join().unwrap().unwrap();
    let (mut store, result) = compactor.join().unwrap();
    assert!(g_busy && r_busy);
    assert_eq!(result.unwrap(), Compaction::Deferred);
    assert!(Query::Series { partial: false }.run(&snapshot).is_ok());
    drop(snapshot);
    assert_eq!(reader.query_reserved(), 0);
    assert!(matches!(
        store.compact().unwrap(),
        Compaction::Executed { .. }
    ));
}

/// Physical reclamation leaves namespace records, every query shape and logical positions equal.
#[cfg(feature = "redb")]
#[test]
fn sparse_redb_compaction_reclaims_file_space_and_preserves_all_queries() {
    use crate::{EventRow, Retention, SeriesPoint, Value, storage::redb::RedbStorage};
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse.redb");
    let mut store = Tsdb::open(
        RedbStorage::open(&path, 4 << 20).unwrap(),
        db::schema(),
        Profile {
            event_buffer_rows: 1,
            ..Profile::desktop()
        },
    )
    .unwrap();
    let payload = vec![b'x'; 160_000];
    for id in 1..=26 {
        let at = if id <= 24 { 0 } else { 48 * db::HOUR_MS };
        let points = [db::MINUTE, db::HOUR].map(|tier| SeriesPoint {
            tier,
            at: Timestamp(at),
            ..db::point(db::LABELS, &[1, 2])
        });
        let mut values = db::closed_values(Value::Series(db::LABELS));
        values[6] = Value::Bytes(if id <= 24 { &payload } else { b"keep" });
        let rows = [EventRow {
            stream: db::CLOSED,
            key: at + id as i64,
            seq: store.next_event_seq(),
            values: &values,
        }];
        let mut input = db::batch(id, &points, &rows);
        input.now = Timestamp(at);
        store.write(&input, Durability::Durable).unwrap();
    }
    // Keep an epoch-stream row and head sources beside the surviving time-stream chunks.
    db::write_simple(&mut store, 27, Durability::Consistent);
    let mut policy = maintenance();
    for retention in &mut policy.tiers {
        *retention = Retention {
            max_age: Some(Duration::from_secs(86_400)),
            max_bytes: None,
        };
    }
    policy.streams[0] = policy.tiers[0];
    let request = Maintain {
        now: Timestamp(48 * db::HOUR_MS + 100),
        retention: &policy,
        rounds: 100,
    };
    assert!(!store.maintain(&request).unwrap().more);
    let reader = store.reader();
    let before_snapshot = reader.capture().unwrap();
    let before_records = db::dump(before_snapshot.storage());
    let before = Answers::read(&before_snapshot);
    let publication = before_snapshot.version().publication;
    let before_seq = store.next_event_seq();
    drop(before_snapshot);
    assert!(
        reader.query_reserved() > 0,
        "retained answers must not defer compaction"
    );
    let old_size = std::fs::metadata(&path).unwrap().len();
    assert_eq!(
        store.compact().unwrap(),
        Compaction::Executed { changed: true }
    );
    let new_size = std::fs::metadata(&path).unwrap().len();
    assert!(
        new_size + 1_000_000 <= old_size,
        "{old_size} became {new_size}"
    );
    assert_eq!(
        store.compact().unwrap(),
        Compaction::Executed { changed: false }
    );
    let after_seq = store.next_event_seq();
    assert_eq!(
        (after_seq.incarnation, after_seq.seq),
        (before_seq.incarnation, before_seq.seq + 1)
    );
    let after_snapshot = reader.capture().unwrap();
    assert_eq!(after_snapshot.version().publication, publication);
    assert_eq!(db::dump(after_snapshot.storage()), before_records);
    let after = Answers::read(&after_snapshot);
    before.assert_equal(&after);
    drop((after_snapshot, before, after));
    assert_eq!(reader.query_reserved(), 0);
    db::write_simple(&mut store, 28, Durability::Durable);
}
