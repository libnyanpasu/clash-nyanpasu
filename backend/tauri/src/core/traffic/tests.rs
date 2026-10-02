use std::{
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use nyanpasu_traffic::{
    ActiveConnection, Bytes, ClosedCursor, ClosedPage, Dimension, Dimensions, FlushBatch, Flushed,
    Frame, Metric, Rate, RedbTrafficStore, ReportRequest, RuleKey, Sample, SessionMeta, Tier,
    TopologyRequest, TrafficError, TrafficFilter, TrafficQuery, TrafficRange, TrafficResult,
    TrafficScope, TrafficStore, TrafficSummary, Usage, UsageGroup,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    Clock, ProfileSelection, RetentionPolicy, TrafficArgs, TrafficClient,
    source::frame_from_snapshot,
};
use crate::core::clash::ws::ClashConnectionsFrame;

const CORE: &str = "core-1";
const A: &str = "11111111-1111-1111-1111-111111111111";
const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

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

struct FakeRetention(Mutex<Option<Duration>>);

impl FakeRetention {
    fn keep(&self, retention: Option<Duration>) {
        *self.0.lock().unwrap() = retention;
    }

    fn keep_days(&self, days: i64) {
        self.keep(Some(Duration::from_millis((days * DAY) as u64)));
    }
}

impl RetentionPolicy for FakeRetention {
    fn retention(&self) -> Option<Duration> {
        *self.0.lock().unwrap()
    }
}

struct FakeClock(AtomicI64);

impl FakeClock {
    fn set(&self, now_ms: i64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A real store whose loads and flushes can be made to fail, as a full disk or a dying app would.
struct FlakyStore {
    inner: RedbTrafficStore,
    fail_load: AtomicBool,
    fail_flush: AtomicBool,
    fail_collect: AtomicBool,
    flushes: AtomicUsize,
    collections: AtomicUsize,
    collected: AtomicU64,
}

impl FlakyStore {
    fn fail_load(&self, fail: bool) {
        self.fail_load.store(fail, Ordering::SeqCst);
    }

    fn fail_flush(&self, fail: bool) {
        self.fail_flush.store(fail, Ordering::SeqCst);
    }

    fn fail_collect(&self, fail: bool) {
        self.fail_collect.store(fail, Ordering::SeqCst);
    }

    /// Flushes attempted, failed ones included.
    fn flushes(&self) -> usize {
        self.flushes.load(Ordering::SeqCst)
    }

    /// Collections attempted, failed ones included.
    fn collections(&self) -> usize {
        self.collections.load(Ordering::SeqCst)
    }

    /// Dimension combinations the collections deleted, in total.
    fn collected(&self) -> u64 {
        self.collected.load(Ordering::SeqCst)
    }
}

impl TrafficStore for FlakyStore {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>> {
        if self.fail_load.load(Ordering::SeqCst) {
            return Err(TrafficError::Storage("load failed on demand".into()));
        }
        self.inner.load()
    }

    fn flush(&self, batch: &FlushBatch) -> TrafficResult<Flushed> {
        self.flushes.fetch_add(1, Ordering::SeqCst);
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

    fn usage(&self, tier: Tier, from: Option<u32>) -> TrafficResult<Vec<(Arc<Dimensions>, Usage)>> {
        self.inner.usage(tier, from)
    }

    fn collect_tuples(&self, keep: &[Arc<Dimensions>]) -> TrafficResult<u64> {
        self.collections.fetch_add(1, Ordering::SeqCst);
        if self.fail_collect.load(Ordering::SeqCst) {
            return Err(TrafficError::Storage("collect failed on demand".into()));
        }
        let collected = self.inner.collect_tuples(keep)?;
        self.collected.fetch_add(collected, Ordering::SeqCst);
        Ok(collected)
    }
}

/// Everything the actor is injected with, except the frames' consumer.
struct Env {
    _dir: TempDir,
    store: Arc<FlakyStore>,
    profiles: Arc<FakeProfiles>,
    retention: Arc<FakeRetention>,
    clock: Arc<FakeClock>,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
    geo: watch::Sender<Option<Arc<nyanpasu_geodata::IpIndex>>>,
}

impl Env {
    async fn spawn(&self, shutdown: &CancellationToken, tasks: &TaskTracker) -> TrafficClient {
        self.try_spawn(shutdown, tasks).await.unwrap()
    }

    async fn try_spawn(
        &self,
        shutdown: &CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<TrafficClient> {
        TrafficClient::spawn(
            TrafficArgs {
                store: self.store.clone(),
                profiles: self.profiles.clone(),
                retention: self.retention.clone(),
                clock: self.clock.clone(),
                frames: self.frames.subscribe(),
                geo: self.geo.subscribe(),
            },
            shutdown.clone(),
            tasks,
        )
        .await
    }
}

/// A real actor over a real store. The frames watch never changes unless a test
/// sends on it, so the pump idles and tests drive the actor through the client.
/// The clock starts at 2 000 ms and the retention at seven days.
struct Harness {
    env: Env,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    client: TrafficClient,
}

impl Deref for Harness {
    type Target = Env;

    fn deref(&self) -> &Env {
        &self.env
    }
}

impl Harness {
    async fn new(profile: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            store: Arc::new(FlakyStore {
                inner: RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap(),
                fail_load: AtomicBool::new(false),
                fail_flush: AtomicBool::new(false),
                fail_collect: AtomicBool::new(false),
                flushes: AtomicUsize::new(0),
                collections: AtomicUsize::new(0),
                collected: AtomicU64::new(0),
            }),
            _dir: dir,
            profiles: Arc::new(FakeProfiles(Mutex::new(Some(profile.to_owned())))),
            retention: Arc::new(FakeRetention(Mutex::new(None))),
            clock: Arc::new(FakeClock(AtomicI64::new(2_000))),
            frames: watch::channel(None).0,
            geo: watch::channel(None).0,
        };
        env.retention.keep_days(7);
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = env.spawn(&shutdown, &tasks).await;
        Self {
            env,
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
        self.client = self.env.spawn(&self.shutdown, &self.tasks).await;
    }

    async fn observe(&self, frame: Frame) {
        self.client.observe(Some(frame)).await.unwrap();
    }

    /// A connection that opens at the start of `hour` and closes a minute later.
    async fn close_in_hour(&self, hour: i64) {
        self.close_in_hour_by(hour, "curl").await;
    }

    async fn close_in_hour_by(&self, hour: i64, process: &str) {
        let wall = hour * HOUR;
        let id = format!("c{hour}");
        self.observe(frame(
            wall,
            wall as u64,
            bytes(10, 10),
            vec![sample(&id, process, 10, 10)],
        ))
        .await;
        self.observe(frame(
            wall + MINUTE,
            (wall + MINUTE) as u64,
            bytes(10, 10),
            Vec::new(),
        ))
        .await;
    }

    async fn report(&self, request: ReportRequest) -> nyanpasu_traffic::TrafficReport {
        self.client.report(request).await.unwrap()
    }

    /// What `query` selects, in total.
    async fn total(&self, query: TrafficQuery) -> Usage {
        self.report(request(query, &[])).await.total
    }
}

fn bytes(upload: u64, download: u64) -> Bytes {
    Bytes { upload, download }
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

fn keys(groups: &[UsageGroup]) -> Vec<&str> {
    groups.iter().map(|g| g.key.as_str()).collect()
}

/// The one dimension combination a usage listing holds.
fn only(rows: Vec<(Arc<Dimensions>, Usage)>) -> Usage {
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows[0].1
}

#[tokio::test]
async fn summary_and_report_include_unflushed_deltas() {
    let h = Harness::new("p1").await;
    h.observe(a_first()).await;
    h.observe(a_second()).await;

    let rate = Rate {
        upload: 50,
        download: 60,
    };
    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.last_sample_at, Some(2_000));
    assert_eq!(summary.active_connections, 1);
    assert_eq!(summary.closed_connections, 0);
    assert_eq!(summary.current_rate, Some(rate));

    // A live connection reaches the usage tables only when it closes, so the answer comes from
    // memory both before and after a flush.
    let report = h.report(request(everything(), &[Dimension::Process])).await;
    assert!(h.store.usage(Tier::Hour, None).unwrap().is_empty());
    assert_eq!(report.total, usage(150, 260, 1));
    assert_eq!(report.current_rate, Some(rate));
    assert_eq!(report.rankings[0].groups.len(), 1);
    assert_eq!(report.rankings[0].groups[0].key, "curl");
    assert_eq!(report.rankings[0].groups[0].usage, usage(150, 260, 1));
    assert_eq!(report.rankings[0].groups[0].current_rate, Some(rate));

    h.client.flush().await.unwrap();
    assert!(h.store.usage(Tier::Hour, None).unwrap().is_empty());
    let (_, active) = h.store.load().unwrap().unwrap();
    assert_eq!(active[0].bytes(), bytes(150, 260));
    assert_eq!(
        h.report(request(everything(), &[Dimension::Process])).await,
        report
    );
}

#[tokio::test]
async fn reports_keep_the_top_rankings_and_merge_the_topology() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(60, 0),
        vec![
            sample("a", "small", 10, 0),
            sample("b", "large", 30, 0),
            sample("c", "medium", 20, 0),
        ],
    ))
    .await;

    let mut asked = request(everything(), &[Dimension::Process]);
    asked.ranking_limit = 2;
    asked.topology = Some(TopologyRequest {
        layers: vec![Dimension::Process, Dimension::Exit],
        metric: Metric::Bytes,
        limit_per_layer: Some(1),
    });
    let report = h.report(asked).await;
    assert_eq!(report.total, usage(60, 0, 3));
    let ranking = &report.rankings[0];
    assert_eq!(keys(&ranking.groups), ["large", "medium"]);
    assert_eq!(ranking.distinct, 3);
    assert_eq!(ranking.other, usage(10, 0, 1));

    // "large", the merged rest and the one exit.
    let topology = report.topology.unwrap();
    assert_eq!(topology.nodes.len(), 3);
    assert_eq!(topology.edges.len(), 2);
}

#[tokio::test]
async fn an_invalid_topology_is_refused() {
    let h = Harness::new("p1").await;
    let mut asked = request(everything(), &[]);
    asked.topology = Some(TopologyRequest {
        layers: vec![Dimension::Process],
        metric: Metric::Bytes,
        limit_per_layer: None,
    });
    let error = h.client.report(asked).await.unwrap_err().to_string();
    assert!(error.starts_with("invalid traffic request"), "{error}");
}

#[tokio::test]
async fn a_query_repeating_a_filter_dimension_is_refused_everywhere() {
    let h = Harness::new("p1").await;
    let repeated = || {
        query(
            TrafficRange::All,
            TrafficScope::All,
            &[(Dimension::Process, "curl"), (Dimension::Process, "wget")],
        )
    };
    let refused = |error: anyhow::Error| {
        let error = error.to_string();
        assert!(error.starts_with("invalid traffic request"), "{error}");
    };
    refused(h.client.report(request(repeated(), &[])).await.unwrap_err());
    refused(
        h.client
            .usage(repeated(), Dimension::Process, None, 10)
            .await
            .unwrap_err(),
    );
    refused(
        h.client
            .usage_by_keys(repeated(), Dimension::Process, vec!["curl".into()])
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn usage_pages_continue_after_the_cursor() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(70, 0),
        vec![
            sample("a", "small", 10, 0),
            sample("b", "large", 30, 0),
            sample("c", "medium", 20, 0),
            sample("d", "tie", 10, 0),
        ],
    ))
    .await;
    let h = &h;
    let page = |after, limit| async move {
        h.client
            .usage(everything(), Dimension::Process, after, limit)
            .await
            .unwrap()
    };

    let first = page(None, 2).await;
    assert_eq!(keys(&first.groups), ["large", "medium"]);
    assert_eq!(first.other.bytes, bytes(20, 0));

    // Equal traffic ranks by key; a page that reaches the end has no cursor.
    let last = page(first.next, 2).await;
    assert_eq!(last.total.bytes, bytes(70, 0));
    assert_eq!(keys(&last.groups), ["small", "tie"]);
    assert_eq!(last.other, Usage::default());
    assert_eq!(last.next, None);

    // A cursor on a tie continues with the next key of the same traffic.
    let first = page(None, 3).await;
    assert_eq!(keys(&first.groups), ["large", "medium", "small"]);
    assert_eq!(keys(&page(first.next, 3).await.groups), ["tie"]);
}

#[tokio::test]
async fn usage_by_keys_merges_stored_pending_and_live_traffic() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(15, 25),
        vec![sample("a", "curl", 10, 20), sample("b", "curl", 5, 5)],
    ))
    .await;
    // "a" closes and is stored; "b" closes and stays pending; "c" is live.
    h.observe(frame(
        2_000,
        1_000,
        bytes(18, 29),
        vec![sample("b", "curl", 8, 9)],
    ))
    .await;
    h.client.flush().await.unwrap();
    h.observe(frame(
        3_000,
        2_000,
        bytes(19, 30),
        vec![sample("c", "curl", 1, 1)],
    ))
    .await;
    assert_eq!(h.store.usage(Tier::Hour, None).unwrap().len(), 1);

    let h = &h;
    let by_keys = |scope| async move {
        h.client
            .usage_by_keys(
                query(TrafficRange::All, scope, &[]),
                Dimension::Process,
                vec!["missing".into(), "curl".into(), "curl".into()],
            )
            .await
            .unwrap()
    };
    // Missing and repeated keys are not listed.
    let all = by_keys(TrafficScope::All).await;
    assert_eq!(keys(&all), ["curl"]);
    assert_eq!(all[0].usage, usage(19, 30, 3));
    assert_eq!(
        by_keys(TrafficScope::Closed).await[0].usage,
        usage(18, 29, 2)
    );
    assert_eq!(by_keys(TrafficScope::Active).await[0].usage, usage(1, 1, 1));

    let rules = h
        .client
        .usage_by_keys(everything(), Dimension::Rule, vec!["Match".into()])
        .await
        .unwrap();
    assert_eq!(keys(&rules), ["Match"]);
    assert_eq!(rules[0].usage, usage(19, 30, 3));
}

#[tokio::test]
async fn a_profile_switch_keeps_the_data_and_the_filter_tells_the_profiles_apart() {
    async fn by_profile(h: &Harness, profile: &str) -> Usage {
        h.total(query(
            TrafficRange::All,
            TrafficScope::All,
            &[(Dimension::Profile, profile)],
        ))
        .await
    }

    let mut h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(10, 0),
        vec![sample("a", "curl", 10, 0)],
    ))
    .await;
    h.client.flush().await.unwrap();

    h.profiles.select("p2");
    // "a" keeps growing and "b" appears, now under p2.
    h.observe(frame(
        2_000,
        1_000,
        bytes(45, 0),
        vec![sample("a", "curl", 25, 0), sample("b", "wget", 20, 0)],
    ))
    .await;

    assert_eq!(h.total(everything()).await, usage(45, 0, 2));
    assert_eq!(by_profile(&h, "p1").await, usage(25, 0, 1));
    assert_eq!(by_profile(&h, "p2").await, usage(20, 0, 1));

    // The data is still there after a restart.
    h.restart().await;
    assert_eq!(h.total(everything()).await, usage(45, 0, 2));
    assert_eq!(by_profile(&h, "p1").await, usage(25, 0, 1));
    assert_eq!(h.client.summary().await.unwrap().active_connections, 2);
}

#[tokio::test]
async fn a_failed_flush_is_retried_and_written_exactly_once() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(11, 22),
        vec![sample("a", "curl", 10, 20), sample("b", "wget", 1, 2)],
    ))
    .await;
    // "a" is gone from the second frame: it closed.
    h.observe(frame(
        2_000,
        1_000,
        bytes(11, 22),
        vec![sample("b", "wget", 1, 2)],
    ))
    .await;

    h.store.fail_flush(true);
    h.client.flush().await.unwrap();
    assert!(h.store.load().unwrap().is_none());
    assert!(h.store.usage(Tier::Hour, None).unwrap().is_empty());
    assert!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .is_empty()
    );
    // Nothing is lost for the queries meanwhile.
    assert_eq!(h.total(everything()).await, usage(11, 22, 2));
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 1);

    h.store.fail_flush(false);
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 20, 1)
    );
    assert_eq!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .len(),
        1
    );
    let (_, active) = h.store.load().unwrap().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "b");

    // Another flush has nothing left to write again.
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 20, 1)
    );
    assert_eq!(h.total(everything()).await, usage(11, 22, 2));
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 1);
}

#[tokio::test]
async fn traffic_is_split_over_the_minute_and_hour_buckets_it_happened_in() {
    let h = Harness::new("p1").await;
    // 100 bytes at 00:59, 60 more at 01:01, closed at 01:02.
    h.observe(frame(
        59 * MINUTE,
        0,
        bytes(100, 0),
        vec![sample(A, "curl", 100, 0)],
    ))
    .await;
    h.observe(frame(
        61 * MINUTE,
        2 * MINUTE as u64,
        bytes(160, 0),
        vec![sample(A, "curl", 160, 0)],
    ))
    .await;
    h.observe(frame(
        62 * MINUTE,
        3 * MINUTE as u64,
        bytes(160, 0),
        Vec::new(),
    ))
    .await;
    h.clock.set(62 * MINUTE);
    h.client.flush().await.unwrap();

    let stored = |tier, from| only(h.store.usage(tier, Some(from)).unwrap());
    assert_eq!(stored(Tier::Hour, 0), usage(160, 0, 1));
    // The close is counted in the hour it happened in, with the traffic of that hour.
    assert_eq!(stored(Tier::Hour, 1), usage(60, 0, 1));
    assert!(h.store.usage(Tier::Hour, Some(2)).unwrap().is_empty());
    assert_eq!(stored(Tier::Minute, 59), usage(160, 0, 1));
    assert_eq!(stored(Tier::Minute, 60), usage(60, 0, 1));
    assert_eq!(stored(Tier::Minute, 62), usage(0, 0, 1));
    assert!(h.store.usage(Tier::Minute, Some(63)).unwrap().is_empty());

    // Two hours later the last hour no longer reaches it, the last six do.
    h.clock.set(62 * MINUTE + 2 * HOUR);
    let closed = |range| query(range, TrafficScope::Closed, &[]);
    assert_eq!(
        h.total(closed(TrafficRange::LastHour)).await,
        Usage::default()
    );
    assert_eq!(
        h.total(closed(TrafficRange::Last6Hours)).await,
        usage(160, 0, 1)
    );
    assert_eq!(
        h.total(closed(TrafficRange::Last24Hours)).await,
        usage(160, 0, 1)
    );
}

#[tokio::test]
async fn shortening_the_retention_deletes_what_it_no_longer_covers() {
    let h = Harness::new("p1").await;
    h.clock.set(2 * HOUR);
    h.observe(frame(
        HOUR,
        0,
        bytes(10, 10),
        vec![sample("a", "curl", 10, 10)],
    ))
    .await;
    h.observe(frame(
        HOUR + MINUTE,
        MINUTE as u64,
        bytes(10, 10),
        Vec::new(),
    ))
    .await;
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 10, 1)
    );

    // Three days on, seven days still cover it ...
    h.clock.set(3 * DAY);
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 10, 1)
    );
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 1);

    // ... one day does not, and the next flush notices.
    h.retention.keep_days(1);
    h.client.flush().await.unwrap();
    assert!(h.store.usage(Tier::Hour, None).unwrap().is_empty());
    assert!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .is_empty()
    );
    assert_eq!(h.client.summary().await.unwrap().closed_connections, 0);
    assert_eq!(h.total(everything()).await, Usage::default());
}

#[tokio::test]
async fn keeping_everything_deletes_nothing() {
    let h = Harness::new("p1").await;
    h.retention.keep(None);
    h.clock.set(2 * HOUR);
    h.observe(frame(
        HOUR,
        0,
        bytes(10, 10),
        vec![sample("a", "curl", 10, 10)],
    ))
    .await;
    h.observe(frame(
        HOUR + MINUTE,
        MINUTE as u64,
        bytes(10, 10),
        Vec::new(),
    ))
    .await;

    h.clock.set(400 * DAY);
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 10, 1)
    );
    assert_eq!(
        h.store
            .closed_connections(None, 10)
            .unwrap()
            .connections
            .len(),
        1
    );
    assert_eq!(h.total(everything()).await, usage(10, 10, 1));
}

#[tokio::test]
async fn unreferenced_dimensions_are_collected_after_hour_rows_expire_at_most_hourly() {
    let h = Harness::new("p1").await;
    // One closed connection each in hours 1, 24 and 25; a connection's hour rows reach the hour
    // tier when it closes. All are stored while the seven days still cover them.
    for hour in [1, 24, 25] {
        h.close_in_hour(hour).await;
    }
    // The first flush after a start reconciles once, whatever the previous run left behind.
    h.clock.set(2 * HOUR);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 1);

    // Hour 1 expires.
    h.clock.set(8 * DAY + 30 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // Hour 24 expires, but the last collection is less than an hour old.
    h.clock.set(8 * DAY + 61 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // Nothing expires.
    h.clock.set(8 * DAY + 62 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // Hour 25 expires, and the last collection is old enough.
    h.clock.set(8 * DAY + 2 * HOUR);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 3);

    // Keeping everything expires nothing.
    h.retention.keep(None);
    h.clock.set(40 * DAY);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 3);
}

#[tokio::test]
async fn a_collection_still_due_runs_once_the_cooldown_passes() {
    let h = Harness::new("p1").await;
    for hour in [1, 24] {
        h.close_in_hour(hour).await;
    }
    h.clock.set(2 * HOUR);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 1);

    // Hour 1 expires.
    h.clock.set(8 * DAY + 30 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // Hour 24 expires within the cooldown: its collection waits.
    h.clock.set(8 * DAY + 61 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // Nothing expires any more, but the cooldown is over.
    h.clock.set(8 * DAY + 90 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 3);

    // And once it ran, nothing is due.
    h.clock.set(8 * DAY + 3 * HOUR);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 3);
}

#[tokio::test]
async fn a_failed_collection_is_retried_by_a_later_flush() {
    let h = Harness::new("p1").await;
    h.close_in_hour(1).await;
    h.clock.set(2 * HOUR);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 1);

    h.store.fail_collect(true);
    h.clock.set(8 * DAY + 30 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 2);

    // No hour row expires now, and the failure started no cooldown.
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 3);

    h.store.fail_collect(false);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 4);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 4);
}

#[tokio::test]
async fn a_restart_collects_what_expired_while_the_cooldown_still_ran() {
    let mut h = Harness::new("p1").await;
    // Distinct processes: each expired hour orphans a dimension combination of its own.
    h.close_in_hour_by(1, "curl").await;
    h.close_in_hour_by(24, "wget").await;
    h.clock.set(2 * HOUR);
    h.client.flush().await.unwrap();

    // Hour 1 expires and its combination is collected.
    h.clock.set(8 * DAY + 30 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collected(), 1);

    // Hour 24 expires within the cooldown: the app ends before its collection may run.
    h.clock.set(8 * DAY + 61 * MINUTE);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collected(), 1);

    // No hour row expires after the restart, yet the orphan is collected.
    h.restart().await;
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collected(), 2);
}

#[tokio::test]
async fn a_restart_while_writes_fail_reconciles_once_they_succeed() {
    let mut h = Harness::new("p1").await;
    h.store.fail_flush(true);
    h.restart().await;
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 0);

    h.store.fail_flush(false);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 1);
    h.client.flush().await.unwrap();
    assert_eq!(h.store.collections(), 1);
}

#[tokio::test]
async fn a_store_that_keeps_refusing_neither_grows_the_session_nor_loses_what_it_held() {
    let mut h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(11, 22),
        vec![sample("a", "curl", 10, 20), sample("b", "wget", 1, 2)],
    ))
    .await;
    // "a" is gone from the second frame: it closed.
    h.observe(frame(
        2_000,
        1_000,
        bytes(11, 22),
        vec![sample("b", "wget", 1, 2)],
    ))
    .await;
    assert!(h.client.summary().await.unwrap().current_rate.is_some());

    h.store.fail_flush(true);
    h.client.flush().await.unwrap();
    // The stale rate is gone, and so is every frame that comes now.
    assert_eq!(h.client.summary().await.unwrap().current_rate, None);
    let held = |summary: TrafficSummary| {
        (
            summary.active_connections,
            summary.closed_connections,
            summary.last_sample_at,
            summary.current_rate,
        )
    };
    let before = held(h.client.summary().await.unwrap());
    assert_eq!(before, (1, 1, Some(2_000), None));
    h.observe(frame(
        3_000,
        2_000,
        bytes(30, 30),
        vec![sample("b", "wget", 5, 5), sample("c", "curl", 7, 7)],
    ))
    .await;
    h.observe(frame(
        4_000,
        3_000,
        bytes(30, 30),
        vec![sample("b", "wget", 9, 9)],
    ))
    .await;
    for _ in 0..3 {
        h.client.flush().await.unwrap();
        assert_eq!(held(h.client.summary().await.unwrap()), before);
        assert_eq!(h.total(everything()).await, usage(11, 22, 2));
    }
    assert!(h.store.usage(Tier::Hour, None).unwrap().is_empty());

    // The store accepts again: what was held is written, once.
    h.store.fail_flush(false);
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 20, 1)
    );
    let (_, active) = h.store.load().unwrap().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].bytes(), bytes(1, 2));
    h.client.flush().await.unwrap();
    assert_eq!(
        only(h.store.usage(Tier::Hour, None).unwrap()),
        usage(10, 20, 1)
    );

    // Observing resumes: "b" brings what it grew since the last accepted frame, "c" is lost.
    h.observe(frame(
        5_000,
        4_000,
        bytes(30, 30),
        vec![sample("b", "wget", 9, 9)],
    ))
    .await;
    assert_eq!(h.total(everything()).await, usage(19, 29, 2));
    assert_eq!(
        h.client.summary().await.unwrap().last_sample_at,
        Some(5_000)
    );
    h.client.flush().await.unwrap();

    // Nothing is counted twice across a restart, All is still Active plus Closed.
    h.restart().await;
    assert_eq!(h.total(everything()).await, usage(19, 29, 2));
    let scoped = |scope| query(TrafficRange::All, scope, &[]);
    let active = h.total(scoped(TrafficScope::Active)).await;
    let closed = h.total(scoped(TrafficScope::Closed)).await;
    assert_eq!(active, usage(9, 9, 1));
    assert_eq!(closed, usage(10, 20, 1));
    assert_eq!(h.client.summary().await.unwrap().active_connections, 1);
}

#[tokio::test]
async fn an_unreadable_store_disables_recording_and_is_never_written() {
    let h = Harness::new("p1").await;
    h.observe(a_first()).await;
    h.shutdown.cancel();
    h.tasks.close();
    h.tasks.wait().await;
    let flushes = h.store.flushes();

    h.store.fail_load(true);
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    assert!(h.try_spawn(&shutdown, &tasks).await.is_err());
    // Nothing is left running: no registered drain, no pump on the feed.
    assert!(tasks.is_empty());
    assert_eq!(h.frames.receiver_count(), 0);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    assert_eq!(h.store.flushes(), flushes);

    // What the store holds is untouched, for the next start to read.
    h.store.fail_load(false);
    let (_, active) = h.store.load().unwrap().unwrap();
    assert_eq!(active.len(), 1);
}

#[tokio::test]
async fn the_active_rate_sums_the_connection_rates_and_a_closed_view_has_none() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(110, 0),
        vec![sample("a", "curl", 100, 0), sample("b", "wget", 10, 0)],
    ))
    .await;
    h.observe(frame(
        2_000,
        1_000,
        bytes(180, 0),
        vec![sample("a", "curl", 150, 0), sample("b", "wget", 30, 0)],
    ))
    .await;

    let rated = |scope| {
        request(
            query(TrafficRange::All, scope, &[]),
            &[Dimension::Process, Dimension::Source],
        )
    };
    let rate = |upload| {
        Some(Rate {
            upload,
            download: 0,
        })
    };
    let report = h.report(rated(TrafficScope::Active)).await;
    assert_eq!(report.current_rate, rate(70));
    assert_eq!(report.rankings[0].groups[0].current_rate, rate(50));
    assert_eq!(report.rankings[0].groups[1].current_rate, rate(20));
    // Both connections share their source.
    assert_eq!(report.rankings[1].groups[0].current_rate, rate(70));

    // "b" closes and "a" slows down.
    h.observe(frame(
        3_000,
        2_000,
        bytes(190, 0),
        vec![sample("a", "curl", 160, 0)],
    ))
    .await;
    let closed = h.report(rated(TrafficScope::Closed)).await;
    assert_eq!(closed.current_rate, None);
    assert_eq!(keys(&closed.rankings[0].groups), ["wget"]);
    assert_eq!(closed.rankings[0].groups[0].current_rate, None);

    let all = h.report(rated(TrafficScope::All)).await;
    assert_eq!(all.current_rate, rate(10));
    let groups = &all.rankings[0].groups;
    assert_eq!(
        (groups[0].key.as_str(), groups[0].current_rate),
        ("curl", rate(10))
    );
    assert_eq!(
        (groups[1].key.as_str(), groups[1].current_rate),
        ("wget", None)
    );
}

#[tokio::test]
async fn restart_with_the_same_instance_does_not_recount() {
    let mut h = Harness::new("p1").await;
    h.observe(a_first()).await;
    // The stop flushes what the interval has not, and the new actor restores it.
    h.restart().await;

    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.active_connections, 1);
    assert_eq!(h.total(everything()).await, usage(100, 200, 1));

    h.observe(a_second()).await;
    assert_eq!(h.total(everything()).await, usage(150, 260, 1));
}

#[tokio::test]
async fn closed_connections_appear_before_and_after_flush() {
    let h = Harness::new("p1").await;
    h.observe(frame(
        1_000,
        0,
        bytes(11, 22),
        vec![sample("a", "curl", 10, 20), sample("b", "wget", 1, 2)],
    ))
    .await;
    // "a" is gone from the second frame: it closed.
    h.observe(frame(
        2_000,
        1_000,
        bytes(11, 22),
        vec![sample("b", "wget", 1, 2)],
    ))
    .await;

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
    assert_eq!(
        closed.dimensions,
        Dimensions {
            profile: Some("p1".into()),
            ..dims("curl")
        }
    );
}

#[tokio::test]
async fn lost_feed_clears_the_current_rate() {
    let h = Harness::new("p1").await;
    h.observe(a_first()).await;
    h.observe(a_second()).await;
    assert!(h.client.summary().await.unwrap().current_rate.is_some());

    h.client.observe(None).await.unwrap();
    let summary = h.client.summary().await.unwrap();
    assert_eq!(summary.current_rate, None);
    // Baselines survive, so the connection is still active and not recounted.
    assert_eq!(summary.active_connections, 1);
    assert_eq!(h.total(everything()).await, usage(150, 260, 1));
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
    let dat = crate::core::geo::fixtures::geoip_dat(&[("US", &["8.0.0.0/8"])]);
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
    let frame = frame_from_snapshot(&raw, 7_000, Duration::from_millis(9), None);

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
            inbound: "unknown".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "DomainSuffix".into(),
                payload: "example.com".into(),
            },
            chains: vec!["Node-A".into(), "Proxy".into()],
            profile: None,
            source_region: "unknown".into(),
            destination_region: "unknown".into(),
            destination_basis: None,
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

#[test]
fn frame_from_snapshot_maps_the_inbound_and_the_regions() {
    let with = |metadata| connection(A, 0, 0, metadata);
    let raw = raw_frame(
        0,
        0,
        vec![
            // The user wins over the listener, and codes are normalized.
            with(json!({
                "inboundName": "mixed",
                "inboundUser": "alice",
                "sourceGeoIP": ["cn"],
                "destinationGeoIP": ["us", "US"],
            })),
            with(json!({
                "inboundName": "mixed",
                "inboundUser": "",
                "destinationGeoIP": ["us", "jp"],
            })),
            with(json!({ "sourceGeoIP": [], "destinationGeoIP": null })),
        ],
    );
    let frame = frame_from_snapshot(&raw, 0, Duration::ZERO, None);
    let regions = |i: usize| {
        let d = &frame.connections[i].dimensions;
        (
            d.inbound.as_str(),
            d.source_region.as_str(),
            d.destination_region.as_str(),
        )
    };
    assert_eq!(regions(0), ("alice", "CN", "US"));
    // Two distinct codes are ambiguous.
    assert_eq!(regions(1), ("mixed", "unknown", "unknown"));
    assert_eq!(regions(2), ("unknown", "unknown", "unknown"));
}
