//! Recovery boundaries use explicit faults, barriers and completion acknowledgements.

use std::{
    cell::Cell,
    ops::ControlFlow,
    sync::{Arc, Barrier, mpsc},
    thread,
};

use super::{
    Query, QueryProgress, db, maintenance, read_fault,
    support::{Observed, Probes},
};
use crate::{
    AuxOp, CaptureError, ErrorKind, EventPage, EventQuery, EventRow, EventSeq, FieldMask, GroupKey,
    GroupRequest, Maintain, MaintainError, Profile, RecoveryError, RecoveryProgress, RecoveryView,
    SeriesGroups, SeriesQuery, Snapshot, TimeRange, Timestamp, Tsdb, Value, WriteError, Written,
    catalog::{pin_count, pinned_combo},
    layout::meta::read_state,
    storage::{
        Direction, Durability, KeyRange, Storage, StorageError, StorageRead,
        conformance::{CommitOutcome, OpClass},
        memory::{MemoryDisk, MemoryStorage},
    },
    version::{LifecycleState, Shared},
    write::PinOp,
};

/// Probes a boundary without acquiring a registry guard when a broken implementation holds it.
fn boundary<S: Storage>(shared: &Shared<S>) -> (bool, bool, Option<LifecycleState>) {
    let gate = shared.gate_available();
    let publication = shared.registry.publication_available();
    (
        gate,
        publication,
        publication.then(|| shared.registry.state()),
    )
}

/// A second combination distinguishes restored pins from merely preserved counts.
const OTHER: &[&[u8]] = &[b"match", b"curl", b"b.com"];

/// One transaction changes data, the active auxiliary baseline and its pin together.
fn write<S: Storage>(
    store: &mut Tsdb<S>,
    id: u64,
    durability: Durability,
) -> Result<Written, WriteError> {
    let labels = if id == 1 { db::LABELS } else { OTHER };
    let points = [db::point(labels, &[1, 2])];
    let rows = [EventRow {
        stream: db::LOGS,
        key: id as i64,
        seq: store.next_event_seq(),
        values: &[Value::Bytes(b"line")],
    }];
    let value = id.to_le_bytes();
    let aux = [AuxOp::Put {
        key: b"active",
        value: &value,
    }];
    let mut pins = Vec::new();
    if id != 1 {
        pins.push(PinOp::Unpin { key: b"active" });
    }
    pins.push(PinOp::Pin {
        key: b"active",
        labels,
    });
    store.write(
        &crate::Input {
            aux: &aux,
            pins: &pins,
            ..db::batch(id, &points, &rows)
        },
        durability,
    )
}

/// Restores the caller's owned baseline while leaving all transaction ownership private.
fn restore<S: Storage>(view: RecoveryView<'_, S>) -> Result<Vec<u8>, RecoveryError> {
    view.aux_get(b"active", |value| value.unwrap().to_vec())
}

/// A complete attempt must return the caller's value and metadata, not defer silently.
fn recovered<T>(progress: RecoveryProgress<T>) -> (T, u64, u64, Option<u64>) {
    match progress {
        RecoveryProgress::Recovered {
            restored,
            incarnation,
            publication,
            last_batch,
        } => (restored, incarnation, publication, last_batch),
        RecoveryProgress::WaitingForReaders => panic!("unexpected reader reservation"),
    }
}

/// A retained series result exercises credit independently of its source snapshot.
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

/// An event page may be charged or genuinely empty without keeping a version.
fn events<S: Storage>(snapshot: &Snapshot<S>, empty: bool) -> EventPage {
    snapshot
        .scan_events(&EventQuery {
            stream: db::LOGS,
            range: TimeRange::new(Timestamp(0), Timestamp(if empty { 0 } else { 100 })).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &crate::EventPredicate::default(),
            limit: 100,
        })
        .unwrap()
}

/// Checks public data and the persistent pin against the same surviving batch prefix.
fn assert_data<S: Storage>(store: &Tsdb<S>, batches: u64) {
    let snapshot = store.reader().capture().unwrap();
    let sums = series(&snapshot);
    assert_eq!(
        sums.lists()[0].iter().next().unwrap().sums,
        &[batches, batches * 2]
    );
    let page = events(&snapshot, false);
    assert_eq!(page.events().len(), batches as usize);
    let state = read_state(snapshot.storage()).unwrap();
    assert_eq!(state.last_batch, Some(batches));
    assert_eq!(state.publication, snapshot.version().publication);
    let combo = pinned_combo(snapshot.storage(), b"active")
        .unwrap()
        .unwrap();
    assert_eq!(pin_count(snapshot.storage(), combo).unwrap(), 1);
    let labels: Vec<_> = snapshot
        .combination(crate::ComboId::new(combo))
        .unwrap()
        .collect();
    assert_eq!(
        snapshot.label(db::TARGET, labels[2]),
        Some(if batches == 1 {
            b"a.com".as_slice()
        } else {
            b"b.com".as_slice()
        })
    );
}

/// Every nonterminal refusal gives the host exactly the same recovery instruction.
fn assert_refused<S: Storage>(store: &mut Tsdb<S>, state: LifecycleState) {
    assert_eq!(store.registry().state(), state);
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

/// Detached failure states own neither the backend, registry roots nor operation credit.
fn assert_unavailable<S: Storage>(store: &mut Tsdb<S>) {
    assert_refused(store, LifecycleState::Unavailable);
    assert!(store.shared().storage().is_none());
    assert!(store.registry().is_empty());
    assert_eq!(store.staging_reserved(), 0);
    assert_eq!(store.shared().query_budget.reserved(), 0);
}

/// A durable unknown commit is either fully present or fully absent, including aux and pins.
#[test]
fn unknown_outcomes_restore_data_aux_and_pins_from_one_publication() {
    for applied in [false, true] {
        let disk = MemoryDisk::new();
        let mut store = db::open(&disk);
        write(&mut store, 1, Durability::Durable).unwrap();
        store
            .storage()
            .inject_commit(CommitOutcome::Unknown { applied });
        let failure = write(&mut store, 2, Durability::Durable).unwrap_err();
        assert!(!failure.not_applied());
        assert_refused(&mut store, LifecycleState::RecoveryNeeded);
        let calls = Cell::new(0);
        let (baseline, incarnation, publication, last_batch) = recovered(
            store
                .recover_with(
                    || {
                        calls.set(calls.get() + 1);
                        disk.open()
                    },
                    restore,
                )
                .unwrap(),
        );
        let expected = if applied { 2 } else { 1 };
        assert_eq!(calls.get(), 1);
        assert_eq!(baseline, u64::to_le_bytes(expected));
        assert_eq!(
            (incarnation, publication, last_batch),
            (2, expected, Some(expected))
        );
        assert_data(&store, expected);
        assert_eq!(store.reader().query_reserved(), 0);
        write(&mut store, expected + 1, Durability::Durable).unwrap();
        assert_data(&store, expected + 1);
    }
}

/// Real failure precedes close, so three Consistent commits cannot be made durable by Drop.
#[test]
fn several_consistent_commits_roll_back_before_host_restoration() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    write(&mut store, 1, Durability::Durable).unwrap();
    for id in 2..=4 {
        write(&mut store, id, Durability::Consistent).unwrap();
    }
    assert_data(&store, 4);
    let exposed = store.next_event_seq();
    let snapshot = store.reader().capture().unwrap();
    store.storage().fail_next(OpClass::Read);
    assert_eq!(
        Query::Events { empty: false }.run(&snapshot),
        Err((false, ErrorKind::BackendFailed))
    );
    assert_refused(&mut store, LifecycleState::RecoveryNeeded);
    drop(snapshot);
    let (baseline, incarnation, publication, last_batch) =
        recovered(store.recover_with(|| disk.open(), restore).unwrap());
    assert_eq!(baseline, 1_u64.to_le_bytes());
    assert_eq!((incarnation, publication, last_batch), (2, 1, Some(1)));
    assert_data(&store, 1);
    let next = store.next_event_seq();
    assert!(next > exposed);
    assert_eq!(
        next,
        EventSeq {
            incarnation: 2,
            seq: 0
        }
    );
    write(&mut store, 2, Durability::Durable).unwrap();
    assert_data(&store, 2);
}

/// An I/O failure during staging never replays the rejected caller input after recovery.
#[test]
fn a_stage_failure_recovers_without_replaying_the_failed_input() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    write(&mut store, 1, Durability::Durable).unwrap();
    store.storage().fail_next(OpClass::Write);
    let error = write(&mut store, 2, Durability::Durable).unwrap_err();
    assert!(matches!(error, WriteError::Stage { .. }));
    assert!(error.not_applied());
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert_refused(&mut store, LifecycleState::RecoveryNeeded);
    let restored = recovered(store.recover_with(|| disk.open(), restore).unwrap());
    assert_eq!((restored.2, restored.3), (1, Some(1)));
    assert_data(&store, 1);
    write(&mut store, 2, Durability::Durable).unwrap();
    assert_data(&store, 2);
}

/// Snapshot/result destruction alone advances drain; idle readers, empty pages and tokens do not.
#[test]
fn recovery_defers_until_the_last_snapshot_and_result_charge_drops() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    write(&mut store, 1, Durability::Durable).unwrap();
    let reader = store.reader();
    let shared = store.shared().clone();
    let snapshot = reader.capture().unwrap();
    let another = reader.capture().unwrap();
    let token = snapshot.catalog();
    let groups = series(&snapshot);
    let page = events(&snapshot, false);
    let empty = events(&snapshot, true);
    let old = Arc::downgrade(&store.current());
    let defer = |store: &mut Tsdb<MemoryStorage>| {
        let progress = store
            .recover_with::<()>(
                || panic!("reopened before drain"),
                |_| panic!("restored early"),
            )
            .unwrap();
        assert!(matches!(progress, RecoveryProgress::WaitingForReaders));
        assert_refused(store, LifecycleState::WaitingForReaders);
        assert!(store.registry().is_empty());
        assert!(shared.storage().is_none());
        assert_eq!(store.head_reserved(), 0);
        assert_eq!(store.staging_reserved(), 0);
    };
    defer(&mut store);
    assert!(old.upgrade().is_some());
    drop(snapshot);
    defer(&mut store);
    drop(another);
    assert!(old.upgrade().is_none());
    defer(&mut store);
    drop(groups);
    defer(&mut store);
    drop(page);
    assert_eq!(shared.query_budget.reserved(), 0);
    let (_, incarnation, publication, _) =
        recovered(store.recover_with(|| disk.open(), restore).unwrap());
    assert_eq!((incarnation, publication), (2, 1));
    assert!(Arc::ptr_eq(&shared, store.shared()));
    let snapshot = reader.clone().capture().unwrap();
    assert_ne!(snapshot.catalog(), token);
    assert!(empty.events().is_empty());
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
    drop(store);
    assert!(matches!(
        reader.capture(),
        Err(CaptureError::CheckOpen {
            source: StorageError::Closed
        })
    ));
}

/// A query paused at its final checkpoint retains scratch until revocation is observed.
#[test]
fn a_paused_query_returns_closed_before_recovery_can_rebuild() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    write(&mut store, 1, Durability::Durable).unwrap();
    let shared = store.shared().clone();
    let snapshot = store.reader().capture().unwrap();
    let baseline = snapshot.reserved();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    let paused = tx.clone();
    shared.at_query("events::scan_events:2", 0, move || {
        paused.send(QueryProgress::Paused).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || {
        let result = Query::Events { empty: false }.run(&snapshot);
        tx.send(QueryProgress::Finished).unwrap();
        (result, snapshot)
    });
    let reached = matches!(rx.recv().unwrap(), QueryProgress::Paused);
    let calls = Cell::new(0);
    let progress = store.recover_with(
        || {
            calls.set(calls.get() + 1);
            disk.open()
        },
        |_| Ok(()),
    );
    if reached {
        resume.wait();
    }
    let (result, snapshot) = worker.join().unwrap();
    assert!(reached);
    assert!(matches!(
        progress.unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(result, Err((true, ErrorKind::Local)));
    assert_eq!(shared.query_budget.reserved(), baseline);
    assert_refused(&mut store, LifecycleState::WaitingForReaders);
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
    recovered(store.recover_with(|| disk.open(), restore).unwrap());
    assert_data(&store, 1);
}

/// An old read failure reports while still charged; installation cannot overtake that report.
#[test]
fn a_late_query_failure_cannot_latch_the_recovered_instance() {
    let disk = MemoryDisk::new();
    let probes = Arc::new(Probes::default());
    let mut store = Tsdb::open(
        Observed::new(disk.open().unwrap(), probes.clone()),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    write(&mut store, 1, Durability::Durable).unwrap();
    let snapshot = store.reader().capture().unwrap();
    let shared = store.shared().clone();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    let paused = tx.clone();
    *probes.fault.lock().unwrap() = Some(read_fault(ErrorKind::BackendFailed));
    probes.read.arm(move || {
        paused.send(QueryProgress::Paused).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || {
        let result = Query::Series { partial: false }.run(&snapshot);
        tx.send(QueryProgress::Finished).unwrap();
        (result, snapshot)
    });
    let reached = matches!(rx.recv().unwrap(), QueryProgress::Paused);
    let reopened = Cell::new(false);
    let restored = Cell::new(false);
    let progress = store.recover_with(
        || {
            reopened.set(true);
            disk.open()
                .map(|storage| Observed::new(storage, probes.clone()))
        },
        |_| {
            restored.set(true);
            Ok(())
        },
    );
    if reached {
        resume.wait();
    }
    let (result, snapshot) = worker.join().unwrap();
    assert!(reached);
    assert!(matches!(
        progress.unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    assert!(
        !reopened.get() && !restored.get(),
        "old failure can still report"
    );
    assert_eq!(result, Err((false, ErrorKind::BackendFailed)));
    assert_eq!(shared.registry.state(), LifecycleState::WaitingForReaders);
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
    recovered(
        store
            .recover_with(
                || {
                    disk.open()
                        .map(|storage| Observed::new(storage, probes.clone()))
                },
                restore,
            )
            .unwrap(),
    );
    write(&mut store, 2, Durability::Durable).unwrap();
    assert_data(&store, 2);
    assert_eq!(store.registry().state(), LifecycleState::Open);
}

/// Capture owns G/R through pinning; close, incarnation commit and restore own neither.
#[test]
fn capture_and_recovery_boundaries_keep_admission_closed_until_installation() {
    let disk = MemoryDisk::new();
    let probes = Arc::new(Probes::default());
    let mut store = Tsdb::open(
        Observed::new(disk.open().unwrap(), probes.clone()),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    write(&mut store, 1, Durability::Consistent).unwrap();
    let reader = store.reader();
    let shared = store.shared().clone();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    shared.registry.after_pin(move || {
        tx.send(()).unwrap();
        release.wait();
    });
    let capturing = reader.clone();
    let worker = thread::spawn(move || capturing.capture());
    rx.recv().unwrap();
    let gates_busy = !shared.gate_available() && !shared.registry.publication_available();
    resume.wait();
    let snapshot = worker.join().unwrap().unwrap();
    assert!(gates_busy);
    assert!(snapshot.reserved() > 0);
    let token = snapshot.catalog();
    let closing = shared.clone();
    let (closed, closed_rx) = mpsc::channel();
    probes.close.arm(move || {
        closed.send(boundary(&closing)).unwrap();
    });
    assert!(matches!(
        store
            .recover_with::<()>(|| panic!("capture remains"), |_| panic!("capture remains"))
            .unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    assert_eq!(
        closed_rx.try_recv().unwrap(),
        (true, true, Some(LifecycleState::Recovering))
    );
    drop(snapshot);
    assert_eq!(reader.query_reserved(), 0);
    let committing = shared.clone();
    let (committed, committed_rx) = mpsc::channel();
    let progress = store
        .recover_with(
            || {
                assert!(shared.gate_available() && shared.registry.publication_available());
                assert!(shared.registry.is_empty());
                assert_eq!(shared.query_budget.reserved(), 0);
                probes.commit.arm(move || {
                    committed.send(boundary(&committing)).unwrap();
                });
                disk.open()
                    .map(|storage| Observed::new(storage, probes.clone()))
            },
            |view| {
                assert_eq!(
                    committed_rx.try_recv().unwrap(),
                    (true, true, Some(LifecycleState::Recovering))
                );
                assert!(shared.gate_available() && shared.registry.publication_available());
                assert_eq!(shared.registry.state(), LifecycleState::Recovering);
                assert!(shared.storage().is_none() && shared.registry.is_empty());
                assert!(matches!(
                    reader.capture(),
                    Err(CaptureError::RecoveryNeeded)
                ));
                assert_eq!(
                    (view.incarnation(), view.publication(), view.last_batch()),
                    (2, 1, Some(1))
                );
                restore(view)
            },
        )
        .unwrap();
    let (baseline, incarnation, publication, last_batch) = recovered(progress);
    assert_eq!(
        baseline,
        1_u64.to_le_bytes(),
        "healthy close preserves Consistent data"
    );
    let snapshot = reader.capture().unwrap();
    let state = read_state(snapshot.storage()).unwrap();
    assert_eq!(
        (state.incarnation, state.publication, state.last_batch),
        (incarnation, publication, last_batch)
    );
    assert_eq!(snapshot.version().publication, publication);
    assert_ne!(snapshot.catalog(), token);
    drop(snapshot);
    assert_eq!(reader.query_reserved(), 0);
}

/// Each cause remains typed, all admission refuses, and only an explicit later attempt reopens.
#[test]
fn failed_reopen_and_restore_leave_unavailable_until_an_explicit_retry() {
    for kind in [
        ErrorKind::Local,
        ErrorKind::Corrupted,
        ErrorKind::BackendFailed,
    ] {
        for fail_restore in [false, true] {
            let disk = MemoryDisk::new();
            let probes = Arc::new(Probes::default());
            let mut store = Tsdb::open(
                Observed::new(disk.open().unwrap(), probes.clone()),
                db::schema(),
                Profile::desktop(),
            )
            .unwrap();
            write(&mut store, 1, Durability::Durable).unwrap();
            let shared = store.shared().clone();
            let old = Arc::downgrade(&store.current());
            let restored = Cell::new(false);
            let (closed, closed_rx) = mpsc::channel();
            let error = store
                .recover_with::<()>(
                    || {
                        if !fail_restore {
                            return Err(read_fault(kind));
                        }
                        let closing = shared.clone();
                        probes.close.arm(move || {
                            closed
                                .send((
                                    closing.gate_available(),
                                    closing.registry.publication_available(),
                                ))
                                .unwrap();
                        });
                        disk.open()
                            .map(|storage| Observed::new(storage, probes.clone()))
                    },
                    |_| {
                        restored.set(true);
                        Err(RecoveryError::Restore {
                            kind,
                            source: "host restore rejected state".into(),
                        })
                    },
                )
                .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert!(std::error::Error::source(&error).is_some());
            assert_eq!(restored.get(), fail_restore);
            assert_eq!(matches!(error, RecoveryError::Restore { .. }), fail_restore);
            if fail_restore {
                assert_eq!(closed_rx.try_recv().unwrap(), (true, true));
            }
            assert!(old.upgrade().is_none());
            assert_unavailable(&mut store);
            recovered(
                store
                    .recover_with(
                        || {
                            disk.open()
                                .map(|storage| Observed::new(storage, probes.clone()))
                        },
                        restore,
                    )
                    .unwrap(),
            );
            assert_data(&store, 1);
        }
    }
}

/// A real malformed catalog record is rejected repeatedly, with no automatic repair.
#[test]
fn open_validation_damage_stays_unavailable_until_the_host_repairs_it() {
    use crate::{layout::COMBOS, storage::StorageWrite};
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    write(&mut store, 1, Durability::Durable).unwrap();
    let (key, saved) = store
        .storage()
        .snapshot()
        .unwrap()
        .scan(COMBOS, KeyRange::All, Direction::Forward, |key, value| {
            ControlFlow::Break((key.to_vec(), value.to_vec()))
        })
        .unwrap()
        .unwrap();
    {
        let storage = store.storage();
        let mut batch = storage.batch().unwrap();
        batch.put(COMBOS, key.as_slice(), &[0xff]).unwrap();
        batch.commit(Durability::Durable).unwrap();
    }
    for _ in 0..2 {
        let error = store
            .recover_with::<()>(|| disk.open(), |_| panic!("restored an invalid catalog"))
            .unwrap_err();
        assert!(matches!(
            error,
            RecoveryError::Open {
                source: crate::OpenError::LoadCatalog { .. }
            }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        assert_unavailable(&mut store);
    }
    let result = store
        .recover_with(
            || {
                let storage = disk.open()?;
                let mut batch = storage.batch().unwrap();
                batch.put(COMBOS, key.as_slice(), &saved).unwrap();
                batch.commit(Durability::Durable).unwrap();
                Ok(storage)
            },
            restore,
        )
        .unwrap();
    recovered(result);
    assert_data(&store, 1);
}

/// Which read of a rebuilt candidate fails, independent of the old instance's retirement.
#[derive(Clone, Copy)]
enum ReadFailure {
    /// Opening the post-settlement transaction.
    Snapshot,
    /// Reading its publication, incarnation and last batch.
    State,
    /// Looking up one auxiliary value.
    Get,
    /// Scanning auxiliary values.
    Scan,
}

/// A failure at each private read boundary drops the candidate and preserves its source kind.
#[test]
fn private_restoration_reads_fail_without_publishing_a_candidate() {
    for point in [
        ReadFailure::Snapshot,
        ReadFailure::State,
        ReadFailure::Get,
        ReadFailure::Scan,
    ] {
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
            write(&mut store, 1, Durability::Durable).unwrap();
            let error = store
                .recover_with(
                    || {
                        if matches!(point, ReadFailure::Snapshot | ReadFailure::State) {
                            let next = probes.clone();
                            probes.snapshot.arm(move || {
                                let failing = next.clone();
                                next.snapshot.arm(move || match point {
                                    ReadFailure::Snapshot => {
                                        *failing.snapshot_fault.lock().unwrap() =
                                            Some(read_fault(kind));
                                    }
                                    ReadFailure::State => {
                                        *failing.fault.lock().unwrap() = Some(read_fault(kind));
                                    }
                                    _ => unreachable!(),
                                });
                            });
                        }
                        disk.open()
                            .map(|storage| Observed::new(storage, probes.clone()))
                    },
                    |view| {
                        *probes.fault.lock().unwrap() = Some(read_fault(kind));
                        match point {
                            ReadFailure::Get => view.aux_get(b"active", |_| ()),
                            ReadFailure::Scan => view
                                .aux_scan(|_, _| ControlFlow::<()>::Continue(()))
                                .map(|_| ()),
                            _ => panic!("restore ran after snapshot failure"),
                        }
                    },
                )
                .unwrap_err();
            assert!(matches!(
                (point, &error),
                (ReadFailure::Snapshot, RecoveryError::OpenSnapshot { .. })
                    | (ReadFailure::State, RecoveryError::ReadState { .. })
                    | (ReadFailure::Get, RecoveryError::ReadAux { .. })
                    | (ReadFailure::Scan, RecoveryError::ScanAux { .. })
            ));
            assert_eq!(error.kind(), kind);
            assert_unavailable(&mut store);
            recovered(
                store
                    .recover_with(
                        || {
                            disk.open()
                                .map(|storage| Observed::new(storage, probes.clone()))
                        },
                        restore,
                    )
                    .unwrap(),
            );
            assert_data(&store, 1);
        }
    }
}

/// Incarnation commit failures expose no candidate; old-owner positions still advance on retry.
#[test]
fn failed_incarnation_commits_and_restore_never_reuse_exposed_positions() {
    for outcome in [
        CommitOutcome::NotApplied,
        CommitOutcome::Unknown { applied: false },
        CommitOutcome::Unknown { applied: true },
    ] {
        let disk = MemoryDisk::new();
        let mut store = db::open(&disk);
        write(&mut store, 1, Durability::Durable).unwrap();
        let before = store.next_event_seq();
        let error = store
            .recover_with::<()>(
                || {
                    let storage = disk.open()?;
                    storage.inject_commit(outcome);
                    Ok(storage)
                },
                |_| panic!("restored after failed incarnation commit"),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            RecoveryError::Open {
                source: crate::OpenError::CommitIncarnation { .. }
            }
        ));
        assert_eq!(
            error.kind(),
            if matches!(outcome, CommitOutcome::NotApplied) {
                ErrorKind::Local
            } else {
                ErrorKind::BackendFailed
            }
        );
        assert_unavailable(&mut store);
        let during = store.next_event_seq();
        assert_eq!(during.incarnation, before.incarnation);
        assert_eq!(during.seq, before.seq + 1);
        let mut failed_incarnation = 0;
        let error = store
            .recover_with::<()>(
                || disk.open(),
                |view| {
                    failed_incarnation = view.incarnation();
                    Err(RecoveryError::Restore {
                        kind: ErrorKind::Local,
                        source: "host full".into(),
                    })
                },
            )
            .unwrap_err();
        assert!(matches!(error, RecoveryError::Restore { .. }));
        assert!(failed_incarnation > during.incarnation);
        let later = store.next_event_seq();
        assert_eq!(
            later,
            EventSeq {
                incarnation: before.incarnation,
                seq: during.seq + 1
            }
        );
        let (_, incarnation, _, _) =
            recovered(store.recover_with(|| disk.open(), restore).unwrap());
        assert!(incarnation > failed_incarnation);
        assert_eq!(
            store.next_event_seq(),
            EventSeq {
                incarnation,
                seq: 0
            }
        );
        assert_data(&store, 1);
    }
}

/// Streaming aux reads distinguish missing and empty values and stop exactly at the break key.
#[test]
fn recovery_view_streams_aux_in_key_order_without_exposing_storage() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    let ops = [
        AuxOp::Put {
            key: b"z",
            value: b"last",
        },
        AuxOp::Put {
            key: b"a",
            value: b"",
        },
        AuxOp::Put {
            key: b"m",
            value: b"middle",
        },
        AuxOp::Put {
            key: b"removed",
            value: b"old",
        },
        AuxOp::Delete { key: b"removed" },
    ];
    store
        .write(
            &crate::Input {
                aux: &ops,
                ..db::batch(7, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let reader = store.reader();
    let result = recovered(
        store
            .recover_with(
                || disk.open(),
                |view| {
                    assert_eq!(
                        (view.incarnation(), view.publication(), view.last_batch()),
                        (2, 1, Some(7))
                    );
                    assert_eq!(
                        view.aux_get(b"removed", |value| value.map(<[u8]>::to_vec))?,
                        None
                    );
                    assert_eq!(
                        view.aux_get(b"a", |value| value.map(<[u8]>::to_vec))?,
                        Some(Vec::new())
                    );
                    let mut visited = Vec::new();
                    let stop = view.aux_scan(|key, value| {
                        visited.push((key.to_vec(), value.to_vec()));
                        if key == b"m" {
                            ControlFlow::Break(23)
                        } else {
                            ControlFlow::Continue(())
                        }
                    })?;
                    assert_eq!(stop, Some(23));
                    assert_eq!(
                        visited,
                        vec![(b"a".to_vec(), vec![]), (b"m".to_vec(), b"middle".to_vec())]
                    );
                    let mut all = Vec::new();
                    assert_eq!(
                        view.aux_scan(|key, _| {
                            all.push(key.to_vec());
                            ControlFlow::<()>::Continue(())
                        })?,
                        None
                    );
                    assert_eq!(reader.query_reserved(), 0);
                    Ok(all)
                },
            )
            .unwrap(),
    );
    assert_eq!(result.0, vec![b"a".to_vec(), b"m".to_vec(), b"z".to_vec()]);
    assert_eq!(reader.query_reserved(), 0);
    assert_eq!(reader.capture().unwrap().version().publication, 1);
}

/// Real file ownership closes during waiting, and old Reader clones capture the recovered file.
#[cfg(feature = "redb")]
#[test]
fn redb_recovery_reopens_the_same_file_after_charged_results_drop() {
    use crate::storage::redb::RedbStorage;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recover.redb");
    let mut store = Tsdb::open(
        RedbStorage::open(&path, 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    write(&mut store, 1, Durability::Consistent).unwrap();
    let reader = store.reader();
    let snapshot = reader.capture().unwrap();
    let token = snapshot.catalog();
    let groups = series(&snapshot);
    drop(snapshot);
    assert!(matches!(
        store
            .recover_with::<()>(|| panic!("result remains"), |_| panic!("result remains"),)
            .unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    // Opening this file independently proves retirement released the file lock while deferred.
    drop(RedbStorage::open(&path, 4 << 20).unwrap());
    drop(groups);
    assert_eq!(reader.query_reserved(), 0);
    let (baseline, incarnation, publication, last_batch) = recovered(
        store
            .recover_with(
                || RedbStorage::open(&path, 4 << 20),
                |view| {
                    assert!(matches!(
                        reader.capture(),
                        Err(CaptureError::RecoveryNeeded)
                    ));
                    restore(view)
                },
            )
            .unwrap(),
    );
    assert_eq!(baseline, 1_u64.to_le_bytes());
    assert_eq!((incarnation, publication, last_batch), (2, 1, Some(1)));
    assert_ne!(reader.capture().unwrap().catalog(), token);
    assert_data(&store, 1);
    write(&mut store, 2, Durability::Durable).unwrap();
    assert_data(&store, 2);
}

/// Closing and the fresh durable incarnation both sync with capture/publication locks free.
#[cfg(feature = "redb")]
#[test]
fn redb_recovery_close_and_incarnation_fsync_hold_neither_shared_lock() {
    use super::support::{Hook, SyncDisk};
    use crate::storage::redb::RedbStorage;
    let sync = Arc::new(Hook::default());
    let disk = SyncDisk::new(sync.clone());
    let mut store = Tsdb::open(
        RedbStorage::open_with_backend(disk.clone(), 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    write(&mut store, 1, Durability::Consistent).unwrap();
    let shared = store.shared().clone();
    let reader = store.reader();
    let closing = shared.clone();
    let (closed, closed_rx) = mpsc::channel();
    sync.arm(move || {
        closed.send(boundary(&closing)).unwrap();
    });
    let committing = shared.clone();
    let (committed, committed_rx) = mpsc::channel();
    let progress = store
        .recover_with(
            || {
                assert_eq!(
                    closed_rx.try_recv().unwrap(),
                    (true, true, Some(LifecycleState::Recovering))
                );
                assert!(shared.storage().is_none() && shared.registry.is_empty());
                let storage = RedbStorage::open_with_backend(disk, 4 << 20)?;
                sync.arm(move || {
                    committed.send(boundary(&committing)).unwrap();
                });
                Ok(storage)
            },
            |view| {
                assert_eq!(
                    committed_rx.try_recv().unwrap(),
                    (true, true, Some(LifecycleState::Recovering))
                );
                assert!(matches!(
                    reader.capture(),
                    Err(CaptureError::RecoveryNeeded)
                ));
                assert_eq!(
                    (view.incarnation(), view.publication(), view.last_batch()),
                    (2, 1, Some(1))
                );
                restore(view)
            },
        )
        .unwrap();
    assert_eq!(
        recovered(progress),
        (1_u64.to_le_bytes().to_vec(), 2, 1, Some(1))
    );
    assert_data(&store, 1);
    assert_eq!(reader.query_reserved(), 0);
}
