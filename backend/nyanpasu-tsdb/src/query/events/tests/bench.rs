//! Reuses the existing private read log for metadata, without a facade observer API.

use crate as tsdb;
use crate::status::bench_tests::fixture;

use super::super::{ReadLog, scan};
use fixture::{DAY, Fixture, SEED, Shape};
use tsdb::{
    EventPredicate, EventQuery, Maintain, Profile, Retention, RetentionPolicy, StreamId, Timestamp,
    Tsdb,
    storage::{Direction, Durability, Storage, memory::MemoryDisk},
};

/// Six overlapping L0 chunks exceed R=2; consolidation/merge must reduce actual scan ways.
fn observe<S: Storage>(storage: S, engine: &str) {
    let fixture = Fixture::generate(
        Shape {
            points: 0,
            events: 96,
            ..Shape::tiny()
        },
        SEED,
    );
    let profile = Profile {
        event_buffer_rows: 1,
        consolidate_chunks: 2,
        max_sorted_runs: 2,
        ..fixture.shape.profile()
    };
    let mut db = Tsdb::open(storage, fixture.shape.schema(), profile).unwrap();
    for i in 0..6 {
        fixture
            .batch(&mut db, 0)
            .write(&mut db, Durability::Durable, |db, input, d| {
                let input = tsdb::Input {
                    batch_id: i + 1,
                    now: Timestamp(3 * DAY),
                    ..*input
                };
                db.write(&input, d).unwrap()
            });
    }
    let ways = |db: &Tsdb<S>| {
        let snapshot = db.reader().capture().unwrap();
        let mut log = ReadLog::default();
        let page = scan(
            &snapshot,
            snapshot.budget(),
            &EventQuery {
                stream: StreamId(0),
                range: fixture.range(),
                direction: Direction::Forward,
                cursor: None,
                predicate: &EventPredicate::default(),
                limit: 16,
            },
            &mut log,
        )
        .unwrap();
        assert_eq!(page.events().len(), 16);
        drop(page);
        assert_eq!(log.ways.len(), 1);
        log.ways[0]
    };
    let before = ways(&db);
    assert_eq!(before, 6);
    println!("synthetic-approximation,{engine},overlap-before,{SEED},{before},0,2,1");
    let keep = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    let done = db
        .maintain(&Maintain {
            now: Timestamp(3 * DAY),
            retention: &keep,
            rounds: 1000,
        })
        .unwrap();
    assert!(!done.more);
    assert!(done.merges > 0);
    let after = ways(&db);
    assert!((1..=2).contains(&after));
    let runs = db.status().unwrap().data().unwrap().streams()[1]
        .pending
        .runs;
    println!("synthetic-approximation,{engine},overlap-after,{SEED},{after},{runs},2,1");
    db.close(Timestamp(3 * DAY)).unwrap();
}

/// Counter-only observation: no Instant timings or performance claims from a debug test.
#[test]
#[ignore = "benchmark metadata; uses the existing test-only ReadLog"]
fn records_overlap_ways() {
    println!("source,engine,phase,seed,ways,sorted_runs,max_sorted_runs,n");
    let disk = MemoryDisk::new();
    observe(disk.open().unwrap(), "memory");
    #[cfg(feature = "redb")]
    {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ways.redb");
        observe(
            tsdb::storage::redb::RedbStorage::open(path, Profile::desktop().backend_cache).unwrap(),
            "redb",
        );
    }
}
