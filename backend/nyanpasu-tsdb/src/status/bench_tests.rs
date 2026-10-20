//! Ignored release measurements of exact charged-memory peaks, not process RSS.

use crate as tsdb;
#[allow(dead_code)]
#[path = "../../benches/support/fixture.rs"]
pub(crate) mod fixture;

use crate::{
    EventPredicate, EventQuery, FieldMask, GroupKey, GroupRequest, Input, Maintain, MemoryBudget,
    RecoveryProgress, Retention, RetentionPolicy, SeriesQuery, Snapshot, StreamId, TierId,
    Timestamp, Tsdb,
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
/// Public storage totals pair the written head/L0 state with converged consolidation.
fn stage<S: Storage>(engine: &str, phase: &str, db: &Tsdb<S>) {
    let status = db.status().unwrap();
    let data = status.data().unwrap();
    println!(
        "stage={phase},engine={engine},catalog={},journal={},tier_rows={},\
         tier_bytes={},event_rows={},event_bytes={},event_gated={}",
        data.catalog.bytes,
        data.journal_bytes,
        data.streams()[0].rows,
        data.streams()[0].bytes,
        data.streams()[1].rows,
        data.streams()[1].bytes,
        data.maintenance.event_consolidation_gated_off
    );
}
/// Measures both head/L0 and consolidated reads, including retained result ownership.
fn reads<S: Storage>(
    engine: &str,
    fixture: &Fixture,
    db: &Tsdb<S>,
    budgets: &[MemoryBudget; 3],
    retained: Option<Snapshot<S>>,
) {
    let snapshot = measure(engine, "capture", budgets, || {
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
    let groups = measure(engine, "series-retained-result", budgets, || {
        snapshot.series(&query).unwrap()
    });
    let page = measure(engine, "event-page", budgets, || {
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
    drop(measure(engine, "status", budgets, || db.status().unwrap()));
    drop(retained);
    drop(snapshot);
    drop(page);
    assert!(
        budgets[2].reserved() > 0,
        "retained result must keep its charge"
    );
    drop(groups);
    assert_eq!(budgets[2].reserved(), 0);
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
    let mut retained = None;
    let mut catalog_resident_max = db.current().catalog.bytes();
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
        catalog_resident_max = catalog_resident_max.max(db.current().catalog.bytes());
        if i == 0 && fixture.shape.retained {
            retained = Some(measure(engine, "capture-retained", &budgets, || {
                db.reader().capture().unwrap()
            }));
        }
    }
    fixture.verify(&db);
    stage(engine, "written", &db);
    reads(engine, &fixture, &db, &budgets, retained);
    measure(engine, "suspend-close", &budgets, || {
        db.suspend(Timestamp(2 * DAY + fixture.shape.late_ms))
            .unwrap()
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
    catalog_resident_max = catalog_resident_max.max(db.current().catalog.bytes());
    println!(
        "synthetic-approximation,{engine},recover-new-roots,0,{},{},{},{},{},{}",
        budgets[0].peak(),
        budgets[1].peak(),
        budgets[2].peak(),
        budgets[0].reserved(),
        budgets[1].reserved(),
        budgets[2].reserved()
    );
    measure(engine, "flush-quiet-head", &budgets, || {
        db.write(
            &Input {
                batch_id: fixture.batches() as u64 + 1,
                now: Timestamp(2 * DAY + fixture.shape.late_ms),
                series: &[],
                events: &[],
                aux: &[],
                pins: &[],
            },
            Durability::Durable,
        )
        .unwrap()
    });
    catalog_resident_max = catalog_resident_max.max(db.current().catalog.bytes());
    stage(engine, "flushed-l0", &db);
    assert_eq!(db.status().unwrap().data().unwrap().journal_bytes, 0);
    let policy = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    for _ in 0..10000 {
        let done = measure(engine, "maintenance-round", &budgets, || {
            db.maintain(&Maintain {
                now: Timestamp(2 * DAY + fixture.shape.late_ms),
                retention: &policy,
                rounds: 1,
            })
            .unwrap()
        });
        catalog_resident_max = catalog_resident_max.max(db.current().catalog.bytes());
        println!(
            "round_work={}/{}/{}/{}/{}/{}/{}",
            done.expired,
            done.cleanups,
            done.recycles,
            done.consolidations,
            done.merges,
            done.chunks_rewritten,
            done.chunks_deleted
        );
        if !done.more {
            break;
        }
    }
    fixture.verify(&db);
    stage(engine, "converged", &db);
    reads(engine, &fixture, &db, &budgets, None);
    if fixture.shape.churn {
        let expire = Retention {
            max_age: Some(std::time::Duration::from_secs(1)),
            max_bytes: None,
        };
        let expired = RetentionPolicy {
            tiers: vec![expire],
            streams: vec![expire],
        };
        for unpin in [false, true] {
            if unpin {
                measure(engine, "unpin", &budgets, || {
                    db.write(
                        &Input {
                            batch_id: fixture.batches() as u64 + 2,
                            now: Timestamp(10 * DAY),
                            series: &[],
                            events: &[],
                            aux: &[],
                            pins: &[crate::PinOp::Unpin {
                                key: b"synthetic-pin",
                            }],
                        },
                        Durability::Durable,
                    )
                    .unwrap()
                });
            }
            for _ in 0..10_000 {
                let done = measure(engine, "retention-recycle-round", &budgets, || {
                    db.maintain(&Maintain {
                        now: Timestamp(10 * DAY),
                        retention: &expired,
                        rounds: 1,
                    })
                    .unwrap()
                });
                catalog_resident_max = catalog_resident_max.max(db.current().catalog.bytes());
                println!(
                    "release_work={}/{}/{}/{}/{}",
                    done.expired,
                    done.cleanups,
                    done.recycles,
                    done.chunks_deleted,
                    done.combos_released
                );
                if !done.more {
                    break;
                }
            }
            let status = db.status().unwrap();
            let data = status.data().unwrap();
            assert!(data.streams().iter().all(|stream| stream.rows == 0));
            assert_eq!(data.catalog.combinations, u64::from(!unpin));
            assert_eq!(data.catalog.pin_keys, u64::from(!unpin));
        }
    }
    println!("catalog_resident_max_bytes={catalog_resident_max}");
    black_box(measure(engine, "compact", &budgets, || {
        db.compact().unwrap()
    }));
    measure(engine, "close", &budgets, || {
        db.close(Timestamp(2 * DAY + fixture.shape.late_ms))
            .unwrap()
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
