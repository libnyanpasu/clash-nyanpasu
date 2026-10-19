//! Caller-driven suspend and close, ordered at query, commit and backend-drop boundaries.

use std::{
    cell::Cell,
    sync::{Arc, Barrier, mpsc},
    thread,
    time::Duration,
};

use super::{
    Query, QueryProgress, db, maintenance, query_fixture,
    support::{Observed, Probes},
};
use crate::{
    AuxOp, CaptureError, CheckpointError, ErrorKind, FieldMask, GroupKey, GroupRequest, Maintain,
    MaintainError, Profile, RecoveryError, RecoveryProgress, SeriesGroups, SeriesQuery, Snapshot,
    TimeRange, Timestamp, Tsdb, WriteError,
    layout::{AUX, meta::read_state},
    storage::{
        CommitFailure, Durability, Storage, StorageError, StorageRead,
        conformance::{CommitOutcome, OpClass},
        memory::{MemoryDisk, MemoryStorage},
    },
    version::LifecycleState,
};

/// Retains a charged result independently of the snapshot that produced it.
fn series<S: Storage>(snapshot: &Snapshot<S>) -> SeriesGroups {
    snapshot
        .series(&SeriesQuery {
            range: TimeRange::new(Timestamp(0), Timestamp(db::HOUR_MS)).unwrap(),
            tier: db::HOUR,
            requests: &[GroupRequest {
                filters: &[],
                key: GroupKey::Total,
            }],
            fields: FieldMask(3),
        })
        .unwrap()
}

/// Both terminal admission decisions refuse before validation or opening any transaction.
fn assert_closed<S: Storage>(store: &mut Tsdb<S>, state: LifecycleState) {
    assert_eq!(store.registry().state(), state);
    let error = store
        .write(&db::batch(99, &[], &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(error, WriteError::Closed));
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(error.not_applied());
    let error = store
        .maintain(&Maintain {
            now: Timestamp(0),
            retention: &maintenance(),
            rounds: 10,
        })
        .unwrap_err();
    assert!(matches!(error, MaintainError::Closed));
    assert_eq!(error.kind(), ErrorKind::Local);
    let error = store.reader().capture().err().unwrap();
    assert!(matches!(
        error,
        CaptureError::CheckOpen {
            source: StorageError::Closed
        }
    ));
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(store.shared().storage().is_none());
    assert!(store.registry().is_empty());
    assert_eq!(store.head_reserved(), 0);
    assert_eq!(store.staging_reserved(), 0);
}

/// No assertion or retirement may strand a query that unexpectedly retains a shared lock.
#[test]
fn suspend_revokes_running_head_and_chunk_queries() {
    for (chunks, point, query) in [
        (false, "series::head:1", Query::Series { partial: false }),
        (
            false,
            "events::select_head:1",
            Query::Events { empty: false },
        ),
        (true, "series::chunk:1", Query::Series { partial: false }),
        (
            true,
            "events::load_stripe:1",
            Query::Events { empty: false },
        ),
    ] {
        let mut store = if chunks {
            query_fixture()
        } else {
            let disk = MemoryDisk::new();
            let mut store = db::open(&disk);
            db::write_simple(&mut store, 1, Durability::Consistent);
            store
        };
        let shared = store.shared().clone();
        let reader = store.reader();
        let snapshot = reader.capture().unwrap();
        let baseline = reader.query_reserved();
        let (tx, rx) = mpsc::channel();
        let resume = Arc::new(Barrier::new(2));
        let release = resume.clone();
        let paused = tx.clone();
        shared.at_query(point, 0, move || {
            paused.send(QueryProgress::Paused).unwrap();
            release.wait();
        });
        let worker = thread::spawn(move || {
            let result = query.run(&snapshot);
            tx.send(QueryProgress::Finished).unwrap();
            (result, snapshot)
        });
        let reached = matches!(rx.recv().unwrap(), QueryProgress::Paused);
        let gates_free = shared.gate_available() && shared.registry.publication_available();
        let suspended = (reached && gates_free).then(|| store.suspend(Timestamp(100)));
        if reached {
            resume.wait();
        }
        let (result, snapshot) = worker.join().unwrap();
        assert!(reached, "missed {point}");
        assert!(gates_free, "query retained G or R at {point}");
        suspended.unwrap().unwrap();
        assert_eq!(result, Err((true, ErrorKind::Local)), "{point}");
        assert_eq!(reader.query_reserved(), baseline);
        assert_closed(&mut store, LifecycleState::Suspended);
        drop(snapshot);
        assert_eq!(reader.query_reserved(), 0);
    }
}

/// The same state, auxiliary baseline and head survive both engines' suspend/resume protocol.
fn resume_cycle<S: Storage>(
    mut store: Tsdb<S>,
    mut reopen: impl FnMut() -> Result<S, StorageError>,
) {
    db::write_simple(&mut store, 1, Durability::Durable);
    let points = [db::point(db::LABELS, &[3, 4])];
    let mut input = db::batch(2, &points, &[]);
    input.aux = &[AuxOp::Put {
        key: b"active",
        value: b"baseline",
    }];
    store.write(&input, Durability::Consistent).unwrap();
    let reader = store.reader();
    let snapshot = reader.capture().unwrap();
    let result = series(&snapshot);
    let token = snapshot.catalog();
    let before = db::dump(snapshot.storage());
    let state = read_state(snapshot.storage()).unwrap();
    assert_eq!(state.publication, snapshot.version().publication);
    let expected_head = snapshot.version().head.clone();
    let old_seq = store.next_event_seq();
    store.suspend(Timestamp(100)).unwrap();
    assert_closed(&mut store, LifecycleState::Suspended);
    // Actual second open while Reader, snapshot and result survive, before recovery's open.
    let raw = reopen().unwrap();
    assert_eq!(db::dump(&raw.snapshot().unwrap()), before);
    drop(raw);
    let calls = Cell::new(0);
    assert!(matches!(
        store
            .recover_with(
                || {
                    calls.set(calls.get() + 1);
                    reopen()
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    // Drop exactly the capture; the result alone must still block reopening.
    drop(snapshot);
    assert!(reader.query_reserved() > 0);
    assert!(matches!(
        store
            .recover_with(
                || {
                    calls.set(calls.get() + 1);
                    reopen()
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    assert_eq!(calls.get(), 0);
    let during = store.next_event_seq();
    assert_eq!(during.incarnation, old_seq.incarnation);
    assert_eq!(during.seq, old_seq.seq + 1);
    assert_eq!(result.lists()[0].iter().next().unwrap().sums, &[4, 6]);
    drop(result);
    assert_eq!(reader.query_reserved(), 0);
    let progress = store
        .recover_with(&mut reopen, |view| {
            assert_eq!((view.publication(), view.last_batch()), (2, Some(2)));
            view.aux_get(b"active", |value| value.unwrap().to_vec())
        })
        .unwrap();
    let RecoveryProgress::Recovered {
        restored,
        incarnation,
        publication,
        last_batch,
    } = progress
    else {
        panic!("drained query budget still deferred");
    };
    assert_eq!(restored, b"baseline");
    assert_eq!(
        (incarnation, publication, last_batch),
        (old_seq.incarnation + 1, 2, Some(2))
    );
    let seq = store.next_event_seq();
    assert_eq!((seq.incarnation, seq.seq), (incarnation, 0));
    let snapshot = reader.capture().unwrap();
    assert_ne!(snapshot.catalog(), token);
    assert_eq!(snapshot.version().head, expected_head);
    assert_eq!(
        series(&snapshot).lists()[0].iter().next().unwrap().sums,
        &[4, 6]
    );
    assert!(Query::Events { empty: false }.run(&snapshot).is_ok());
    drop(snapshot);
    assert_eq!(reader.query_reserved(), 0);
    store
        .maintain(&Maintain {
            now: Timestamp(0),
            retention: &maintenance(),
            rounds: 1,
        })
        .unwrap();
    db::write_simple(&mut store, 3, Durability::Durable);
}

/// Consistent commits and auxiliary records survive; drain is measured by actual charges.
#[test]
fn suspend_resume_preserves_consistent_state_after_readers_and_results_drain() {
    let disk = MemoryDisk::new();
    resume_cycle(db::open(&disk), || disk.open());
}

/// A live Reader and old read transaction cannot retain the actual redb file lock.
#[cfg(feature = "redb")]
#[test]
fn redb_suspend_releases_the_file_and_recover_with_resumes_it() {
    use crate::storage::redb::RedbStorage;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("suspend.redb");
    let reopen = || RedbStorage::open(&path, 4 << 20);
    let store = Tsdb::open(reopen().unwrap(), db::schema(), Profile::desktop()).unwrap();
    resume_cycle(store, reopen);
}

/// Checkpoint faults occur at read, write, commit, or before beginning on a failed backend.
#[test]
fn checkpoint_failures_still_detach_in_the_requested_state() {
    for close in [false, true] {
        for fault in 0..6 {
            let disk = MemoryDisk::new();
            let mut store = db::open_with(&disk, Profile::mobile());
            db::write_simple(&mut store, 1, Durability::Durable);
            db::write_simple(&mut store, 2, Durability::Consistent);
            let due = store.durability_due(Timestamp(101));
            match fault {
                0 => store.storage().inject_commit(CommitOutcome::NotApplied),
                1 => store
                    .storage()
                    .inject_commit(CommitOutcome::Unknown { applied: false }),
                2 => store
                    .storage()
                    .inject_commit(CommitOutcome::Unknown { applied: true }),
                3 => store.storage().fail_next(OpClass::Read),
                4 => store.storage().fail_next(OpClass::Write),
                _ => {
                    store.storage().fail_next(OpClass::Read);
                    assert!(
                        store
                            .storage()
                            .snapshot()
                            .unwrap()
                            .get(AUX, b"x", |_| ())
                            .is_err()
                    );
                    store.registry().require_recovery();
                }
            }
            let error = if close {
                store.close(Timestamp(100))
            } else {
                store.suspend(Timestamp(100))
            }
            .unwrap_err();
            assert_eq!(
                error.kind(),
                if fault == 0 {
                    ErrorKind::Local
                } else {
                    ErrorKind::BackendFailed
                }
            );
            match (fault, error) {
                (
                    0,
                    CheckpointError::Commit {
                        source: CommitFailure::NotApplied { .. },
                    },
                )
                | (
                    1 | 2,
                    CheckpointError::Commit {
                        source: CommitFailure::Unknown { .. },
                    },
                )
                | (3, CheckpointError::ReadState { .. })
                | (4, CheckpointError::WriteState { .. })
                | (5, CheckpointError::BeginBatch { .. }) => (),
                (_, error) => panic!("wrong checkpoint failure: {error:?}"),
            }
            let state = if close {
                LifecycleState::Closed
            } else {
                LifecycleState::Suspended
            };
            assert_closed(&mut store, state);
            assert_eq!(store.durability_due(Timestamp(101)), due);
            // Idempotence after failure neither reports that failure again nor reopens storage.
            store.suspend(Timestamp(200)).unwrap();
            assert_closed(&mut store, state);
            store.close(Timestamp(200)).unwrap();
            assert_closed(&mut store, LifecycleState::Closed);
            drop(disk.open().unwrap());
        }
    }
}

/// Admission/revocation precede commit; backend destruction follows commit outside G and R.
#[test]
fn checkpoint_and_drop_boundaries_preserve_metadata_and_idempotence() {
    for close in [false, true] {
        let disk = MemoryDisk::new();
        let probes = Arc::new(Probes::default());
        let mut store = Tsdb::open(
            Observed::new(disk.open().unwrap(), probes.clone()),
            db::schema(),
            Profile::desktop(),
        )
        .unwrap();
        db::write_simple(&mut store, 1, Durability::Consistent);
        let reader = store.reader();
        let snapshot = reader.capture().unwrap();
        let before = db::dump(snapshot.storage());
        let shared = store.shared().clone();
        let committing = shared.clone();
        let (tx, rx) = mpsc::channel();
        probes.commit.arm(move || {
            let g = committing.gate_available();
            let r = committing.registry.publication_available();
            let state = r.then(|| committing.registry.state());
            let revoked = committing.check_open().is_err();
            let _ = tx.send((g, r, state, revoked));
        });
        let closing = shared.clone();
        let (tx, closed) = mpsc::channel();
        probes.close.arm(move || {
            let g = closing.gate_available();
            let r = closing.registry.publication_available();
            let empty = r.then(|| closing.registry.is_empty());
            // A failed assertion may drop the receiver before owner retirement runs this hook.
            let _ = tx.send((g, r, empty));
        });
        probes
            .snapshot
            .arm(|| panic!("checkpoint opened a planning snapshot"));
        probes.durabilities.lock().unwrap().clear();
        if close {
            store.close(Timestamp(100))
        } else {
            store.suspend(Timestamp(100))
        }
        .unwrap();
        let state = if close {
            LifecycleState::Closed
        } else {
            LifecycleState::Suspended
        };
        assert_eq!(rx.try_recv().unwrap(), (false, true, Some(state), true));
        assert_eq!(closed.try_recv().unwrap(), (true, true, Some(true)));
        assert_eq!(*probes.durabilities.lock().unwrap(), [Durability::Durable]);
        assert_closed(&mut store, state);
        assert_eq!(
            Query::Series { partial: false }.run(&snapshot),
            Err((true, ErrorKind::Local))
        );
        drop(snapshot);
        assert_eq!(reader.query_reserved(), 0);
        let raw = disk.open().unwrap();
        assert_eq!(db::dump(&raw.snapshot().unwrap()), before);
        // While an independent backend owns the disk, repeated lifecycle calls cannot reopen it.
        store.suspend(Timestamp(200)).unwrap();
        store.close(Timestamp(200)).unwrap();
        store.close(Timestamp(300)).unwrap();
        store.suspend(Timestamp(300)).unwrap();
        assert_closed(&mut store, LifecycleState::Closed);
        let seq = store.next_event_seq();
        let calls = Cell::new(0);
        let error = store
            .recover_with(
                || {
                    calls.set(calls.get() + 1);
                    disk.open().map(|s| Observed::new(s, probes.clone()))
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(matches!(error, RecoveryError::Closed));
        assert_eq!(error.kind(), ErrorKind::Local);
        assert_eq!(calls.get(), 0);
        assert_eq!(store.next_event_seq().seq, seq.seq + 1);
        assert_closed(&mut store, LifecycleState::Closed);
        assert_eq!(*probes.durabilities.lock().unwrap(), [Durability::Durable]);
    }
}

/// Public durability decisions distinguish both counters: elapsed time and written bytes.
#[test]
fn durable_bookkeeping_resets_only_after_a_confirmed_checkpoint() {
    for fail in [false, true] {
        for byte_limit in [1, u64::MAX] {
            let disk = MemoryDisk::new();
            let profile = Profile {
                durable_interval: Some(Duration::from_millis(100)),
                durable_after_bytes: byte_limit,
                ..Profile::desktop()
            };
            let mut store = db::open_with(&disk, profile);
            db::write_simple(&mut store, 1, Durability::Durable);
            db::write_simple(&mut store, 2, Durability::Consistent);
            if fail {
                store.storage().inject_commit(CommitOutcome::NotApplied);
            }
            let result = store.suspend(Timestamp(1000));
            assert_eq!(result.is_err(), fail);
            assert_eq!(
                store.durability_due(Timestamp(1001)),
                if fail {
                    Durability::Durable
                } else {
                    Durability::Consistent
                }
            );
            assert_eq!(
                store.durability_due(Timestamp(1)),
                if !fail || byte_limit == 1 {
                    Durability::Durable
                } else {
                    Durability::Consistent
                }
            );
        }
    }
}

/// A crash immediately after checkpointing must retain data even without clean backend Drop.
#[test]
fn checkpoint_promotes_consistent_commits_before_backend_destruction() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    db::write_simple(&mut store, 1, Durability::Consistent);
    let storage: MemoryStorage = store.into_storage();
    super::super::checkpoint(&storage).unwrap();
    storage.crash();
    let reopened = db::open(&disk);
    assert_eq!(reopened.current().publication, 1);
    let snapshot = reopened.reader().capture().unwrap();
    assert_eq!(
        series(&snapshot).lists()[0].iter().next().unwrap().sums,
        &[1, 2]
    );
}

/// Every admission state gives its documented owner/capture answer before validation or I/O.
#[test]
fn admission_and_stopping_cover_every_lifecycle_state() {
    for state in [
        LifecycleState::Open,
        LifecycleState::RecoveryNeeded,
        LifecycleState::Recovering,
        LifecycleState::WaitingForReaders,
        LifecycleState::Unavailable,
        LifecycleState::Suspended,
        LifecycleState::Closed,
    ] {
        for close in [false, true] {
            let disk = MemoryDisk::new();
            let probes = Arc::new(Probes::default());
            let mut store = Tsdb::open(
                Observed::new(disk.open().unwrap(), probes.clone()),
                db::schema(),
                Profile::desktop(),
            )
            .unwrap();
            match state {
                LifecycleState::Open => (),
                LifecycleState::RecoveryNeeded => store.registry().require_recovery(),
                LifecycleState::Suspended => store.suspend(Timestamp(0)).unwrap(),
                LifecycleState::Closed => store.close(Timestamp(0)).unwrap(),
                // These states occur after retirement and before installation in recover_with.
                _ => drop(store.shared().retire(state)),
            }
            probes.durabilities.lock().unwrap().clear();
            if matches!(state, LifecycleState::Suspended | LifecycleState::Closed) {
                assert_closed(&mut store, state);
            } else if state == LifecycleState::Open {
                db::write_simple(&mut store, 1, Durability::Durable);
                store
                    .maintain(&Maintain {
                        now: Timestamp(0),
                        retention: &maintenance(),
                        rounds: 1,
                    })
                    .unwrap();
                drop(store.reader().capture().unwrap());
                probes.durabilities.lock().unwrap().clear();
            } else {
                let error = store
                    .write(&db::batch(99, &[], &[]), Durability::Durable)
                    .unwrap_err();
                assert!(matches!(error, WriteError::RecoveryNeeded));
                assert_eq!(error.kind(), ErrorKind::BackendFailed);
                assert!(error.not_applied());
                let error = store
                    .maintain(&Maintain {
                        now: Timestamp(0),
                        retention: &maintenance(),
                        rounds: 1,
                    })
                    .unwrap_err();
                assert!(matches!(error, MaintainError::RecoveryNeeded));
                assert_eq!(error.kind(), ErrorKind::BackendFailed);
                let error = store.reader().capture().err().unwrap();
                assert!(matches!(error, CaptureError::RecoveryNeeded));
                assert_eq!(error.kind(), ErrorKind::BackendFailed);
            }
            assert_eq!(store.registry().state(), state);
            assert!(probes.durabilities.lock().unwrap().is_empty());
            if close {
                store.close(Timestamp(100))
            } else {
                store.suspend(Timestamp(100))
            }
            .unwrap();
            let terminal = if close || state == LifecycleState::Closed {
                LifecycleState::Closed
            } else {
                LifecycleState::Suspended
            };
            assert_closed(&mut store, terminal);
            let commits = probes.durabilities.lock().unwrap().clone();
            assert_eq!(
                commits.as_slice(),
                if matches!(state, LifecycleState::Open | LifecycleState::RecoveryNeeded) {
                    &[Durability::Durable][..]
                } else {
                    &[][..]
                }
            );
            assert_eq!(store.reader().query_reserved(), 0);
        }
    }
}
