//! Isolated allocator instrumentation: other traffic tests keep their ordinary allocator.
use nyanpasu_traffic::*;
use nyanpasu_tsdb::{Profile, storage::redb::RedbStorage};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count(bytes: usize) {
    if COUNTING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[test]
#[ignore = "release benchmark with 500,000 stored hour rows"]
fn report_500k_hour_rows() {
    let dir = tempfile::tempdir().unwrap();
    let profile = Profile::desktop();
    let storage =
        RedbStorage::open(dir.path().join("traffic.tsdb"), profile.backend_cache).unwrap();
    let (mut db, _) = TrafficDb::open(storage, profile).unwrap();
    let tuples: Vec<_> = (0..500)
        .map(|i| {
            Arc::new(Dimensions {
                process: format!("/usr/bin/process-{}", i % 40),
                source: format!("192.168.1.{}", i % 20),
                inbound: format!("mixed-{}", i % 3),
                target: format!("host-{i}.example.com"),
                protocol: if i % 7 == 0 { "udp" } else { "tcp" }.into(),
                rule: RuleKey {
                    kind: "DomainSuffix".into(),
                    payload: format!("rule-{}", i % 25),
                },
                chains: vec![format!("Node-{}", i % 15), "Auto".into(), "Proxy".into()],
                profile: Some(format!("profile-{}", i % 3)),
                source_region: "CN".into(),
                destination_region: format!("R{}", i % 12),
                destination_basis: None,
            })
        })
        .collect();
    let empty = || FlushBatch {
        meta: SessionMeta::default(),
        active: vec![],
        removed: vec![],
        minutes: vec![],
        hours: vec![],
        closed: vec![],
        closed_buckets: vec![],
    };
    for first in (0..1000u32).step_by(4) {
        let mut batch = empty();
        batch.hours = (first..first + 4)
            .flat_map(|hour| {
                tuples.iter().map(move |d| {
                    (
                        hour,
                        d.clone(),
                        Usage {
                            bytes: Bytes {
                                upload: u64::from(hour) + 1,
                                download: 2,
                            },
                            connections: 1,
                        },
                    )
                })
            })
            .collect();
        db.flush(&batch, i64::from(first + 4) * 3_600_000, None)
            .unwrap();
    }
    db.flush(&empty(), 1002 * 3_600_000, None).unwrap();
    let request = ReportRequest {
        query: TrafficQuery {
            range: TrafficRange::All,
            scope: TrafficScope::Closed,
            filters: vec![TrafficFilter {
                dimension: Dimension::Inbound,
                value: "mixed-1".into(),
            }],
        },
        metric: Metric::Bytes,
        rankings: vec![
            Dimension::Origin,
            Dimension::Inbound,
            Dimension::Target,
            Dimension::Exit,
            Dimension::Process,
        ],
        ranking_limit: 10,
        topology: Some(TopologyRequest {
            layers: vec![
                Dimension::Origin,
                Dimension::Rule,
                Dimension::Chain,
                Dimension::Exit,
            ],
            limit_per_layer: Some(7),
        }),
    };
    let session = Session::new();
    for round in 0..5 {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        let started = Instant::now();
        COUNTING.store(true, Ordering::Relaxed);
        let report = db.report(&session, &request, 1002 * 3_600_000).unwrap();
        COUNTING.store(false, Ordering::Relaxed);
        let elapsed = started.elapsed();
        println!(
            "round {round}: {elapsed:?}; {} heap allocations/reallocations, {} requested bytes",
            ALLOCATIONS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed)
        );
        assert_eq!(report.total.connections, 167_000);
    }
}
