use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use nyanpasu_observers::traffic::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficClient,
    TrafficFeed, TrafficOwnerFactory, TrafficStartArgs, TrafficStorage,
};
use nyanpasu_traffic::{
    Bytes, Dimension, Dimensions, Frame, Metric, ReportRequest, RuleKey, Sample, TrafficFilter,
    TrafficQuery, TrafficRange, TrafficScope, TrafficSummary, Usage,
};
use nyanpasu_tsdb::Profile;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::TrafficFileStorage;
use crate::{clash::ws::ClashConnectionsFrame, client::traffic_frames::ClashFrameNormalizer};

const CORE: &str = "core-1";
const A: &str = "11111111-1111-1111-1111-111111111111";

struct FakeProfiles(Mutex<Option<String>>);

impl ProfileSelection for FakeProfiles {
    fn current(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
}

struct FakeRetention(Mutex<Option<Duration>>);

impl RetentionPolicy for FakeRetention {
    fn retention(&self) -> Option<Duration> {
        *self.0.lock().unwrap()
    }
}

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct FakeLocalSource(Mutex<LocalSourceIps>);

impl LocalSourceLocation for FakeLocalSource {
    fn addresses(&self) -> LocalSourceIps {
        *self.0.lock().unwrap()
    }
}

/// A file-backed owner fed by the core's Clash normalizer.
struct Harness {
    _dir: TempDir,
    _shutdown: CancellationToken,
    _tasks: TaskTracker,
    client: TrafficClient,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
    geo: watch::Sender<Option<Arc<nyanpasu_geodata::IpIndex>>>,
    local_source: Arc<FakeLocalSource>,
}

impl Harness {
    async fn new(profile: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let limits = Profile::desktop();
        let factory = Arc::new(TrafficFileStorage::new(
            dir.path().join("traffic.tsdb"),
            limits.backend_cache,
        ));
        let frames = watch::channel(None).0;
        let geo = watch::channel(None).0;
        let local_source = Arc::new(FakeLocalSource::default());
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = factory
            .spawn(
                TrafficStartArgs {
                    profile: limits,
                    profiles: Arc::new(FakeProfiles(Mutex::new(Some(profile.to_owned())))),
                    retention: Arc::new(FakeRetention(Mutex::new(Some(Duration::from_secs(
                        7 * 24 * 60 * 60,
                    ))))),
                    clock: Arc::new(FakeClock(AtomicI64::new(2_000))),
                    frames: TrafficFeed::new(frames.subscribe(), Arc::new(ClashFrameNormalizer)),
                    geo: geo.subscribe(),
                    local_source: local_source.clone(),
                },
                shutdown.clone(),
                &tasks,
            )
            .await
            .unwrap();
        Self {
            _dir: dir,
            _shutdown: shutdown,
            _tasks: tasks,
            client,
            frames,
            geo,
            local_source,
        }
    }

    async fn report(&self, request: ReportRequest) -> nyanpasu_traffic::TrafficReport {
        self.client.report(request).await.unwrap()
    }

    async fn total(&self, query: TrafficQuery) -> Usage {
        self.report(request(query, &[])).await.total
    }
}

fn bytes(upload: u64, download: u64) -> Bytes {
    Bytes { upload, download }
}

#[tokio::test]
async fn startup_imports_v3_and_continues_the_restored_session() {
    use nyanpasu_traffic::{ActiveConnection, ClosedConnection, SessionMeta};
    use redb::{Database, TableDefinition};
    use std::collections::BTreeMap;

    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    let db = Database::create(&old).unwrap();
    let txn = db.begin_write().unwrap();
    txn.open_table(TableDefinition::<&str, u64>::new("schema"))
        .unwrap()
        .insert("version", 3)
        .unwrap();
    let meta = SessionMeta {
        last_sample_at: Some(1000),
        instance_id: Some(CORE.into()),
        global_counters: Some(bytes(10, 20)),
    };
    txn.open_table(TableDefinition::<&str, &[u8]>::new("session"))
        .unwrap()
        .insert("meta", serde_json::to_vec(&meta).unwrap().as_slice())
        .unwrap();
    let active = ActiveConnection {
        id: A.into(),
        started_at: 500,
        first_seen_at: 1000,
        counters: bytes(10, 20),
        dimensions: dims("legacy"),
        minutes: BTreeMap::from([(0, bytes(10, 20))]),
        hours: BTreeMap::from([(0, bytes(10, 20))]),
    };
    txn.open_table(TableDefinition::<&str, &[u8]>::new("active"))
        .unwrap()
        .insert(A, serde_json::to_vec(&active).unwrap().as_slice())
        .unwrap();
    txn.open_table(TableDefinition::<u64, &[u8]>::new("tuples"))
        .unwrap()
        .insert(0, serde_json::to_vec(&dims("closed")).unwrap().as_slice())
        .unwrap();
    for name in ["minute_usage", "hour_usage"] {
        txn.open_table(TableDefinition::<(u32, u64), (u64, u64, u64)>::new(name))
            .unwrap()
            .insert((0, 0), (3, 4, 1))
            .unwrap();
    }
    let closed = ClosedConnection {
        id: "closed".into(),
        started_at: 1,
        first_seen_at: 2,
        closed_at: 999,
        bytes: bytes(3, 4),
        dimensions: dims("closed"),
    };
    txn.open_table(TableDefinition::<(u64, &str), &[u8]>::new("closed"))
        .unwrap()
        .insert(
            (999, "closed"),
            serde_json::to_vec(&closed).unwrap().as_slice(),
        )
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    let profile = Profile::desktop();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let (_frames, frames) = watch::channel(None);
    let (_geo, geo) = watch::channel(None);
    let factory: Arc<dyn TrafficOwnerFactory> = Arc::new(TrafficFileStorage::new(
        dir.path().join("traffic.tsdb"),
        profile.backend_cache,
    ));
    let client = factory
        .spawn(
            TrafficStartArgs {
                profile,
                profiles: Arc::new(FakeProfiles(Mutex::new(Some("new-profile".into())))),
                retention: Arc::new(FakeRetention(Mutex::new(None))),
                clock: Arc::new(FakeClock(AtomicI64::new(2000))),
                frames: TrafficFeed::new(frames, Arc::new(ClashFrameNormalizer)),
                geo,
                local_source: Arc::new(FakeLocalSource::default()),
            },
            shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
    assert!(!old.exists());
    let summary = client.summary().await.unwrap();
    assert_eq!(summary.last_sample_at, Some(1000));
    assert_eq!(summary.active_connections, 1);
    assert_eq!(summary.closed_connections, 1);
    client
        .observe(Some(frame(
            2000,
            2000,
            bytes(15, 27),
            vec![sample(A, "changed", 15, 27)],
        )))
        .await
        .unwrap();
    let report = client
        .report(request(
            query(TrafficRange::All, TrafficScope::All, &[]),
            &[Dimension::Process, Dimension::Profile],
        ))
        .await
        .unwrap();
    assert_eq!(report.total, usage(18, 31, 2));
    // A continued connection takes the latest sample's dimensions but keeps its profile;
    // a new connection would have been attributed to "new-profile".
    let keys = |ranking: usize| -> Vec<_> {
        report.rankings[ranking]
            .groups
            .iter()
            .map(|g| g.key.as_str())
            .collect()
    };
    assert_eq!(keys(0), ["changed", "closed"]);
    assert_eq!(keys(1), [""]);
    client.flush().await.unwrap();
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    let (db, restored) = TrafficFileStorage::new(
        dir.path().join("traffic.tsdb"),
        Profile::desktop().backend_cache,
    )
    .initialize(Profile::desktop())
    .unwrap();
    let (meta, active) = restored.unwrap();
    assert_eq!(meta.global_counters, Some(bytes(15, 27)));
    assert_eq!(active[0].counters, bytes(15, 27));
    assert_eq!(active[0].bytes(), bytes(15, 27));
    assert_eq!(
        db.summary(&nyanpasu_traffic::Session::new())
            .unwrap()
            .closed_connections,
        1
    );
}

fn usage(upload: u64, download: u64, connections: u64) -> Usage {
    Usage {
        bytes: bytes(upload, download),
        connections,
    }
}

fn dims(process: &str) -> Dimensions {
    Dimensions {
        process: process.into(),
        source: "192.168.1.2".into(),
        inbound: "mixed".into(),
        target: "example.com".into(),
        protocol: "tcp".into(),
        rule: RuleKey {
            kind: "Match".into(),
            payload: String::new(),
        },
        chains: vec!["Node-A".into(), "Proxy".into()],
        profile: None,
        source_region: "unknown".into(),
        destination_region: "unknown".into(),
        destination_basis: None,
    }
}

fn sample(id: &str, process: &str, upload: u64, download: u64) -> Sample {
    Sample {
        id: id.into(),
        started_at: 500,
        counters: bytes(upload, download),
        dimensions: dims(process),
    }
}

fn frame(wall_ms: i64, mono_ms: u64, totals: Bytes, connections: Vec<Sample>) -> Frame {
    Frame {
        instance_id: CORE.into(),
        wall_ms,
        mono: Duration::from_millis(mono_ms),
        totals,
        connections,
    }
}

fn query(range: TrafficRange, scope: TrafficScope, filters: &[(Dimension, &str)]) -> TrafficQuery {
    TrafficQuery {
        range,
        scope,
        filters: filters
            .iter()
            .map(|(dimension, value)| TrafficFilter {
                dimension: *dimension,
                value: (*value).into(),
            })
            .collect(),
    }
}

fn everything() -> TrafficQuery {
    query(TrafficRange::All, TrafficScope::All, &[])
}

fn request(query: TrafficQuery, rankings: &[Dimension]) -> ReportRequest {
    ReportRequest {
        query,
        metric: Metric::Bytes,
        rankings: rankings.to_vec(),
        ranking_limit: 10,
        topology: None,
    }
}

fn connection(
    id: &str,
    upload: i64,
    download: i64,
    metadata: serde_json::Value,
) -> clash_api::Connection {
    serde_json::from_value(json!({
        "id": id,
        "metadata": metadata,
        "upload": upload,
        "download": download,
        "start": "2024-01-01T00:00:00Z",
        "chains": ["Node-A", "Proxy"],
        "rule": "DomainSuffix",
        "rulePayload": "example.com",
    }))
    .unwrap()
}

fn raw_frame(
    upload_total: i64,
    download_total: i64,
    connections: Vec<clash_api::Connection>,
) -> Arc<ClashConnectionsFrame> {
    Arc::new(ClashConnectionsFrame {
        instance_id: CORE.into(),
        snapshot: Arc::new(clash_api::ConnectionsSnapshot {
            upload_total,
            download_total,
            connections: Some(connections),
            memory: None,
        }),
    })
}

async fn until(client: &TrafficClient, done: impl Fn(&TrafficSummary) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done(&client.summary().await.unwrap()) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn pump_delivers_frames_and_lost_feeds() {
    let h = Harness::new("p1").await;
    h.frames.send_replace(Some(raw_frame(
        100,
        200,
        vec![connection(A, 100, 200, json!(null))],
    )));
    until(&h.client, |s| s.last_sample_at.is_some()).await;
    // The frame is stamped by the injected clock.
    assert_eq!(
        h.client.summary().await.unwrap().last_sample_at,
        Some(2_000)
    );

    h.frames.send_replace(Some(raw_frame(
        150,
        260,
        vec![connection(A, 150, 260, json!(null))],
    )));
    until(&h.client, |s| s.current_rate.is_some()).await;
    assert_eq!(h.total(everything()).await, usage(150, 260, 1));

    h.frames.send_replace(None);
    until(&h.client, |s| s.current_rate.is_none()).await;
}

#[tokio::test]
async fn pump_locates_regions_with_the_published_index() {
    let h = Harness::new("p1").await;
    let dat = nyanpasu_observers::geo::fixtures::geoip_dat(&[("US", &["8.0.0.0/8"])]);
    h.geo.send_replace(Some(Arc::new(
        nyanpasu_geodata::IpIndex::from_geoip_dat(&dat).unwrap(),
    )));

    h.frames.send_replace(Some(raw_frame(
        100,
        200,
        // The index answers instead of the core's code.
        vec![connection(
            A,
            100,
            200,
            json!({ "destinationIP": "8.8.8.8", "destinationGeoIP": ["cn"] }),
        )],
    )));
    until(&h.client, |s| s.last_sample_at.is_some()).await;

    let report = h
        .report(request(everything(), &[Dimension::DestinationRegion]))
        .await;
    assert_eq!(report.rankings[0].groups[0].key, "US");
}

#[tokio::test]
async fn pump_maps_local_regions_without_changing_source_identity_or_usage() {
    let h = Harness::new("p1").await;
    let dat = nyanpasu_observers::geo::fixtures::geoip_dat(&[("US", &["8.0.0.0/8"])]);
    h.geo.send_replace(Some(Arc::new(
        nyanpasu_geodata::IpIndex::from_geoip_dat(&dat).unwrap(),
    )));
    *h.local_source.0.lock().unwrap() = LocalSourceIps {
        ipv4: Some("8.8.8.8".parse().unwrap()),
        ipv6: None,
    };
    h.frames.send_replace(Some(raw_frame(
        100,
        200,
        vec![connection(A, 100, 200, json!({"sourceIP":"192.168.1.2"}))],
    )));
    until(&h.client, |s| s.last_sample_at.is_some()).await;
    let report = h
        .report(request(
            everything(),
            &[Dimension::Source, Dimension::SourceRegion],
        ))
        .await;
    assert_eq!(report.rankings[0].groups[0].key, "192.168.1.2");
    assert_eq!(report.rankings[1].groups[0].key, "US");
    assert_eq!(h.total(everything()).await, usage(100, 200, 1));

    *h.local_source.0.lock().unwrap() = LocalSourceIps::default();
    h.frames.send_replace(Some(raw_frame(
        150,
        260,
        vec![connection(A, 150, 260, json!({"sourceIP":"192.168.1.2"}))],
    )));
    until(&h.client, |s| s.current_rate.is_some()).await;
    let report = h
        .report(request(everything(), &[Dimension::SourceRegion]))
        .await;
    assert_eq!(report.rankings[0].groups[0].key, "unknown");
    assert_eq!(h.total(everything()).await, usage(150, 260, 1));
}
