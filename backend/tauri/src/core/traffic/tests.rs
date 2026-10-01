use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nyanpasu_traffic::{
    ActiveConnection, Bytes, ClosedCursor, ClosedPage, Dimensions, FlushBatch, Frame, GroupBy,
    Rate, RedbTrafficStore, RuleKey, Sample, SessionMeta, TopologyKey, TrafficError, TrafficResult,
    TrafficStore, TrafficSummary, UsageGroup,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{ProfileSelection, TrafficArgs, TrafficClient, source::frame_from_snapshot};
use crate::core::clash::ws::ClashConnectionsFrame;

const CORE: &str = "core-1";
const A: &str = "11111111-1111-1111-1111-111111111111";

struct FakeProfiles(Mutex<Option<String>>);

impl FakeProfiles {
    fn select(&self, id: &str) {
        *self.0.lock().unwrap() = Some(id.to_owned());
    }
}

impl ProfileSelection for FakeProfiles {
    fn current(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A real store whose flushes can be made to fail, as a full disk or a dying app would.
struct FlakyStore {
    inner: RedbTrafficStore,
    fail_flush: AtomicBool,
}

impl FlakyStore {
    fn fail_flush(&self, fail: bool) {
        self.fail_flush.store(fail, Ordering::SeqCst);
    }
}

impl TrafficStore for FlakyStore {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>> {
        self.inner.load()
    }

    fn flush(&self, batch: &FlushBatch) -> TrafficResult<()> {
        if self.fail_flush.load(Ordering::SeqCst) {
            return Err(TrafficError::Storage("flush failed on demand".into()));
        }
        self.inner.flush(batch)
    }

    fn closed_connections(
        &self,
        before: Option<&ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage> {
        self.inner.closed_connections(before, limit)
    }

    fn closed_count(&self) -> TrafficResult<u64> {
        self.inner.closed_count()
    }

    fn totals(&self, group: GroupBy) -> TrafficResult<Vec<(String, Bytes)>> {
        self.inner.totals(group)
    }

    fn totals_of(&self, group: GroupBy, keys: &[String]) -> TrafficResult<Vec<(String, Bytes)>> {
        self.inner.totals_of(group, keys)
    }

    fn topology(&self) -> TrafficResult<Vec<(TopologyKey, Bytes)>> {
        self.inner.topology()
    }
}

/// A real actor over a real store. The frames watch never changes unless a test
/// sends on it, so the pump idles and tests drive the actor through the client.
struct Harness {
    _dir: TempDir,
    store: Arc<FlakyStore>,
    profiles: Arc<FakeProfiles>,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    client: TrafficClient,
}

impl Harness {
    async fn new(profile: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(FlakyStore {
            inner: RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap(),
            fail_flush: AtomicBool::new(false),
        });
        let profiles = Arc::new(FakeProfiles(Mutex::new(Some(profile.to_owned()))));
        let frames = watch::channel(None).0;
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = spawn(&store, &profiles, &frames, &shutdown, &tasks).await;
        Self {
            _dir: dir,
            store,
            profiles,
            frames,
            shutdown,
            tasks,
            client,
        }
    }

    /// Stops the actor as application shutdown does, `post_stop` included, and
    /// starts a new one over the same store, as an app restart would.
    async fn restart(&mut self) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
        self.shutdown = CancellationToken::new();
        self.tasks = TaskTracker::new();
        self.client = spawn(
            &self.store,
            &self.profiles,
            &self.frames,
            &self.shutdown,
            &self.tasks,
        )
        .await;
    }
}

async fn spawn(
    store: &Arc<FlakyStore>,
    profiles: &Arc<FakeProfiles>,
    frames: &watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
) -> TrafficClient {
    TrafficClient::spawn(
        TrafficArgs {
            store: store.clone(),
            profiles: profiles.clone(),
            frames: frames.subscribe(),
        },
        shutdown.clone(),
        tasks,
    )
    .await
    .unwrap()
}

fn bytes(upload: u64, download: u64) -> Bytes {
    Bytes { upload, download }
}

fn dims(process: &str) -> Dimensions {
    Dimensions {
        process: process.into(),
        source: "192.168.1.2".into(),
        target: "example.com".into(),
        protocol: "tcp".into(),
        rule: RuleKey {
            kind: "Match".into(),
            payload: String::new(),
        },
        chains: vec!["Node-A".into(), "Proxy".into()],
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

/// The first frame of connection `a` and the follow-up 1s later that adds
/// 50 upload and 60 download bytes.
fn a_first() -> Frame {
    frame(1_000, 0, bytes(100, 200), vec![sample(A, "curl", 100, 200)])
}

fn a_second() -> Frame {
    frame(
        2_000,
        1_000,
        bytes(150, 260),
        vec![sample(A, "curl", 150, 260)],
    )
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
        snapshot: clash_api::ConnectionsSnapshot {
            upload_total,
            download_total,
            connections: Some(connections),
            memory: None,
        },
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
async fn summary_and_usage_include_unflushed_deltas() {
    let h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.observe(Some(a_second())).await.unwrap();

    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.profile.as_deref(), Some("p1"));
    assert_eq!(summary.last_sample_at, Some(2_000));
    assert_eq!(summary.core_bytes, bytes(150, 260));
    assert_eq!(summary.active_connections, 1);
    assert_eq!(
        summary.current_rate,
        Some(Rate {
            upload: 50,
            download: 60
        })
    );

    // Nothing is flushed yet, so what the query sees is all pending.
    assert!(h.store.totals(GroupBy::Process).unwrap().is_empty());
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(150, 260));
    assert_eq!(usage.other, Bytes::default());
    assert_eq!(usage.groups.len(), 1);
    assert_eq!(usage.groups[0].key, "curl");
    assert_eq!(usage.groups[0].bytes, bytes(150, 260));
    assert_eq!(
        usage.groups[0].current_rate,
        Some(Rate {
            upload: 50,
            download: 60
        })
    );

    // Flushing moves the deltas to the store without changing the answer.
    h.client.flush().await.unwrap();
    assert_eq!(
        h.store.totals(GroupBy::Process).unwrap(),
        [("curl".to_owned(), bytes(150, 260))]
    );
    assert_eq!(
        h.client.usage(GroupBy::Process, None, 10).await.unwrap(),
        usage
    );
}

#[tokio::test]
async fn usage_and_topology_keep_the_top_entries() {
    let h = Harness::new("p1").await;
    h.client
        .observe(Some(frame(
            1_000,
            0,
            bytes(60, 0),
            vec![
                sample("a", "small", 10, 0),
                sample("b", "large", 30, 0),
                sample("c", "medium", 20, 0),
            ],
        )))
        .await
        .unwrap();

    let usage = h.client.usage(GroupBy::Process, None, 2).await.unwrap();
    assert_eq!(usage.total, bytes(60, 0));
    assert_eq!(
        usage
            .groups
            .iter()
            .map(|g| g.key.as_str())
            .collect::<Vec<_>>(),
        ["large", "medium"]
    );
    assert_eq!(usage.other, bytes(10, 0));

    let topology = h.client.topology(1).await.unwrap();
    assert_eq!(topology.paths.len(), 1);
    assert_eq!(topology.paths[0].key.source, "large");
    assert_eq!(topology.paths[0].bytes, bytes(30, 0));
    assert_eq!(topology.other, bytes(30, 0));
    assert!(!topology.nodes.is_empty() && !topology.edges.is_empty());
}

fn keys(groups: &[UsageGroup]) -> Vec<&str> {
    groups.iter().map(|g| g.key.as_str()).collect()
}

#[tokio::test]
async fn usage_pages_continue_after_the_cursor() {
    let h = Harness::new("p1").await;
    h.client
        .observe(Some(frame(
            1_000,
            0,
            bytes(70, 0),
            vec![
                sample("a", "small", 10, 0),
                sample("b", "large", 30, 0),
                sample("c", "medium", 20, 0),
                sample("d", "tie", 10, 0),
            ],
        )))
        .await
        .unwrap();

    let first = h.client.usage(GroupBy::Process, None, 2).await.unwrap();
    assert_eq!(keys(&first.groups), ["large", "medium"]);
    assert_eq!(first.other, bytes(20, 0));

    // Equal traffic ranks by key; a page that reaches the end has no cursor.
    let last = h
        .client
        .usage(GroupBy::Process, first.next, 2)
        .await
        .unwrap();
    assert_eq!(last.total, bytes(70, 0));
    assert_eq!(keys(&last.groups), ["small", "tie"]);
    assert_eq!(last.other, Bytes::default());
    assert_eq!(last.next, None);

    // A cursor on a tie continues with the next key of the same traffic.
    let first = h.client.usage(GroupBy::Process, None, 3).await.unwrap();
    assert_eq!(keys(&first.groups), ["large", "medium", "small"]);
    let last = h
        .client
        .usage(GroupBy::Process, first.next, 3)
        .await
        .unwrap();
    assert_eq!(keys(&last.groups), ["tie"]);
}

#[tokio::test]
async fn usage_by_keys_merges_stored_and_pending_traffic() {
    let h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.flush().await.unwrap();
    h.client.observe(Some(a_second())).await.unwrap();

    // `curl` has 100/200 stored and 50/60 pending; missing and repeated keys are not listed.
    let usage = h
        .client
        .usage_by_keys(
            GroupBy::Process,
            vec!["missing".into(), "curl".into(), "curl".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        usage,
        [UsageGroup {
            key: "curl".into(),
            bytes: bytes(150, 260),
            current_rate: Some(Rate {
                upload: 50,
                download: 60
            }),
        }]
    );

    let rules = h
        .client
        .usage_by_keys(GroupBy::Rule, vec!["Match".into()])
        .await
        .unwrap();
    assert_eq!(keys(&rules), ["Match"]);
    assert_eq!(rules[0].bytes, bytes(150, 260));
}

#[tokio::test]
async fn profile_switch_wipes_stored_totals_but_keeps_baselines() {
    let h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.flush().await.unwrap();
    assert!(!h.store.totals(GroupBy::Process).unwrap().is_empty());

    h.profiles.select("p2");
    h.client.observe(Some(a_second())).await.unwrap();

    // The wipe is immediate; the switch does not wait for the next flush.
    assert!(h.store.totals(GroupBy::Process).unwrap().is_empty());
    let (meta, _) = h.store.load().unwrap().unwrap();
    assert_eq!(meta.profile.as_deref(), Some("p2"));

    // The surviving connection only counts what it transferred after the switch.
    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.profile.as_deref(), Some("p2"));
    assert_eq!(summary.started_at, 2_000);
    assert_eq!(summary.core_bytes, bytes(50, 60));
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(50, 60));

    h.client.flush().await.unwrap();
    assert_eq!(
        h.store.totals(GroupBy::Process).unwrap(),
        [("curl".to_owned(), bytes(50, 60))]
    );
}

#[tokio::test]
async fn a_crash_after_a_profile_switch_does_not_recount_surviving_connections() {
    let mut h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.flush().await.unwrap();

    h.profiles.select("p2");
    h.client.observe(Some(a_second())).await.unwrap();

    // No flush since the switch, yet the store already holds the new session and the
    // baseline the surviving connection is measured against.
    let (meta, active) = h.store.load().unwrap().unwrap();
    assert_eq!(meta.profile.as_deref(), Some("p2"));
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, A);
    assert_eq!(active[0].counters, bytes(100, 200));
    assert_eq!(active[0].bytes, Bytes::default());
    assert!(h.store.totals(GroupBy::Process).unwrap().is_empty());

    // The app dies before the next flush: the stop cannot persist anything further.
    h.store.fail_flush(true);
    h.restart().await;
    h.store.fail_flush(false);

    h.client.observe(Some(a_second())).await.unwrap();
    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.profile.as_deref(), Some("p2"));
    assert_eq!(summary.core_bytes, bytes(50, 60));
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(50, 60));
}

#[tokio::test]
async fn a_failed_reset_flush_hides_the_previous_session_until_it_is_retried() {
    let h = Harness::new("p1").await;
    h.client
        .observe(Some(frame(
            1_000,
            0,
            bytes(11, 22),
            vec![sample("a", "curl", 10, 20), sample("b", "wget", 1, 2)],
        )))
        .await
        .unwrap();
    h.client
        .observe(Some(frame(
            2_000,
            1_000,
            bytes(11, 22),
            vec![sample("b", "wget", 1, 2)],
        )))
        .await
        .unwrap();
    h.client.flush().await.unwrap();
    assert_eq!(h.store.totals(GroupBy::Process).unwrap().len(), 2);
    assert_eq!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .len(),
        1
    );

    h.store.fail_flush(true);
    h.profiles.select("p2");
    h.client
        .observe(Some(frame(
            3_000,
            2_000,
            bytes(14, 26),
            vec![sample("b", "wget", 4, 6)],
        )))
        .await
        .unwrap();

    // The wipe did not land, so the store still holds p1 ...
    assert_eq!(h.store.totals(GroupBy::Process).unwrap().len(), 2);
    assert_eq!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .len(),
        1
    );
    // ... and none of it leaks into the p2 session.
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(3, 4));
    assert_eq!(usage.groups.len(), 1);
    assert_eq!(usage.groups[0].key, "wget");
    let topology = h.client.topology(10).await.unwrap();
    assert_eq!(topology.paths.len(), 1);
    assert_eq!(topology.paths[0].bytes, bytes(3, 4));
    assert_eq!(
        h.client.closed_connections(None, 10).await.unwrap(),
        ClosedPage {
            connections: Vec::new(),
            next: None
        }
    );
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 0);

    // The next flush carries the wipe again.
    h.store.fail_flush(false);
    h.client.flush().await.unwrap();
    assert_eq!(
        h.store.totals(GroupBy::Process).unwrap(),
        [("wget".to_owned(), bytes(3, 4))]
    );
    assert!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .is_empty()
    );
    let (meta, _) = h.store.load().unwrap().unwrap();
    assert_eq!(meta.profile.as_deref(), Some("p2"));
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(3, 4));
}

#[tokio::test]
async fn restart_with_the_same_profile_and_instance_does_not_recount() {
    let mut h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    // The stop flushes what the interval has not, and the new actor restores it.
    h.restart().await;

    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.core_bytes, bytes(100, 200));
    assert_eq!(summary.active_connections, 1);

    h.client.observe(Some(a_second())).await.unwrap();
    assert_eq!(
        h.client.summary().await.unwrap().core_bytes,
        bytes(150, 260)
    );
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(150, 260));
}

#[tokio::test]
async fn restart_with_another_profile_wipes_the_store() {
    let mut h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.flush().await.unwrap();
    assert!(!h.store.totals(GroupBy::Process).unwrap().is_empty());

    h.profiles.select("p2");
    h.restart().await;

    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.profile.as_deref(), Some("p2"));
    assert_eq!(summary.core_bytes, Bytes::default());
    assert_eq!(summary.active_connections, 0);
    assert!(h.store.totals(GroupBy::Process).unwrap().is_empty());
    let (meta, active) = h.store.load().unwrap().unwrap();
    assert_eq!(meta.profile.as_deref(), Some("p2"));
    assert!(active.is_empty());
}

#[tokio::test]
async fn closed_connections_appear_before_and_after_flush() {
    let h = Harness::new("p1").await;
    h.client
        .observe(Some(frame(
            1_000,
            0,
            bytes(11, 22),
            vec![sample("a", "curl", 10, 20), sample("b", "wget", 1, 2)],
        )))
        .await
        .unwrap();
    // "a" is gone from the second frame: it closed.
    h.client
        .observe(Some(frame(
            2_000,
            1_000,
            bytes(11, 22),
            vec![sample("b", "wget", 1, 2)],
        )))
        .await
        .unwrap();

    // Not stored yet, but already listed ...
    let pending = h.client.closed_connections(None, 10).await.unwrap();
    assert!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .is_empty()
    );

    assert_eq!(h.client.summary().await.unwrap().closed_connections, 1);

    // ... and listed once more after the flush moves it to the store.
    h.client.flush().await.unwrap();
    let page = h.client.closed_connections(None, 10).await.unwrap();
    assert_eq!(page, pending);
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 1);
    assert_eq!(page.next, None);
    assert_eq!(page.connections.len(), 1);
    let closed = &page.connections[0];
    assert_eq!(closed.id, "a");
    assert_eq!(closed.closed_at, 2_000);
    assert_eq!(closed.bytes, bytes(10, 20));
    assert_eq!(closed.dimensions, dims("curl"));
}

#[tokio::test]
async fn lost_feed_clears_the_current_rate() {
    let h = Harness::new("p1").await;
    h.client.observe(Some(a_first())).await.unwrap();
    h.client.observe(Some(a_second())).await.unwrap();
    assert!(h.client.summary().await.unwrap().current_rate.is_some());

    h.client.observe(None).await.unwrap();
    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.current_rate, None);
    // Baselines survive, so the connection is still active and not recounted.
    assert_eq!(summary.active_connections, 1);
    assert_eq!(summary.core_bytes, bytes(150, 260));
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

    h.frames.send_replace(Some(raw_frame(
        150,
        260,
        vec![connection(A, 150, 260, json!(null))],
    )));
    until(&h.client, |s| s.current_rate.is_some()).await;
    let usage = h.client.usage(GroupBy::Process, None, 10).await.unwrap();
    assert_eq!(usage.total, bytes(150, 260));

    h.frames.send_replace(None);
    until(&h.client, |s| s.current_rate.is_none()).await;
}

#[test]
fn frame_from_snapshot_extracts_dimensions() {
    let connections = vec![
        connection(
            A,
            10,
            20,
            json!({
                "network": "tcp",
                "sourceIP": "192.168.1.2",
                "destinationIP": "1.1.1.1",
                "host": "example.com",
                "process": "curl",
                "processPath": "C:\\Tools\\curl.exe",
            }),
        ),
        // Empty fields fall through: no path, no host.
        connection(
            "22222222-2222-2222-2222-222222222222",
            1,
            2,
            json!({
                "network": "udp",
                "destinationIP": "1.1.1.1",
                "host": "",
                "process": "dns",
                "processPath": "",
            }),
        ),
        connection("33333333-3333-3333-3333-333333333333", -5, -6, json!(null)),
        connection(
            "44444444-4444-4444-4444-444444444444",
            0,
            0,
            json!({ "network": "icmp" }),
        ),
    ];
    let raw = raw_frame(-1, 30, connections);
    let frame = frame_from_snapshot(&raw, 7_000, Duration::from_millis(9));

    assert_eq!(frame.instance_id, CORE);
    assert_eq!(frame.wall_ms, 7_000);
    assert_eq!(frame.mono, Duration::from_millis(9));
    assert_eq!(frame.totals, bytes(0, 30));
    assert_eq!(frame.connections.len(), 4);

    let full = &frame.connections[0];
    assert_eq!(full.id, A);
    assert_eq!(full.started_at, 1_704_067_200_000);
    assert_eq!(full.counters, bytes(10, 20));
    assert_eq!(
        full.dimensions,
        Dimensions {
            process: "C:/Tools/curl.exe".into(),
            source: "192.168.1.2".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "DomainSuffix".into(),
                payload: "example.com".into(),
            },
            chains: vec!["Node-A".into(), "Proxy".into()],
        }
    );

    let fallback = &frame.connections[1].dimensions;
    assert_eq!(fallback.process, "dns");
    assert_eq!(fallback.source, "unknown");
    assert_eq!(fallback.target, "1.1.1.1");
    assert_eq!(fallback.protocol, "udp");

    let bare = &frame.connections[2];
    assert_eq!(bare.counters, bytes(0, 0));
    assert_eq!(bare.dimensions.process, "unknown");
    assert_eq!(bare.dimensions.source, "unknown");
    assert_eq!(bare.dimensions.target, "unknown");
    assert_eq!(bare.dimensions.protocol, "unknown");

    assert_eq!(frame.connections[3].dimensions.protocol, "icmp");
}
