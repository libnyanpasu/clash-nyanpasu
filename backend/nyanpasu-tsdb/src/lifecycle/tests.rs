use super::*;
use crate::{
    db::tests::{self as db, LABELS, LOGS},
    storage::memory::MemoryDisk,
    write::{EventRow, Value},
};

/// Replay measures retained children beside live construction leases on both opens.
#[test]
fn populated_open_and_reopen_stay_within_their_admitted_plans() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    let values = [Value::Bytes(b"retained journal row")];
    for id in 1..=24 {
        let points = [db::point(LABELS, &[1, 2])];
        let events = [EventRow {
            stream: LOGS,
            key: id as i64,
            seq: store.next_event_seq(),
            values: &values,
        }];
        store
            .write(&db::batch(id, &points, &events), Durability::Durable)
            .unwrap();
    }
    let expected = store.current().head.clone();
    drop(store.into_storage());
    for _ in 0..2 {
        let opened = open(disk.open().unwrap(), &db::schema(), &Profile::desktop()).unwrap();
        assert_eq!(opened.version.head, expected);
        for (planned, observed) in opened.allocation_peaks {
            assert!(
                observed <= planned,
                "observed {observed}, admitted {planned}"
            );
        }
        assert!(opened.allocation_peaks[0].1 >= expected.bytes());
        assert_eq!(opened.head_budget.reserved(), opened.version.head.bytes());
        assert_eq!(opened.staging_budget.reserved(), 0);
        drop(opened);
    }
}

/// Left-out replay sources build within the opening plan at the unchanged staging floor.
#[test]
fn opening_left_out_sources_observes_the_build_peak_at_minimum_staging() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    for id in 1..=12 {
        db::write_simple(&mut store, id, Durability::Durable);
    }
    drop(store.into_storage());
    let schema = db::schema();
    let shape = Shape::new(&schema);
    let head = Head::empty(schema.events.len()).bytes() + 64;
    let profile = Profile {
        head,
        event_buffer_bytes: 1,
        staging: least_staging(&schema, &shape, head),
        ..Profile::desktop()
    };
    for first in [true, false] {
        let opened = open(disk.open().unwrap(), &schema, &profile).unwrap();
        for (planned, observed) in opened.allocation_peaks {
            assert!(
                observed <= planned,
                "observed {observed}, admitted {planned}"
            );
        }
        if first {
            assert!(
                opened.allocation_peaks[1].1 > 0,
                "left-out records were built"
            );
        }
        assert!(opened.version.head.bytes() <= profile.head);
        assert_eq!(opened.staging_budget.reserved(), 0);
        drop(opened);
    }
}

/// Settlement measures charged execution beside its planning arrays and retained head.
#[test]
fn opening_journal_settlement_observes_the_execution_peak() {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    for id in 1..=100 {
        let points = [db::point(LABELS, &[1, 2])];
        let mut input = db::batch(id, &points, &[]);
        input.now = crate::Timestamp(0);
        store.write(&input, Durability::Durable).unwrap();
    }
    let expected = store.current().head.clone();
    drop(store.into_storage());
    let profile = Profile {
        journal_bytes: 128,
        ..Profile::desktop()
    };
    for first in [true, false] {
        let opened = open(disk.open().unwrap(), &db::schema(), &profile).unwrap();
        for (planned, observed) in opened.allocation_peaks {
            assert!(
                observed <= planned,
                "observed {observed}, admitted {planned}"
            );
        }
        if first {
            assert!(opened.allocation_peaks[2].1 > 0, "settlement was built");
        }
        assert_eq!(
            opened.version.head.series().count(),
            expected.series().count()
        );
        assert_eq!(opened.staging_budget.reserved(), 0);
        drop(opened);
    }
}

/// The query forms exercised against exactly the same captured publication.
#[derive(Clone, Copy)]
enum Query {
    /// A grouped series scan, optionally excluding later buckets to read stats.
    Series { partial: bool },
    /// An event page, optionally empty without entering its merge.
    Events { empty: bool },
    /// A head scan whose filter rejects every row, including complete batches.
    RejectedEvents,
}

impl Query {
    /// Runs the public query and retains only its error classification.
    fn run<S: Storage>(self, snapshot: &crate::Snapshot<S>) -> Result<(), (bool, ErrorKind)> {
        match self {
            Self::Series { partial } => snapshot
                .series(&crate::SeriesQuery {
                    range: crate::TimeRange::new(
                        crate::Timestamp(0),
                        crate::Timestamp(if partial {
                            db::HOUR_MS
                        } else {
                            100 * db::HOUR_MS
                        }),
                    )
                    .unwrap(),
                    tier: db::HOUR,
                    requests: &[crate::GroupRequest {
                        filters: &[],
                        key: crate::GroupKey::Total,
                    }],
                    fields: crate::FieldMask(3),
                })
                .map(drop)
                .map_err(|error| {
                    (
                        matches!(
                            error,
                            crate::SeriesQueryError::CheckOpen {
                                source: crate::storage::StorageError::Closed
                            }
                        ),
                        error.kind(),
                    )
                }),
            Self::Events { .. } | Self::RejectedEvents => snapshot
                .scan_events(&crate::EventQuery {
                    stream: LOGS,
                    range: crate::TimeRange::new(
                        crate::Timestamp(0),
                        crate::Timestamp(if matches!(self, Self::Events { empty: true }) {
                            0
                        } else {
                            i64::MAX
                        }),
                    )
                    .unwrap(),
                    direction: crate::storage::Direction::Forward,
                    cursor: None,
                    predicate: &crate::EventPredicate {
                        filters: if matches!(self, Self::RejectedEvents) {
                            &[crate::EventFilter {
                                column: 0,
                                condition: crate::EventCondition::Contains(b"absent"),
                            }]
                        } else {
                            &[]
                        },
                    },
                    limit: 10_000,
                })
                .map(drop)
                .map_err(|error| {
                    (
                        matches!(
                            error,
                            crate::EventScanError::CheckOpen {
                                source: crate::storage::StorageError::Closed
                            }
                        ),
                        error.kind(),
                    )
                }),
        }
    }
}

/// Builds both chunk and head rows, including a multi-piece event value.
fn query_fixture() -> crate::Tsdb<crate::storage::memory::MemoryStorage> {
    let disk = MemoryDisk::new();
    let mut store = db::open(&disk);
    let points = [0, 2].map(|hour| crate::SeriesPoint {
        at: crate::Timestamp(hour * db::HOUR_MS),
        ..db::point(LABELS, &[1, 2])
    });
    let text = vec![b'x'; 160_000];
    let values = [Value::Bytes(&text)];
    let events = [EventRow {
        stream: LOGS,
        key: 1,
        seq: store.next_event_seq(),
        values: &values,
    }];
    store
        .write(&db::batch(1, &points, &events), Durability::Durable)
        .unwrap();
    let mut flush = db::batch(2, &[], &[]);
    flush.now = crate::Timestamp(48 * db::HOUR_MS);
    store.write(&flush, Durability::Durable).unwrap();
    store
        .maintain(&crate::Maintain {
            now: flush.now,
            retention: &crate::RetentionPolicy {
                tiers: vec![crate::Retention::default(); 2],
                streams: vec![crate::Retention::default(); 2],
            },
            rounds: 10,
        })
        .unwrap();
    db::write_simple(&mut store, 3, Durability::Durable);
    store
}

/// A boundary notification also reports completion, so a missing checkpoint cannot hang a test.
#[derive(Debug)]
enum QueryProgress {
    /// The armed boundary has been reached and is waiting on the resume barrier.
    Paused,
    /// The query has returned; reaching this first means the target boundary was skipped.
    Finished,
}

/// Orders retirement at one checkpoint and rejects continuing as far as any later checkpoint.
fn revoke_at(point: &'static str, skip: usize, query: Query) {
    revoke_in(query_fixture(), point, skip, query);
}

/// Runs the same retirement protocol against a caller-selected source shape.
fn revoke_in(
    store: crate::Tsdb<crate::storage::memory::MemoryStorage>,
    point: &'static str,
    skip: usize,
    query: Query,
) {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };
    let shared = store.shared().clone();
    let snapshot = store.reader().capture().unwrap();
    let baseline = shared.query_budget.reserved();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    let paused = tx.clone();
    shared.at_query(point, skip, move || {
        paused.send(QueryProgress::Paused).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || {
        let result = query.run(&snapshot);
        tx.send(QueryProgress::Finished).unwrap();
        (result, snapshot)
    });
    let reached = matches!(rx.recv().unwrap(), QueryProgress::Paused);
    let gate_free = shared.gate_available();
    let registry_free = shared.registry.publication_available();
    let mut store = Some(store);
    if reached {
        // A failed lock probe must not retire while the worker still owns that lock.
        if gate_free && registry_free {
            drop(store.take());
            shared.at_query("*", 0, move || {
                panic!("continued past revoked checkpoint {point}")
            });
        }
        resume.wait();
    }
    let (result, snapshot) = worker.join().unwrap();
    assert!(reached, "checkpoint {point} was never reached");
    assert!(gate_free, "{point} retained G");
    assert!(registry_free, "{point} retained R");
    assert_eq!(result, Err((true, ErrorKind::Local)), "{point}");
    assert_eq!(
        shared.query_budget.reserved(),
        baseline,
        "scratch leaked at {point}"
    );
    drop(snapshot);
    assert_eq!(
        shared.query_budget.reserved(),
        0,
        "capture leaked at {point}"
    );
}

/// Each series checkpoint pauses before its load; retirement must stop at that exact boundary.
#[test]
fn series_revocation_stops_at_every_checkpoint() {
    for point in [
        "series::series:1",
        "series::series:2",
        "series::chunks:1",
        "series::chunk:1",
        "series::chunk:2",
        "series::value:1",
        "series::value:2",
        "series::head:1",
        "series::head:2",
    ] {
        revoke_at(point, 0, Query::Series { partial: true });
    }
}

/// Covers decoded and cached rows, stripe refills, multi-piece reads, and empty final returns.
#[test]
fn event_revocation_stops_at_every_checkpoint() {
    for point in [
        "events::scan_events:1",
        "events::scan_events:2",
        "events::list_chunks:1",
        "events::chain:1",
        "events::chain:2",
        "events::chain:3",
        "events::chain:4",
        "events::chain:5",
        "events::chain:6",
        "events::merge:1",
        "events::merge:2",
        "events::merge:3",
        "events::select_head:1",
        "events::select_head:2",
        "events::open:1",
        "events::load_stripe:1",
        "events::read:1",
        "events::read:2",
        "events::materialize:1",
        "events::materialize:2",
        "events::materialize:3",
        "events::materialize:4",
        "events::read_large:1",
        "events::read_large:2",
        "events::read_large:3",
    ] {
        revoke_at(point, 0, Query::Events { empty: false });
    }
    revoke_at("events::read_large:2", 1, Query::Events { empty: false });
    revoke_at("events::scan_events:2", 0, Query::Events { empty: true });
}

/// A paused capture excludes retirement, then completes with credit before the owner closes.
#[test]
fn capture_gate_orders_retirement_and_reader_clones_cannot_keep_the_backend() {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };
    let disk = MemoryDisk::new();
    let store = db::open(&disk);
    let shared = store.shared().clone();
    let reader = store.reader();
    let clone = reader.clone();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    shared.registry.after_snapshot(move || {
        tx.send(()).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || reader.capture());
    rx.recv().unwrap();
    let gate_busy = !shared.gate_available();
    let registry_busy = !shared.registry.publication_available();
    resume.wait();
    let snapshot = worker.join().unwrap().unwrap();
    assert!(gate_busy && registry_busy);
    assert!(
        snapshot.reserved() > 0,
        "even an empty version has fixed graphs"
    );
    let token = snapshot.catalog();
    drop(store);
    assert!(shared.gate_available());
    assert!(shared.registry.publication_available());
    assert!(matches!(
        clone.capture(),
        Err(crate::CaptureError::CheckOpen {
            source: crate::storage::StorageError::Closed
        })
    ));
    let reopened = db::open(&disk);
    assert_ne!(reopened.reader().capture().unwrap().catalog(), token);
    assert_eq!(shared.query_budget.reserved(), snapshot.reserved());
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
    shared.report(ErrorKind::BackendFailed);
    assert!(
        !shared.registry.needs_recovery(),
        "late failure resurrected Closed"
    );
}

/// Capture and retained result charges are the only retirement drain condition.
#[test]
fn snapshots_and_results_hold_credit_until_their_last_drop() {
    let store = query_fixture();
    let reader = store.reader();
    let shared = store.shared().clone();
    let snapshot = reader.capture().unwrap();
    let token = snapshot.catalog();
    let series = snapshot
        .series(&crate::SeriesQuery {
            range: crate::TimeRange::new(crate::Timestamp(0), crate::Timestamp(db::HOUR_MS))
                .unwrap(),
            tier: db::HOUR,
            requests: &[crate::GroupRequest {
                filters: &[],
                key: crate::GroupKey::Total,
            }],
            fields: crate::FieldMask(3),
        })
        .unwrap();
    let query = crate::EventQuery {
        stream: LOGS,
        range: crate::TimeRange::new(crate::Timestamp(0), crate::Timestamp(10)).unwrap(),
        direction: crate::storage::Direction::Forward,
        cursor: None,
        predicate: &crate::EventPredicate::default(),
        limit: 10,
    };
    let events = snapshot.scan_events(&query).unwrap();
    let empty = snapshot
        .scan_events(&crate::EventQuery {
            range: crate::TimeRange::new(crate::Timestamp(0), crate::Timestamp(0)).unwrap(),
            ..query
        })
        .unwrap();
    let another = reader.capture().unwrap();
    drop(store);
    drop(snapshot);
    assert!(shared.query_budget.reserved() >= another.reserved());
    drop(another);
    assert!(shared.query_budget.reserved() > 0);
    assert!(!series.lists().is_empty());
    assert!(!events.events().is_empty());
    drop(series);
    assert!(shared.query_budget.reserved() > 0);
    drop(events);
    assert_eq!(shared.query_budget.reserved(), 0);
    assert!(empty.events().is_empty());
    drop((empty, reader, token));
    assert_eq!(shared.query_budget.reserved(), 0);
}

mod compaction;
mod recovery;
pub(crate) mod support;
mod suspend;

/// A valid no-expiry maintenance call used to distinguish lifecycle refusal from validation.
fn maintenance() -> crate::RetentionPolicy {
    crate::RetentionPolicy {
        tiers: vec![crate::Retention::default(); 2],
        streams: vec![crate::Retention::default(); 2],
    }
}

/// Injected errors exercise the facade independently of an engine's own error latch.
fn read_fault(kind: ErrorKind) -> crate::storage::StorageError {
    match kind {
        ErrorKind::BackendFailed => crate::storage::StorageError::Failed {
            source: "injected query I/O failure".into(),
        },
        ErrorKind::Local => crate::storage::StorageError::Limit {
            detail: "injected limit".into(),
        },
        ErrorKind::Corrupted => crate::storage::StorageError::Corrupted {
            detail: "injected damage".into(),
        },
    }
}

/// Query completion orders the next owner admission; only BackendFailed may refuse it.
#[test]
fn query_failures_latch_only_backend_failed_before_the_next_owner_batch() {
    use std::sync::Arc;
    use support::{Observed, Probes};
    for query in [
        Query::Series { partial: false },
        Query::Events { empty: false },
    ] {
        for kind in [
            ErrorKind::BackendFailed,
            ErrorKind::Local,
            ErrorKind::Corrupted,
        ] {
            let probes = Arc::new(Probes::default());
            let mut store = crate::Tsdb::open(
                Observed::new(MemoryDisk::new().open().unwrap(), probes.clone()),
                db::schema(),
                Profile::desktop(),
            )
            .unwrap();
            db::write_simple(&mut store, 1, Durability::Durable);
            let shared = store.shared().clone();
            let snapshot = store.reader().capture().unwrap();
            *probes.fault.lock().unwrap() = Some(read_fault(kind));
            assert_eq!(query.run(&snapshot), Err((false, kind)));
            assert_eq!(
                shared.registry.needs_recovery(),
                kind == ErrorKind::BackendFailed
            );
            if kind == ErrorKind::BackendFailed {
                probes
                    .batch
                    .arm(|| panic!("opened a batch after query failure"));
                assert!(matches!(
                    store.write(&db::batch(2, &[], &[]), Durability::Durable),
                    Err(crate::WriteError::RecoveryNeeded)
                ));
                assert!(matches!(
                    store.maintain(&crate::Maintain {
                        now: crate::Timestamp(0),
                        retention: &maintenance(),
                        rounds: 1,
                    }),
                    Err(crate::MaintainError::RecoveryNeeded)
                ));
            } else {
                db::write_simple(&mut store, 2, Durability::Durable);
                store
                    .maintain(&crate::Maintain {
                        now: crate::Timestamp(0),
                        retention: &maintenance(),
                        rounds: 1,
                    })
                    .unwrap();
            }
            assert_eq!(shared.query_budget.reserved(), snapshot.reserved());
            drop(snapshot);
            assert_eq!(shared.query_budget.reserved(), 0);
        }
    }
}

/// The capture read unwinds R before latching a failure; damaged and local captures do not latch.
#[test]
fn capture_failures_report_after_releasing_the_registry_guard() {
    use std::sync::Arc;
    use support::{Observed, Probes};
    for kind in [
        ErrorKind::BackendFailed,
        ErrorKind::Local,
        ErrorKind::Corrupted,
    ] {
        let probes = Arc::new(Probes::default());
        let store = crate::Tsdb::open(
            Observed::new(MemoryDisk::new().open().unwrap(), probes.clone()),
            db::schema(),
            Profile::desktop(),
        )
        .unwrap();
        *probes.fault.lock().unwrap() = Some(read_fault(kind));
        let error = store.reader().capture().map(drop).unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(
            store.registry().needs_recovery(),
            kind == ErrorKind::BackendFailed
        );
        assert!(store.registry().publication_available());
        assert_eq!(store.shared().query_budget.reserved(), 0);
    }
}

/// The commit barrier permits a reader's failure report, and publication must preserve its latch.
#[test]
fn commit_and_close_release_registry_locks_and_publication_preserves_a_query_latch() {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };
    use support::{Observed, Probes};
    let probes = Arc::new(Probes::default());
    let mut store = crate::Tsdb::open(
        Observed::new(MemoryDisk::new().open().unwrap(), probes.clone()),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    let shared = store.shared().clone();
    let snapshot = store.reader().capture().unwrap();
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    probes.commit.arm(move || {
        tx.send(()).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || {
        let written = store.write(&db::batch(1, &[], &[]), Durability::Durable);
        (store, written)
    });
    rx.recv().unwrap();
    let gate_busy = !shared.gate_available();
    let registry_free = shared.registry.publication_available();
    *probes.fault.lock().unwrap() = Some(read_fault(ErrorKind::BackendFailed));
    let failed = registry_free.then(|| Query::Series { partial: false }.run(&snapshot));
    resume.wait();
    let (mut store, written) = worker.join().unwrap();
    assert!(gate_busy && registry_free);
    assert_eq!(failed, Some(Err((false, ErrorKind::BackendFailed))));
    assert_eq!(written.unwrap().publication, 1);
    assert!(shared.registry.needs_recovery());
    probes
        .batch
        .arm(|| panic!("publication lost the concurrent failure latch"));
    assert!(matches!(
        store.write(&db::batch(2, &[], &[]), Durability::Durable),
        Err(crate::WriteError::RecoveryNeeded)
    ));
    let closing = shared.clone();
    let (closed_tx, closed_rx) = mpsc::channel();
    probes.close.arm(move || {
        closed_tx
            .send((
                closing.gate_available(),
                closing.registry.publication_available(),
            ))
            .unwrap();
    });
    drop(store);
    assert_eq!(closed_rx.recv().unwrap(), (true, true));
    assert_eq!(shared.query_budget.reserved(), snapshot.reserved());
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
}

/// The transaction-drop barrier must still observe capture credit after the real transaction dies.
#[test]
fn snapshot_drops_its_transaction_before_returning_capture_credit() {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };
    use support::{Observed, Probes};
    let probes = Arc::new(Probes::default());
    let store = crate::Tsdb::open(
        Observed::new(MemoryDisk::new().open().unwrap(), probes.clone()),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    let shared = store.shared().clone();
    let snapshot = store.reader().capture().unwrap();
    let bytes = snapshot.reserved();
    assert!(bytes > 0);
    drop(store);
    let (tx, rx) = mpsc::channel();
    let resume = Arc::new(Barrier::new(2));
    let release = resume.clone();
    probes.snapshot_dropped.arm(move || {
        tx.send(()).unwrap();
        release.wait();
    });
    let worker = thread::spawn(move || drop(snapshot));
    rx.recv().unwrap();
    let during_drop = shared.query_budget.reserved();
    resume.wait();
    worker.join().unwrap();
    assert_eq!(during_drop, bytes);
    assert_eq!(shared.query_budget.reserved(), 0);
}

/// Real file reopening succeeds after owner Drop while old readers, snapshots and results live.
#[cfg(feature = "redb")]
#[test]
fn redb_retirement_releases_the_file_with_old_readers_and_results_alive() {
    use crate::storage::redb::RedbStorage;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retirement.redb");
    let mut store = crate::Tsdb::open(
        RedbStorage::open(&path, 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    db::write_simple(&mut store, 1, Durability::Consistent);
    let reader = store.reader();
    let snapshot = reader.capture().unwrap();
    let shared = store.shared().clone();
    let result = snapshot
        .series(&crate::SeriesQuery {
            range: crate::TimeRange::new(crate::Timestamp(0), crate::Timestamp(db::HOUR_MS))
                .unwrap(),
            tier: db::HOUR,
            requests: &[crate::GroupRequest {
                filters: &[],
                key: crate::GroupKey::Total,
            }],
            fields: crate::FieldMask(3),
        })
        .unwrap();
    drop(store);
    let reopened = crate::Tsdb::open(
        RedbStorage::open(&path, 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    assert_eq!(reopened.current().publication, 1);
    assert!(matches!(
        reader.capture(),
        Err(crate::CaptureError::CheckOpen { .. })
    ));
    assert_eq!(
        Query::Series { partial: false }.run(&snapshot),
        Err((true, ErrorKind::Local))
    );
    assert_eq!(
        Query::Events { empty: false }.run(&snapshot),
        Err((true, ErrorKind::Local))
    );
    drop(snapshot);
    assert!(shared.query_budget.reserved() > 0);
    assert_eq!(result.lists()[0].iter().next().unwrap().sums, &[1, 2]);
    drop(result);
    assert_eq!(shared.query_budget.reserved(), 0);
}

/// Sync callbacks order try-write probes inside a durable commit and the clean-close flush.
#[cfg(feature = "redb")]
#[test]
fn redb_fsync_holds_no_registry_guard_and_close_holds_neither_gate() {
    use crate::storage::redb::RedbStorage;
    use std::sync::{Arc, mpsc};
    use support::{Hook, SyncDisk};
    let sync = Arc::new(Hook::default());
    let mut store = crate::Tsdb::open(
        RedbStorage::open_with_backend(SyncDisk::new(sync.clone()), 4 << 20).unwrap(),
        db::schema(),
        Profile::desktop(),
    )
    .unwrap();
    let shared = store.shared().clone();
    let during_commit = shared.clone();
    let (tx, rx) = mpsc::channel();
    sync.arm(move || {
        tx.send((
            during_commit.gate_available(),
            during_commit.registry.publication_available(),
        ))
        .unwrap();
    });
    db::write_simple(&mut store, 1, Durability::Durable);
    assert_eq!(rx.try_recv().unwrap(), (false, true));
    db::write_simple(&mut store, 2, Durability::Consistent);
    let during_close = shared.clone();
    let (tx, rx) = mpsc::channel();
    sync.arm(move || {
        tx.send((
            during_close.gate_available(),
            during_close.registry.publication_available(),
        ))
        .unwrap();
    });
    drop(store);
    assert_eq!(rx.try_recv().unwrap(), (true, true));
}

/// Row-batch checks also cover cached head rows and a predicate rejecting every visited row.
#[test]
fn head_only_queries_check_later_batches_even_when_filters_reject_every_row() {
    for (point, query) in [
        ("events::select_head:2", Query::RejectedEvents),
        ("events::merge:3", Query::Events { empty: false }),
        ("events::materialize:1", Query::Events { empty: false }),
    ] {
        let disk = MemoryDisk::new();
        let mut store = db::open_with(
            &disk,
            Profile {
                event_buffer_rows: 20_000,
                event_buffer_bytes: 8 << 20,
                ..Profile::desktop()
            },
        );
        let values = [Value::Bytes(b"head row")];
        let rows: Vec<_> = (0..5_000)
            .map(|key| EventRow {
                stream: LOGS,
                key,
                seq: store.next_event_seq(),
                values: &values,
            })
            .collect();
        store
            .write(&db::batch(1, &[], &rows), Durability::Durable)
            .unwrap();
        assert_eq!(store.current().head.events(LOGS).rows(), 5_000);
        revoke_in(store, point, 1, query);
    }
}

/// A read paused inside the storage port sees retirement before returning its selected error.
#[test]
fn storage_read_errors_after_retirement_are_closed_except_backend_failure() {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };
    use support::{Observed, Probes};
    for query in [
        Query::Series { partial: false },
        Query::Events { empty: false },
    ] {
        for fault in 0..4 {
            for retire in [false, true] {
                let probes = Arc::new(Probes::default());
                let store = crate::Tsdb::open(
                    Observed::new(MemoryDisk::new().open().unwrap(), probes.clone()),
                    db::schema(),
                    Profile::desktop(),
                )
                .unwrap();
                let shared = store.shared().clone();
                let snapshot = store.reader().capture().unwrap();
                let baseline = snapshot.reserved();
                let error = match fault {
                    0 => crate::storage::StorageError::Closed,
                    1 => read_fault(ErrorKind::Local),
                    2 => read_fault(ErrorKind::Corrupted),
                    _ => read_fault(ErrorKind::BackendFailed),
                };
                let kind = error.kind();
                *probes.fault.lock().unwrap() = Some(error);
                let (tx, rx) = mpsc::channel();
                let resume = Arc::new(Barrier::new(2));
                let release = resume.clone();
                let paused = tx.clone();
                probes.read.arm(move || {
                    paused.send(QueryProgress::Paused).unwrap();
                    release.wait();
                });
                let worker = thread::spawn(move || {
                    let result = query.run(&snapshot);
                    tx.send(QueryProgress::Finished).unwrap();
                    (result, snapshot)
                });
                let reached = matches!(rx.recv().unwrap(), QueryProgress::Paused);
                let gate_free = shared.gate_available();
                let registry_free = shared.registry.publication_available();
                let mut store = Some(store);
                if reached {
                    if retire && gate_free && registry_free {
                        drop(store.take());
                    }
                    resume.wait();
                }
                let (result, snapshot) = worker.join().unwrap();
                assert!(reached && gate_free && registry_free);
                let normalized = retire && kind != ErrorKind::BackendFailed;
                assert_eq!(
                    result,
                    Err((normalized, if normalized { ErrorKind::Local } else { kind }))
                );
                assert_eq!(
                    shared.registry.needs_recovery(),
                    !retire && kind == ErrorKind::BackendFailed
                );
                assert_eq!(shared.query_budget.reserved(), baseline);
                drop(snapshot);
                assert_eq!(shared.query_budget.reserved(), 0);
            }
        }
    }
}
