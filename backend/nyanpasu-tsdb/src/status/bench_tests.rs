//! Ignored release measurements of exact charged-memory peaks, not process RSS.

use crate as tsdb;
#[allow(dead_code)]
#[path = "../../benches/support/fixture.rs"]
pub(crate) mod fixture;

use crate::{
    EventPredicate, EventQuery, FieldMask, GroupKey, GroupRequest, Maintain, MemoryBudget,
    RecoveryProgress, Retention, RetentionPolicy, SeriesQuery, StreamId, TierId, Timestamp, Tsdb,
    storage::{Direction, Durability, Storage, memory::MemoryDisk},
};
use fixture::{DAY, Fixture, SEED, Scale, Shape};
use std::{hint::black_box, time::Instant};

/// Root budgets exclude sub-budget double counting; clones keep only the accounting objects.
fn roots<S: Storage>(db: &Tsdb<S>) -> [MemoryBudget; 3] {
    [
        db.round_context(&db.current()).head_budget.clone(),
        db.staging_budget().clone(),
        db.shared().query_budget.clone(),
    ]
}
/// Reset with resident charges as baseline, run once, then report every transient high-water mark.
fn measure<T>(engine: &str, name: &str, budgets: &[MemoryBudget; 3], run: impl FnOnce() -> T) -> T {
    // Verification and other untimed reads may reserve more than a timed operation.
    // Record their exact peaks before resetting; ns=0 denotes an untimed observation.
    println!(
        "synthetic-approximation,{engine},between-operations,0,{},{},{},{},{},{}",
        budgets[0].peak(),
        budgets[1].peak(),
        budgets[2].peak(),
        budgets[0].reserved(),
        budgets[1].reserved(),
        budgets[2].reserved()
    );
    for budget in budgets {
        budget.reset_peak();
    }
    let start = Instant::now();
    let result = black_box(run());
    let ns = start.elapsed().as_nanos();
    println!(
        "synthetic-approximation,{engine},{name},{ns},{},{},{},{},{},{}",
        budgets[0].peak(),
        budgets[1].peak(),
        budgets[2].peak(),
        budgets[0].reserved(),
        budgets[1].reserved(),
        budgets[2].reserved()
    );
    result
}
/// Shared generated inputs, all public operations, and exact root charges on either engine.
fn peaks<S: crate::CompactStorage>(
    engine: &str,
    reopen: impl Fn() -> Result<S, crate::storage::StorageError>,
) {
    if cfg!(debug_assertions) {
        panic!("run ignored memory measurements with --release");
    }
    let scale = match std::env::var("TSDB_BENCH_SCALE").as_deref() {
        Ok("M") => Scale::M,
        Ok("L") => Scale::L,
        Ok("smoke") => Scale::Smoke,
        _ => Scale::S,
    };
    let shapes = Shape::all(scale);
    let name = std::env::var("TSDB_BENCH_SHAPE").unwrap_or_else(|_| "hourly".into());
    let shape = *shapes
        .iter()
        .find(|s| s.name == name)
        .expect("known benchmark shape");
    let fixture = Fixture::generate(shape, SEED);
    println!(
        "engine={engine} scale={} shape={} seed={SEED}",
        scale.name(),
        shape.name
    );
    let mut db = Tsdb::open(
        reopen().unwrap(),
        fixture.shape.schema(),
        fixture.shape.profile(),
    )
    .unwrap();
    let mut budgets = roots(&db);
    println!(
        "source,engine,operation,ns,head_peak,staging_peak,query_peak,\
         head_after,staging_after,query_after"
    );
    // Open's root objects already contain exact admission peaks from construction/replay.
    println!(
        "synthetic-approximation,{engine},open,0,{},{},{},{},{},{}",
        budgets[0].peak(),
        budgets[1].peak(),
        budgets[2].peak(),
        budgets[0].reserved(),
        budgets[1].reserved(),
        budgets[2].reserved()
    );
    for i in 0..fixture.batches() {
        fixture.batch(&mut db, i).write(
            &mut db,
            if i % 2 == 0 {
                Durability::Consistent
            } else {
                Durability::Durable
            },
            |db, input, d| measure(engine, "write", &budgets, || db.write(input, d).unwrap()),
        );
    }
    fixture.verify(&db);
    let snapshot = measure(engine, "capture", &budgets, || {
        db.reader().capture().unwrap()
    });
    let query = SeriesQuery {
        tier: TierId(0),
        range: fixture.range(),
        fields: FieldMask(1),
        requests: &[GroupRequest {
            filters: &[],
            key: GroupKey::Total,
        }],
    };
    let groups = measure(engine, "series-retained-result", &budgets, || {
        snapshot.series(&query).unwrap()
    });
    let page = measure(engine, "event-page", &budgets, || {
        snapshot
            .scan_events(&EventQuery {
                stream: StreamId(0),
                range: fixture.range(),
                direction: Direction::Forward,
                cursor: None,
                predicate: &EventPredicate::default(),
                limit: 16,
            })
            .unwrap()
    });
    drop(measure(engine, "status", &budgets, || db.status().unwrap()));
    drop(snapshot);
    drop(page);
    assert!(
        budgets[2].reserved() > 0,
        "retained result must keep its charge"
    );
    drop(groups);
    assert_eq!(budgets[2].reserved(), 0);
    measure(engine, "suspend-close", &budgets, || {
        db.suspend(Timestamp(2 * DAY)).unwrap()
    });
    assert!(budgets.iter().all(|b| b.reserved() == 0));
    let result = measure(engine, "recover-old-roots", &budgets, || {
        db.recover_with(&reopen, |view| {
            view.aux_get(b"synthetic-host", |b| b.map(<[u8]>::to_vec))
        })
        .unwrap()
    });
    assert!(matches!(result, RecoveryProgress::Recovered { .. }));
    budgets = roots(&db);
    println!(
        "synthetic-approximation,{engine},recover-new-roots,0,{},{},{},{},{},{}",
        budgets[0].peak(),
        budgets[1].peak(),
        budgets[2].peak(),
        budgets[0].reserved(),
        budgets[1].reserved(),
        budgets[2].reserved()
    );
    let policy = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    for _ in 0..10000 {
        let done = measure(engine, "maintenance-round", &budgets, || {
            db.maintain(&Maintain {
                now: Timestamp(2 * DAY),
                retention: &policy,
                rounds: 1,
            })
            .unwrap()
        });
        if !done.more {
            break;
        }
    }
    black_box(measure(engine, "compact", &budgets, || {
        db.compact().unwrap()
    }));
    measure(engine, "close", &budgets, || {
        db.close(Timestamp(2 * DAY)).unwrap()
    });
    assert!(budgets.iter().all(|b| b.reserved() == 0));
}
/// Root peaks include reservations taken and returned inside one facade call.
#[test]
#[ignore = "release-only synthetic charged-memory benchmark"]
fn memory_charged_peaks() {
    let disk = MemoryDisk::new();
    peaks("memory", || disk.open());
}
/// Real redb keeps backend cache/overhead outside these three facade root budgets.
#[cfg(feature = "redb")]
#[test]
#[ignore = "release-only synthetic charged-memory benchmark"]
fn redb_charged_peaks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("peaks.redb");
    peaks("redb", || {
        crate::storage::redb::RedbStorage::open(&path, crate::Profile::desktop().backend_cache)
    });
}
