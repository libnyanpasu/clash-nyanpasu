//! Public-facade work, with preparation and decoded verification outside timing.

use super::{
    fixture::{DAY, Fixture, Scale, Shape},
    output::{Context, Samples},
    tsdb::{
        storage::{Direction, Durability, StorageError},
        *,
    },
};
use std::hint::black_box;

/// Engine-specific reopening and physical observations; no hidden shared store handle.
pub struct Engine<F, O> {
    /// Stable engine label.
    pub name: &'static str,
    /// Opens the same private synthetic dataset after retirement.
    pub reopen: F,
    /// File length, successful write bytes, successful syncs and backend closes.
    pub observe: O,
}
/// Runs one synthetic day; warmup uses a separate disposable fixture in the caller.
pub fn day<S: CompactStorage>(
    fixture: &Fixture,
    scale: Scale,
    seed: u64,
    durability: Durability,
    read_samples: usize,
    engine: Engine<impl Fn() -> Result<S, StorageError>, impl Fn() -> (u64, u64, u64, u64)>,
) -> String {
    let mut samples = Samples::default();
    let schema = fixture.shape.schema();
    let profile = fixture.shape.profile();
    let mut db = samples.time("open-empty", 1, || {
        Tsdb::open((engine.reopen)().unwrap(), schema, profile).unwrap()
    });
    let policy = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    let reader = db.reader();
    let mut retained = None;
    let mut writes_while_retained = 0;
    let mut rounds_while_retained = 0;
    for i in 0..fixture.batches() {
        let batch = fixture.batch(&mut db, i);
        batch.write(&mut db, durability, |db, input, d| {
            samples.time(
                "write",
                (input.series.len() + input.events.len()) as u64,
                || db.write(black_box(input), d).unwrap(),
            )
        });
        writes_while_retained += usize::from(retained.is_some());
        if i == 0 && fixture.shape.retained {
            retained = Some(reader.capture().unwrap());
        }
        if i % 16 == 15 && !fixture.shape.stalled {
            round(&mut db, &policy, DAY + fixture.shape.late_ms, &mut samples);
            rounds_while_retained += usize::from(retained.is_some());
        }
    }
    // Verification and query warmup precede query sampling and never contribute samples.
    fixture.verify(&db);
    reads(
        &db,
        fixture.range(),
        fixture.points.len() as u64,
        fixture.rows.len() as u64,
        read_samples,
        &mut samples,
    );
    if fixture.shape.retained {
        assert!(retained.is_some());
        assert!(writes_while_retained > 0);
        assert!(rounds_while_retained > 0);
    }
    drop(retained.take());
    let replay_bytes = db.status().unwrap().data().unwrap().journal_bytes;
    samples.time("suspend-for-replay", 1, || {
        db.suspend(Timestamp(2 * DAY)).unwrap()
    });
    drop(db);
    let schema = fixture.shape.schema();
    let profile = fixture.shape.profile();
    let mut db = samples.time("open-replay", 1, || {
        Tsdb::open((engine.reopen)().unwrap(), schema, profile).unwrap()
    });
    fixture.verify(&db);
    let reader = db.reader();
    for _ in 0..10000 {
        if !round(
            &mut db,
            &policy,
            2 * DAY + fixture.shape.late_ms,
            &mut samples,
        ) {
            break;
        }
    }
    assert!(!db.status().unwrap().data().unwrap().maintenance.behind);
    fixture.verify(&db);
    let old = reader.capture().unwrap();
    let closes = (engine.observe)().3;
    samples.time("suspend-actual-close", 1, || {
        db.suspend(Timestamp(2 * DAY)).unwrap()
    });
    if engine.name == "redb" {
        assert_eq!((engine.observe)().3, closes + 1);
    }
    let waiting = samples.time("recover-waiting", 1, || {
        db.recover_with(&engine.reopen, |_| Ok(())).unwrap()
    });
    assert!(matches!(waiting, RecoveryProgress::WaitingForReaders));
    drop(old);
    let progress = samples.time("recover-restore", 1, || {
        db.recover_with(&engine.reopen, |view| {
            view.aux_get(b"synthetic-host", |bytes| {
                bytes.map(|v| u64::from_le_bytes(v.try_into().unwrap()))
            })
        })
        .unwrap()
    });
    match progress {
        RecoveryProgress::Recovered { restored, .. } => {
            assert_eq!(restored, Some(fixture.batches() as u64))
        }
        RecoveryProgress::WaitingForReaders => panic!("released fixture still held"),
    }
    fixture.verify(&db);
    if fixture.shape.churn {
        churn(&mut db, fixture, seed, &mut samples);
    }
    black_box(samples.time("compact", 1, || db.compact().unwrap()));
    let status = db.status().unwrap();
    let data = status.data().unwrap();
    let logical_bytes = data.streams().iter().map(|s| s.bytes).sum();
    let journal_bytes = data.journal_bytes;
    let catalog_bytes = data.catalog.bytes;
    let runs = data.streams().iter().map(|s| s.pending.runs).sum();
    let crowding = data
        .streams()
        .iter()
        .map(|s| s.pending.crowding_marks)
        .sum();
    drop(status);
    samples.time("close-actual-close", 1, || {
        db.close(Timestamp(2 * DAY)).unwrap()
    });
    let (file_bytes, written_bytes, syncs, _) = (engine.observe)();
    samples.csv(&Context {
        source: "synthetic-approximation",
        seed,
        scale,
        shape: fixture.shape,
        engine: engine.name,
        durability: if durability == Durability::Durable {
            "Durable"
        } else {
            "Consistent"
        },
        logical_bytes,
        file_bytes,
        written_bytes,
        syncs,
        journal_bytes,
        runs,
        crowding,
        catalog_bytes,
        replay_bytes,
    })
}

/// One public maintenance call, classified by its returned transaction count.
fn round<S: CompactStorage>(
    db: &mut Tsdb<S>,
    policy: &RetentionPolicy,
    now: i64,
    samples: &mut Samples,
) -> bool {
    let done = samples.time("maintain-idle", 1, || {
        db.maintain(&Maintain {
            now: Timestamp(now),
            retention: policy,
            rounds: 1,
        })
        .unwrap()
    });
    let name = if done.expired > 0 {
        "maintain-expire"
    } else if done.cleanups > 0 {
        "maintain-cleanup"
    } else if done.recycles > 0 {
        "maintain-recycle"
    } else if done.consolidations > 0 {
        "maintain-consolidate"
    } else if done.merges > 0 {
        "maintain-merge"
    } else {
        "maintain-idle"
    };
    samples.name_last(name);
    done.more
}

/// Tiny disposable write/query warmup; no warmup sample enters the reported distribution.
pub fn warmup<S: CompactStorage>(reopen: impl Fn() -> Result<S, StorageError>, seed: u64) {
    let fixture = Fixture::generate(Shape::tiny(), seed);
    let mut db = Tsdb::open(
        reopen().unwrap(),
        fixture.shape.schema(),
        fixture.shape.profile(),
    )
    .unwrap();
    for i in 0..fixture.batches() {
        fixture
            .batch(&mut db, i)
            .write(&mut db, Durability::Consistent, |db, input, d| {
                db.write(input, d).unwrap()
            });
    }
    fixture.verify(&db);
    db.close(Timestamp(DAY)).unwrap();
}

/// Isolated series/event workloads identify every round kind using public Maintained counters.
pub fn maintenance<S: CompactStorage>(
    events: bool,
    seed: u64,
    engine: Engine<impl Fn() -> Result<S, StorageError>, impl Fn() -> (u64, u64, u64, u64)>,
) -> String {
    let shape = Shape {
        name: if events {
            "maintenance-events"
        } else {
            "maintenance-series"
        },
        points: if events { 0 } else { 2 },
        events: if events { 2 } else { 0 },
        combinations: 2,
        ..Shape::tiny()
    };
    let fixture = Fixture::generate(shape, seed);
    let profile = Profile {
        journal_bytes: 1,
        series_quiet_time: Some(std::time::Duration::from_millis(1)),
        event_buffer_rows: 1,
        consolidate_chunks: 2,
        max_sorted_runs: 2,
        cleanup_chunks: 2,
        ..shape.profile()
    };
    let mut db = Tsdb::open((engine.reopen)().unwrap(), shape.schema(), profile).unwrap();
    let keep = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    let mut samples = Samples::default();
    // Repeated overlapping two-row batches, prepared before each timed API call.
    for phase in 0..2 {
        for id in 0..6 {
            fixture
                .batch(&mut db, 0)
                .write(&mut db, Durability::Durable, |db, input, d| {
                    let input = Input {
                        batch_id: (phase * 6 + id + 1) as u64,
                        now: Timestamp(3 * DAY),
                        ..*input
                    };
                    db.write(&input, d).unwrap()
                });
        }
        for _ in 0..1000 {
            if !round(&mut db, &keep, 3 * DAY, &mut samples) {
                break;
            }
        }
    }
    let expected = fixture.expected_sum * 12;
    let snap = db.reader().capture().unwrap();
    let result = snap
        .series(&SeriesQuery {
            range: fixture.range(),
            tier: TierId(0),
            fields: FieldMask(1),
            requests: &[GroupRequest {
                filters: &[],
                key: GroupKey::Total,
            }],
        })
        .unwrap();
    assert_eq!(
        result.lists()[0].iter().map(|g| g.sums[0]).sum::<u64>(),
        expected
    );
    drop(result);
    drop(snap);
    let expire = Retention {
        max_age: Some(std::time::Duration::from_secs(1)),
        max_bytes: None,
    };
    let policy = RetentionPolicy {
        tiers: vec![expire],
        streams: vec![expire],
    };
    for _ in 0..1000 {
        if !round(&mut db, &policy, 10 * DAY, &mut samples) {
            break;
        }
    }
    for name in [
        "maintain-expire",
        "maintain-cleanup",
        "maintain-recycle",
        "maintain-consolidate",
        "maintain-merge",
    ] {
        assert!(
            samples.0.iter().any(|sample| sample.operation == name),
            "unmeasured {name} events={events}"
        );
    }
    // The isolated stream makes the otherwise shared consolidation/merge counters unambiguous.
    for sample in &mut samples.0 {
        sample.operation = match (sample.operation, events) {
            ("maintain-consolidate", false) => "maintain-series-consolidate",
            ("maintain-consolidate", true) => "maintain-event-consolidate",
            ("maintain-merge", false) => "maintain-deltas-merge",
            ("maintain-merge", true) => "maintain-runs-merge",
            (name, _) => name,
        };
    }
    let status = db.status().unwrap();
    let data = status.data().unwrap();
    assert!(
        data.streams()
            .iter()
            .all(|s| s.rows == 0 && s.pending.cleanup_chunks == 0)
    );
    assert_eq!(data.catalog.combinations, 0);
    drop(status);
    db.close(Timestamp(10 * DAY)).unwrap();
    let (file_bytes, written_bytes, syncs, _) = (engine.observe)();
    samples.csv(&Context {
        source: "synthetic-approximation",
        seed,
        scale: Scale::Smoke,
        shape,
        engine: engine.name,
        durability: "Durable",
        logical_bytes: 0,
        file_bytes,
        written_bytes,
        syncs,
        journal_bytes: 0,
        runs: 0,
        crowding: 0,
        catalog_bytes: 0,
        replay_bytes: 0,
    })
}

/// Shared timed read path for synthetic data and a compatible owner-supplied sanitized copy.
pub fn reads<S: CompactStorage>(
    db: &Tsdb<S>,
    range: TimeRange,
    points: u64,
    events: u64,
    repeats: usize,
    samples: &mut Samples,
) {
    let snap = samples.time("capture", 1, || db.reader().capture().unwrap());
    let query = SeriesQuery {
        range,
        tier: TierId(0),
        requests: &[GroupRequest {
            filters: &[],
            key: GroupKey::Total,
        }],
        fields: FieldMask(1),
    };
    let predicate = EventPredicate::default();
    let event_query = EventQuery {
        stream: StreamId(0),
        range,
        direction: Direction::Forward,
        cursor: None,
        predicate: &predicate,
        limit: 16,
    };
    drop(snap.series(&query).unwrap());
    drop(snap.scan_events(&event_query).unwrap());
    drop(db.status().unwrap());
    for _ in 0..repeats {
        drop(samples.time("series-query", points, || {
            snap.series(black_box(&query)).unwrap()
        }));
        drop(samples.time("event-page", 16.min(events), || {
            snap.scan_events(black_box(&event_query)).unwrap()
        }));
        drop(samples.time("status", 1, || db.status().unwrap()));
    }
}

/// Explicit path mode operates on a private copy only, through the same public read path.
#[cfg(feature = "redb")]
pub fn sanitized(
    path: &std::path::Path,
    scale: Scale,
    shape: Shape,
    seed: u64,
    read_samples: usize,
) -> String {
    use super::{
        disk::{Counting, Counts},
        tsdb::storage::redb::RedbStorage,
    };
    use std::sync::Arc;
    let temp = super::tempdir();
    let copy = temp.path().join("sanitized.redb");
    std::fs::copy(path, &copy).unwrap();
    let counts = Arc::new(Counts::default());
    let mut samples = Samples::default();
    let schema = shape.schema();
    let profile = shape.profile();
    let mut db = samples.time("open-replay", 1, || {
        Tsdb::open(
            RedbStorage::open_with_backend(
                Counting::open(&copy, counts.clone()),
                shape.profile().backend_cache,
            )
            .unwrap(),
            schema,
            profile,
        )
        .unwrap()
    });
    let status = db.status().unwrap();
    let data = status.data().unwrap();
    let logical_bytes = data.streams().iter().map(|s| s.bytes).sum();
    let journal_bytes = data.journal_bytes;
    let catalog_bytes = data.catalog.bytes;
    let runs = data.streams().iter().map(|s| s.pending.runs).sum();
    let crowding = data
        .streams()
        .iter()
        .map(|s| s.pending.crowding_marks)
        .sum();
    let points = data.streams()[0].rows;
    let events = data.streams()[1].rows;
    let width = if shape.minute { 60_000 } else { 3_600_000 };
    let mut bounds = Vec::new();
    for stream in data.streams() {
        if let (Some(first), Some(last)) = (stream.oldest, stream.newest) {
            let width = match stream.stream {
                StreamRef::Tier(_) => width,
                StreamRef::Events(_) => 1,
            };
            bounds.push((
                first.saturating_mul(width),
                last.saturating_add(1).saturating_mul(width),
            ));
        }
    }
    let start = bounds.iter().map(|pair| pair.0).min().unwrap_or(0);
    let end = bounds.iter().map(|pair| pair.1).max().unwrap_or(DAY);
    let range = TimeRange::new(Timestamp(start), Timestamp(end)).unwrap();
    drop(status);
    reads(&db, range, points, events, read_samples, &mut samples);
    samples.time("close-actual-close", 1, || {
        db.close(Timestamp(DAY)).unwrap()
    });
    let (written_bytes, syncs) = counts.read();
    samples.csv(&Context {
        source: "owner-supplied-copy",
        seed,
        scale,
        shape,
        engine: "redb",
        durability: "existing",
        logical_bytes,
        file_bytes: std::fs::metadata(&copy).unwrap().len(),
        written_bytes,
        syncs,
        journal_bytes,
        runs,
        crowding,
        catalog_bytes,
        replay_bytes: journal_bytes,
    })
}

/// Expires old data while pinned, releases the pin, then admits distinct replacement labels.
fn churn<S: CompactStorage>(db: &mut Tsdb<S>, fixture: &Fixture, seed: u64, samples: &mut Samples) {
    let expire = Retention {
        max_age: Some(std::time::Duration::from_secs(1)),
        max_bytes: None,
    };
    let policy = RetentionPolicy {
        tiers: vec![expire],
        streams: vec![expire],
    };
    for _ in 0..1000 {
        if !round(db, &policy, 10 * DAY, samples) {
            break;
        }
    }
    let status = db.status().unwrap();
    let data = status.data().unwrap();
    assert!(data.streams().iter().all(|stream| stream.rows == 0));
    assert_eq!(data.catalog.pin_keys, 1);
    assert_eq!(data.catalog.combinations, 1);
    drop(status);
    let input = Input {
        batch_id: fixture.batches() as u64 + 1,
        now: Timestamp(10 * DAY),
        series: &[],
        events: &[],
        aux: &[],
        pins: &[PinOp::Unpin {
            key: b"synthetic-pin",
        }],
    };
    samples.time("unpin", 1, || {
        db.write(&input, Durability::Durable).unwrap()
    });
    for _ in 0..1000 {
        if !round(db, &policy, 10 * DAY, samples) {
            break;
        }
    }
    assert_eq!(db.status().unwrap().data().unwrap().catalog.combinations, 0);
    let mut replacement = Fixture::generate(
        Shape {
            combinations: 16,
            points: 0,
            events: 16,
            churn: false,
            ..fixture.shape
        },
        seed,
    );
    for label in &mut replacement.labels {
        label.extend_from_slice(b"-replacement");
    }
    for row in &mut replacement.rows {
        row.at += 20 * DAY;
    }
    replacement
        .batch(db, 0)
        .write(db, Durability::Durable, |db, input, d| {
            let input = Input {
                batch_id: fixture.batches() as u64 + 2,
                ..*input
            };
            samples.time("churn-new-labels", 16, || db.write(&input, d).unwrap())
        });
    let snapshot = db.reader().capture().unwrap();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: StreamId(0),
            range: TimeRange::new(Timestamp(0), Timestamp(21 * DAY)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 64,
        })
        .unwrap();
    assert_eq!(page.events().len(), replacement.rows.len());
    for (event, row) in page.events().iter().zip(&replacement.rows) {
        assert_eq!(event.key, row.at);
        let EventValue::Combo(combo) = event.values[0] else {
            panic!("missing replacement series")
        };
        let label = snapshot.combination(combo).unwrap().next().unwrap();
        assert_eq!(
            snapshot.label(LabelId(0), label).unwrap(),
            replacement.labels[row.label]
        );
    }
}

/// Copy mode preserves the requested scale and refuses to overwrite its source through an alias.
#[cfg(feature = "redb")]
pub fn check_copy(seed: u64) {
    let temp = super::tempdir();
    let path = temp.path().join("source.redb");
    warmup(
        || super::tsdb::storage::redb::RedbStorage::open(&path, Profile::desktop().backend_cache),
        seed,
    );
    let before = std::fs::read(&path).unwrap();
    let csv = format!(
        "{}{}",
        super::output::HEADER,
        sanitized(&path, Scale::M, Shape::tiny(), seed, 1)
    );
    super::output::validate(&csv);
    for row in csv.lines().skip(1) {
        assert_eq!(row.split(',').nth(2), Some("M"));
    }
    let alias = temp.path().join("alias.redb");
    std::fs::hard_link(&path, &alias).unwrap();
    assert_eq!(
        super::output::write_new(&alias, &csv).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
