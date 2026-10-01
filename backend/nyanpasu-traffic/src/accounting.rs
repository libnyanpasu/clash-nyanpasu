use crate::model::*;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Sample {
    pub id: String,
    /// Connection start, wall clock milliseconds.
    pub started_at: i64,
    /// Cumulative counters as reported by the core.
    pub counters: Bytes,
    pub dimensions: Dimensions,
}

#[derive(Clone, Debug)]
pub struct Frame {
    /// Identifies the core process; counters and connection ids are only comparable within it.
    pub instance_id: String,
    pub wall_ms: i64,
    /// Monotonic clock reading, used for rates only.
    pub mono: Duration,
    /// The core's global counters.
    pub totals: Bytes,
    pub connections: Vec<Sample>,
}

#[derive(Clone, Debug)]
pub struct FlushBatch {
    /// Wipe everything stored before applying the batch: it starts a new session, so nothing
    /// of the previous one may survive, and the wipe carries the baselines it must not lose.
    pub reset: bool,
    pub meta: SessionMeta,
    pub active: Vec<ActiveConnection>,
    /// Deltas since the previous batch.
    pub totals: Vec<(Dimension, String, Bytes)>,
    pub topology: Vec<(TopologyKey, Bytes)>,
    pub closed: Vec<ClosedConnection>,
}

/// The dimensions the v1 store keeps totals for.
// FIXME(actor-migration): legacy behavior kept temporarily for the v1 totals table.
// New code must use `Dimension::ALL`. Remove with `pending_totals` once the store records buckets.
const STORED: [Dimension; 7] = [
    Dimension::Process,
    Dimension::Source,
    Dimension::Target,
    Dimension::Protocol,
    Dimension::Rule,
    Dimension::Exit,
    Dimension::Chain,
];

pub struct Session {
    meta: SessionMeta,
    active: HashMap<String, ActiveConnection>,
    /// Rates from the latest frame; only connections seen in two consecutive frames have one.
    rates: HashMap<String, Rate>,
    current_rate: Option<Rate>,
    last_mono: Option<Duration>,
    pending_totals: HashMap<Dimension, HashMap<String, Bytes>>,
    pending_topology: HashMap<TopologyKey, Bytes>,
    closed: Vec<ClosedConnection>,
    /// The store still holds another session until a batch with `reset` lands.
    reset_pending: bool,
}

impl Session {
    pub fn new(profile: Option<String>, now_ms: i64) -> Self {
        Self {
            reset_pending: true,
            ..Self::restore(
                SessionMeta {
                    profile,
                    started_at: now_ms,
                    last_sample_at: None,
                    core_bytes: Bytes::default(),
                    instance_id: None,
                    global_counters: None,
                },
                Vec::new(),
            )
        }
    }

    /// Rates stay unknown until two frames have been observed.
    pub fn restore(meta: SessionMeta, active: Vec<ActiveConnection>) -> Self {
        Self {
            meta,
            active: active.into_iter().map(|c| (c.id.clone(), c)).collect(),
            rates: HashMap::new(),
            current_rate: None,
            last_mono: None,
            pending_totals: HashMap::new(),
            pending_topology: HashMap::new(),
            closed: Vec::new(),
            reset_pending: false,
        }
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    pub fn observe(&mut self, frame: &Frame) {
        if self.meta.instance_id.as_deref() != Some(frame.instance_id.as_str()) {
            let closed_at = self.meta.last_sample_at.unwrap_or(frame.wall_ms);
            self.closed
                .extend(self.active.drain().map(|(_, c)| close(c, closed_at)));
            self.meta.global_counters = None;
            self.last_mono = None;
            self.meta.instance_id = Some(frame.instance_id.clone());
        }

        let elapsed = self
            .last_mono
            .and_then(|m| frame.mono.checked_sub(m))
            .filter(|d| !d.is_zero());

        let global = delta(frame.totals, self.meta.global_counters);
        self.meta.core_bytes = self.meta.core_bytes.saturating_add(global);
        self.meta.global_counters = Some(frame.totals);
        self.current_rate = elapsed.map(|e| rate(global, e));

        self.rates.clear();
        for sample in &frame.connections {
            let (increment, existed) = match self.active.get_mut(&sample.id) {
                Some(conn) => {
                    let increment = delta(sample.counters, Some(conn.counters));
                    conn.counters = sample.counters;
                    conn.bytes = conn.bytes.saturating_add(increment);
                    conn.dimensions = sample.dimensions.clone();
                    (increment, true)
                }
                None => {
                    self.active.insert(
                        sample.id.clone(),
                        ActiveConnection {
                            id: sample.id.clone(),
                            started_at: sample.started_at,
                            first_seen_at: frame.wall_ms,
                            counters: sample.counters,
                            bytes: sample.counters,
                            dimensions: sample.dimensions.clone(),
                        },
                    );
                    (sample.counters, false)
                }
            };
            if let Some(elapsed) = elapsed
                && existed
            {
                self.rates
                    .insert(sample.id.clone(), rate(increment, elapsed));
            }
            if increment != Bytes::default() {
                self.record(&sample.dimensions, increment);
            }
        }

        let seen: HashSet<&str> = frame.connections.iter().map(|s| s.id.as_str()).collect();
        self.closed.extend(
            self.active
                .extract_if(|id, _| !seen.contains(id.as_str()))
                .map(|(_, c)| close(c, frame.wall_ms)),
        );

        self.meta.last_sample_at = Some(frame.wall_ms);
        self.last_mono = Some(frame.mono);
    }

    /// The core connection is gone: rates are unknown, baselines are kept for the next frame.
    pub fn disconnect(&mut self) {
        self.rates.clear();
        self.current_rate = None;
        self.last_mono = None;
    }

    /// Starts a new session. Counter baselines survive, so connections that outlive the switch
    /// only contribute what they transfer afterwards.
    pub fn switch_profile(&mut self, profile: Option<String>, now_ms: i64) {
        self.meta.profile = profile;
        self.meta.started_at = now_ms;
        self.meta.last_sample_at = None;
        self.meta.core_bytes = Bytes::default();
        for conn in self.active.values_mut() {
            conn.bytes = Bytes::default();
        }
        self.pending_totals.clear();
        self.pending_topology.clear();
        self.closed.clear();
        self.reset_pending = true;
    }

    /// Whether the next batch wipes the store; until it lands, stored data belongs to another
    /// session and must not be read.
    pub fn reset_pending(&self) -> bool {
        self.reset_pending
    }

    /// Closed since the last batch, so not in the store yet.
    pub fn pending_closed(&self) -> &[ClosedConnection] {
        &self.closed
    }

    /// Re-arms the wipe after a batch that carried it could not be stored.
    pub fn require_reset(&mut self) {
        self.reset_pending = true;
    }

    pub fn take_batch(&mut self) -> FlushBatch {
        FlushBatch {
            reset: std::mem::take(&mut self.reset_pending),
            meta: self.meta.clone(),
            active: self.active.values().cloned().collect(),
            totals: std::mem::take(&mut self.pending_totals)
                .into_iter()
                .flat_map(|(g, keys)| keys.into_iter().map(move |(key, bytes)| (g, key, bytes)))
                .collect(),
            topology: std::mem::take(&mut self.pending_topology)
                .into_iter()
                .collect(),
            closed: std::mem::take(&mut self.closed),
        }
    }

    /// `stored_closed` is how many closed connections of this session the store holds.
    pub fn summary(&self, stored_closed: u64) -> TrafficSummary {
        TrafficSummary {
            profile: self.meta.profile.clone(),
            started_at: self.meta.started_at,
            last_sample_at: self.meta.last_sample_at,
            core_bytes: self.meta.core_bytes,
            active_connections: self.active.len() as u64,
            closed_connections: stored_closed + self.closed.len() as u64,
            current_rate: self.current_rate,
        }
    }

    pub fn pending_totals(&self, g: Dimension) -> impl Iterator<Item = (&str, Bytes)> {
        self.pending_totals
            .get(&g)
            .into_iter()
            .flatten()
            .map(|(key, bytes)| (key.as_str(), *bytes))
    }

    pub fn pending_total(&self, g: Dimension, key: &str) -> Option<Bytes> {
        self.pending_totals.get(&g)?.get(key).copied()
    }

    pub fn pending_topology(&self) -> impl Iterator<Item = (&TopologyKey, Bytes)> {
        self.pending_topology
            .iter()
            .map(|(key, bytes)| (key, *bytes))
    }

    /// Sum of the per-connection rates of each group; empty while no rate is known.
    pub fn current_rate_by(&self, g: Dimension) -> HashMap<String, Rate> {
        let mut out: HashMap<String, Rate> = HashMap::new();
        for conn in self.active.values() {
            if let Some(rate) = self.rates.get(&conn.id) {
                let slot = out.entry(group_key(&conn.dimensions, g)).or_default();
                slot.upload = slot.upload.saturating_add(rate.upload);
                slot.download = slot.download.saturating_add(rate.download);
            }
        }
        out
    }

    fn record(&mut self, dimensions: &Dimensions, increment: Bytes) {
        for g in STORED {
            let slot = self
                .pending_totals
                .entry(g)
                .or_default()
                .entry(group_key(dimensions, g))
                .or_default();
            *slot = slot.saturating_add(increment);
        }
        let slot = self
            .pending_topology
            .entry(TopologyKey::from_dimensions(dimensions))
            .or_default();
        *slot = slot.saturating_add(increment);
    }
}

/// Increase since `previous`, per component; a smaller value means the counter restarted and the
/// current value is what accumulated since.
fn delta(current: Bytes, previous: Option<Bytes>) -> Bytes {
    let component = |current: u64, previous: Option<u64>| match previous {
        Some(previous) if current >= previous => current - previous,
        _ => current,
    };
    Bytes {
        upload: component(current.upload, previous.map(|p| p.upload)),
        download: component(current.download, previous.map(|p| p.download)),
    }
}

/// `elapsed` is non-zero.
fn rate(delta: Bytes, elapsed: Duration) -> Rate {
    let per_second = |bytes: u64| {
        let rate = u128::from(bytes) * 1_000_000_000 / elapsed.as_nanos();
        u64::try_from(rate).unwrap_or(u64::MAX)
    };
    Rate {
        upload: per_second(delta.upload),
        download: per_second(delta.download),
    }
}

fn close(conn: ActiveConnection, closed_at: i64) -> ClosedConnection {
    ClosedConnection {
        id: conn.id,
        started_at: conn.started_at,
        first_seen_at: conn.first_seen_at,
        closed_at,
        bytes: conn.bytes,
        dimensions: conn.dimensions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = "core-1";

    fn bytes(upload: u64, download: u64) -> Bytes {
        Bytes { upload, download }
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
        }
    }

    fn sample(id: &str, upload: u64, download: u64) -> Sample {
        Sample {
            id: id.into(),
            started_at: 1_000,
            counters: bytes(upload, download),
            dimensions: dims("curl"),
        }
    }

    fn frame(
        instance: &str,
        wall_ms: i64,
        mono_ms: u64,
        totals: Bytes,
        connections: Vec<Sample>,
    ) -> Frame {
        Frame {
            instance_id: instance.into(),
            wall_ms,
            mono: Duration::from_millis(mono_ms),
            totals,
            connections,
        }
    }

    fn pending(session: &Session, g: Dimension, key: &str) -> Option<Bytes> {
        session.pending_total(g, key)
    }

    #[test]
    fn first_observation_counts_full_counters() {
        let mut session = Session::new(Some("p1".into()), 500);
        session.observe(&frame(
            INSTANCE,
            2_000,
            0,
            bytes(300, 900),
            vec![sample("a", 100, 400)],
        ));

        assert_eq!(session.meta().core_bytes, bytes(300, 900));
        assert_eq!(session.meta().global_counters, Some(bytes(300, 900)));
        assert_eq!(session.meta().instance_id.as_deref(), Some(INSTANCE));
        assert_eq!(session.meta().last_sample_at, Some(2_000));
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(100, 400))
        );
        assert_eq!(
            pending(&session, Dimension::Exit, "Node-A"),
            Some(bytes(100, 400))
        );
        assert_eq!(session.pending_topology().count(), 1);

        let batch = session.take_batch();
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].counters, bytes(100, 400));
        assert_eq!(batch.active[0].bytes, bytes(100, 400));
        assert_eq!(batch.active[0].first_seen_at, 2_000);
    }

    #[test]
    fn later_frames_count_deltas() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));
        session.take_batch();
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(150, 260),
            vec![sample("a", 130, 190)],
        ));

        assert_eq!(session.meta().core_bytes, bytes(150, 260));
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(30, 90))
        );
        let batch = session.take_batch();
        assert_eq!(batch.active[0].counters, bytes(130, 190));
        assert_eq!(batch.active[0].bytes, bytes(130, 190));
    }

    #[test]
    fn unchanged_counters_leave_no_pending_delta() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(10, 10),
            vec![sample("a", 10, 10)],
        ));
        session.take_batch();
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(10, 10),
            vec![sample("a", 10, 10)],
        ));

        let batch = session.take_batch();
        assert!(batch.totals.is_empty());
        assert!(batch.topology.is_empty());
    }

    #[test]
    fn counter_reset_counts_the_current_value() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(1_000, 1_000),
            vec![sample("a", 500, 500)],
        ));
        session.take_batch();
        // Global upload restarted; connection download restarted, upload kept growing.
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(40, 1_200),
            vec![sample("a", 600, 20)],
        ));

        assert_eq!(session.meta().core_bytes, bytes(1_040, 1_200));
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(100, 20))
        );
    }

    #[test]
    fn missing_connection_is_closed_with_its_session_bytes() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(30, 30),
            vec![sample("a", 10, 10), sample("b", 20, 20)],
        ));
        session.take_batch();
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(35, 35),
            vec![sample("b", 25, 25)],
        ));

        let batch = session.take_batch();
        assert_eq!(batch.closed.len(), 1);
        let closed = &batch.closed[0];
        assert_eq!(closed.id, "a");
        assert_eq!(closed.closed_at, 2_000);
        assert_eq!(closed.bytes, bytes(10, 10));
        assert_eq!(closed.first_seen_at, 1_000);
        assert_eq!(batch.active.len(), 1);
        assert_eq!(session.summary(0).active_connections, 1);
    }

    #[test]
    fn summary_counts_stored_and_unflushed_closed_connections() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(30, 30),
            vec![sample("a", 10, 10), sample("b", 20, 20)],
        ));
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(35, 35),
            vec![sample("b", 25, 25)],
        ));

        assert_eq!(session.summary(5).closed_connections, 6);
        session.take_batch();
        assert_eq!(session.summary(6).closed_connections, 6);
    }

    #[test]
    fn instance_change_closes_everything_and_restarts_baselines() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));
        session.take_batch();

        // The new core reuses the connection id with small counters.
        session.observe(&frame(
            "core-2",
            5_000,
            9_000,
            bytes(7, 7),
            vec![sample("a", 5, 5)],
        ));

        assert_eq!(session.meta().instance_id.as_deref(), Some("core-2"));
        assert_eq!(session.meta().core_bytes, bytes(107, 107));
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(5, 5))
        );
        assert_eq!(session.summary(0).current_rate, None);

        let batch = session.take_batch();
        assert_eq!(batch.closed.len(), 1);
        assert_eq!(batch.closed[0].closed_at, 1_000);
        assert_eq!(batch.closed[0].bytes, bytes(100, 100));
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].bytes, bytes(5, 5));
    }

    #[test]
    fn switch_profile_keeps_baselines_and_zeroes_the_session() {
        let mut session = Session::new(Some("p1".into()), 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));

        session.switch_profile(Some("p2".into()), 1_500);
        assert_eq!(session.meta().profile.as_deref(), Some("p2"));
        assert_eq!(session.meta().started_at, 1_500);
        assert_eq!(session.meta().last_sample_at, None);
        assert_eq!(session.meta().core_bytes, Bytes::default());
        assert_eq!(session.meta().instance_id.as_deref(), Some(INSTANCE));
        assert_eq!(session.meta().global_counters, Some(bytes(100, 100)));
        assert_eq!(session.pending_totals(Dimension::Process).count(), 0);
        assert_eq!(session.pending_topology().count(), 0);
        let switch = session.take_batch();
        assert!(switch.reset);
        assert!(switch.closed.is_empty());
        assert_eq!(switch.active.len(), 1);
        assert_eq!(switch.active[0].counters, bytes(100, 100));
        assert_eq!(switch.active[0].bytes, Bytes::default());

        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(130, 170),
            vec![sample("a", 120, 150)],
        ));

        assert_eq!(session.meta().core_bytes, bytes(30, 70));
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(20, 50))
        );
        let batch = session.take_batch();
        assert_eq!(batch.active[0].bytes, bytes(20, 50));
        assert_eq!(batch.active[0].counters, bytes(120, 150));
    }

    #[test]
    fn rates_need_elapsed_time_and_an_existing_connection() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));
        assert_eq!(session.summary(0).current_rate, None);
        assert!(session.current_rate_by(Dimension::Process).is_empty());

        session.observe(&frame(
            INSTANCE,
            3_000,
            2_000,
            bytes(300, 500),
            vec![sample("a", 200, 300), sample("b", 50, 50)],
        ));
        assert_eq!(
            session.summary(0).current_rate,
            Some(Rate {
                upload: 100,
                download: 200
            })
        );
        // "b" is new in this frame, so only "a" contributes.
        assert_eq!(
            session.current_rate_by(Dimension::Process).get("curl"),
            Some(&Rate {
                upload: 50,
                download: 100
            })
        );

        // No monotonic progress: rates are unknown again, not stale.
        session.observe(&frame(
            INSTANCE,
            4_000,
            2_000,
            bytes(300, 500),
            vec![sample("a", 200, 300)],
        ));
        assert_eq!(session.summary(0).current_rate, None);
        assert!(session.current_rate_by(Dimension::Process).is_empty());
    }

    #[test]
    fn current_rate_by_sums_connections_per_group() {
        let mut session = Session::new(None, 0);
        let other = |id: &str, up, down| Sample {
            dimensions: dims("firefox"),
            ..sample(id, up, down)
        };
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(0, 0),
            vec![sample("a", 0, 0), sample("b", 0, 0), other("c", 0, 0)],
        ));
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(60, 0),
            vec![sample("a", 10, 0), sample("b", 20, 0), other("c", 30, 0)],
        ));

        let by_process = session.current_rate_by(Dimension::Process);
        assert_eq!(by_process["curl"].upload, 30);
        assert_eq!(by_process["firefox"].upload, 30);
        assert_eq!(
            session.current_rate_by(Dimension::Exit)["Node-A"].upload,
            60
        );
    }

    #[test]
    fn rates_are_whole_bytes_per_second_rounded_down() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(0, 0),
            vec![sample("a", 0, 0)],
        ));
        // 10 bytes over 3s is 3.33 B/s; 2 bytes over 3s is 0.67 B/s.
        session.observe(&frame(
            INSTANCE,
            4_000,
            3_000,
            bytes(10, 2),
            vec![sample("a", 10, 2)],
        ));

        let expected = Rate {
            upload: 3,
            download: 0,
        };
        assert_eq!(session.summary(0).current_rate, Some(expected));
        assert_eq!(
            session.current_rate_by(Dimension::Process).get("curl"),
            Some(&expected)
        );
    }

    #[test]
    fn rates_saturate() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(0, 0),
            vec![sample("a", 0, 0), sample("b", 0, 0)],
        ));
        // u64::MAX bytes within a millisecond overflows a u64 rate; two such connections
        // overflow their sum.
        session.observe(&frame(
            INSTANCE,
            1_001,
            1,
            bytes(u64::MAX, 0),
            vec![sample("a", u64::MAX, 0), sample("b", u64::MAX, 0)],
        ));

        assert_eq!(session.summary(0).current_rate.unwrap().upload, u64::MAX);
        assert_eq!(
            session.current_rate_by(Dimension::Process)["curl"],
            Rate {
                upload: u64::MAX,
                download: 0
            }
        );
    }

    #[test]
    fn disconnect_clears_rates_but_keeps_baselines() {
        let mut session = Session::new(None, 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));
        session.observe(&frame(
            INSTANCE,
            2_000,
            1_000,
            bytes(150, 150),
            vec![sample("a", 150, 150)],
        ));
        assert!(session.summary(0).current_rate.is_some());

        session.disconnect();
        assert_eq!(session.summary(0).current_rate, None);
        assert!(session.current_rate_by(Dimension::Process).is_empty());
        assert_eq!(session.summary(0).active_connections, 1);

        session.take_batch();
        session.observe(&frame(
            INSTANCE,
            9_000,
            8_000,
            bytes(160, 160),
            vec![sample("a", 160, 160)],
        ));
        // Baselines survived: only the delta is counted, and no rate is known yet.
        assert_eq!(
            pending(&session, Dimension::Process, "curl"),
            Some(bytes(10, 10))
        );
        assert_eq!(session.summary(0).current_rate, None);
    }

    #[test]
    fn restore_counts_only_the_delta_since_the_last_flush() {
        let mut session = Session::new(Some("p1".into()), 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(100, 100),
            vec![sample("a", 100, 100)],
        ));
        let batch = session.take_batch();

        let mut restored = Session::restore(batch.meta, batch.active);
        restored.observe(&frame(
            INSTANCE,
            7_000,
            0,
            bytes(120, 140),
            vec![sample("a", 120, 140)],
        ));

        assert_eq!(restored.meta().core_bytes, bytes(120, 140));
        assert_eq!(
            pending(&restored, Dimension::Process, "curl"),
            Some(bytes(20, 40))
        );
        assert_eq!(restored.summary(0).current_rate, None);
        assert!(restored.take_batch().closed.is_empty());
    }

    #[test]
    fn the_reset_flag_follows_the_session_lifecycle() {
        let mut session = Session::new(Some("p1".into()), 0);
        assert!(session.reset_pending());
        assert!(session.take_batch().reset);
        assert!(!session.reset_pending());
        assert!(!session.take_batch().reset);

        session.switch_profile(Some("p2".into()), 1);
        assert!(session.reset_pending());
        assert!(session.take_batch().reset);
        assert!(!session.take_batch().reset);

        // A reset batch that could not be stored has to go out again.
        session.switch_profile(None, 2);
        assert!(session.take_batch().reset);
        session.require_reset();
        assert!(session.reset_pending());
        assert!(session.take_batch().reset);

        let mut restored = Session::restore(session.meta().clone(), Vec::new());
        assert!(!restored.reset_pending());
        assert!(!restored.take_batch().reset);
    }

    #[test]
    fn take_batch_drains_pending_state() {
        let mut session = Session::new(Some("p1".into()), 0);
        session.observe(&frame(
            INSTANCE,
            1_000,
            0,
            bytes(10, 10),
            vec![sample("a", 10, 10)],
        ));

        let batch = session.take_batch();
        assert_eq!(batch.meta.profile.as_deref(), Some("p1"));
        assert_eq!(batch.totals.len(), STORED.len());
        assert_eq!(batch.topology.len(), 1);

        let again = session.take_batch();
        assert!(again.totals.is_empty());
        assert!(again.topology.is_empty());
        assert!(again.closed.is_empty());
        assert_eq!(again.active.len(), 1);
    }
}
