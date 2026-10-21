use crate::{
    bucket::{hour_of, minute_cutoff, minute_of},
    model::*,
    query::{Row, Usage},
};
use nyanpasu_tsdb::EventSeq;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
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
    pub meta: SessionMeta,
    /// The active connections that changed since the previous batch.
    pub active: Vec<ActiveConnection>,
    /// Connections closed since the previous batch. Applied before `active`, because a new
    /// connection may have reused a closed one's id.
    pub removed: Vec<String>,
    /// Usage increments of connections closed since the previous batch, by minute bucket.
    pub minutes: Vec<(u32, Arc<Dimensions>, Usage)>,
    /// The same by hour bucket.
    pub hours: Vec<(u32, Arc<Dimensions>, Usage)>,
    pub closed: Vec<PendingClosed>,
    /// Per-connection contributions, in `closed` order, for bounded atomic close groups.
    pub closed_buckets: Vec<ClosedBuckets>,
}

#[derive(Clone, Debug)]
pub struct ClosedBuckets {
    pub minutes: BTreeMap<u32, Bytes>,
    pub hours: BTreeMap<u32, Bytes>,
}

impl FlushBatch {
    /// Keeps only data whose transactions have not committed.
    pub fn uncommitted(mut self, closed: usize, active: usize) -> Self {
        self.active.drain(..active);
        if closed == self.closed.len() {
            self.removed.clear();
            self.minutes.clear();
            self.hours.clear();
            self.closed.clear();
            self.closed_buckets.clear();
        } else if closed > 0 {
            let mut remaining = self.closed_part(closed..self.closed.len());
            remaining.active = self.active;
            return remaining;
        }
        self
    }

    pub(crate) fn closed_part(&self, range: std::ops::Range<usize>) -> Self {
        let mut minutes = Pending::new();
        let mut hours = Pending::new();
        for (conn, buckets) in self.closed[range.clone()]
            .iter()
            .zip(&self.closed_buckets[range.clone()])
        {
            closed_usage(conn, buckets, &mut minutes, &mut hours);
        }
        Self {
            meta: self.meta.clone(),
            active: vec![],
            removed: self.closed[range.clone()]
                .iter()
                .map(|c| c.id.clone())
                .collect(),
            minutes: drain_usage(&mut minutes),
            hours: drain_usage(&mut hours),
            closed: self.closed[range.clone()].to_vec(),
            closed_buckets: self.closed_buckets[range].to_vec(),
        }
    }
}

/// Usage of closed connections that is not stored yet, by bucket and final dimensions.
type Pending = HashMap<(u32, Arc<Dimensions>), Usage>;

#[derive(Default)]
pub struct Session {
    meta: SessionMeta,
    active: HashMap<String, ActiveConnection>,
    /// Active connections that differ from the stored ones.
    dirty: HashSet<String>,
    /// Rates from the latest frame; only connections seen in two consecutive frames have one.
    rates: HashMap<String, Rate>,
    current_rate: Option<Rate>,
    last_mono: Option<Duration>,
    minutes: Pending,
    hours: Pending,
    closed: Vec<PendingClosed>,
    closed_buckets: Vec<ClosedBuckets>,
    /// Conservative Input::bytes estimate for the indivisible part, excluding metadata.
    pending_bytes: u64,
    removed: Vec<String>,
    /// The store refused the last batch and it is held in the buffers above: frames are ignored
    /// until the next batch is taken, so that the buffers stay as they are.
    refused: bool,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rates stay unknown until two frames have been observed.
    pub fn restore(meta: SessionMeta, active: Vec<ActiveConnection>) -> Self {
        Self {
            meta,
            active: active.into_iter().map(|c| (c.id.clone(), c)).collect(),
            ..Self::default()
        }
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    /// `profile` is the current profile; connections first seen in this frame keep it for good.
    /// While the store refuses writes the frame is ignored: the counters are cumulative, so the
    /// first frame after the outage carries what the ignored ones would have added.
    /// Takes ownership so new or changed observations can move into the session.
    pub fn observe(
        &mut self,
        frame: Frame,
        profile: Option<&str>,
        sequence: &mut impl FnMut() -> EventSeq,
    ) {
        if self.refused {
            return;
        }
        if self.meta.instance_id.as_deref() != Some(frame.instance_id.as_str()) {
            let closed_at = self.meta.last_sample_at.unwrap_or(frame.wall_ms);
            let gone: Vec<_> = self.active.drain().map(|(_, c)| c).collect();
            for conn in gone {
                self.close(conn, closed_at, sequence());
            }
            self.meta.global_counters = None;
            self.last_mono = None;
            self.meta.instance_id = Some(frame.instance_id);
        }

        let elapsed = self
            .last_mono
            .and_then(|m| frame.mono.checked_sub(m))
            .filter(|d| !d.is_zero());

        let global = delta(frame.totals, self.meta.global_counters);
        self.meta.global_counters = Some(frame.totals);
        self.current_rate = elapsed.map(|e| rate(global, e));

        let (minute, hour) = (minute_of(frame.wall_ms), hour_of(frame.wall_ms));
        self.rates.clear();
        let mut seen = HashSet::with_capacity(frame.connections.len());
        for mut sample in frame.connections {
            let (increment, existed) = match self.active.get_mut(&sample.id) {
                Some(conn) => {
                    let increment = delta(sample.counters, Some(conn.counters));
                    // Compare observations without relabelling the first-seen profile.
                    let profile = conn.dimensions.profile.take();
                    sample.dimensions.profile = None;
                    let dimensions_changed = conn.dimensions != sample.dimensions;
                    if conn.counters != sample.counters || dimensions_changed {
                        self.dirty.insert(sample.id.clone());
                    }
                    conn.counters = sample.counters;
                    if dimensions_changed {
                        sample.dimensions.profile = profile;
                        conn.dimensions = sample.dimensions;
                    } else {
                        conn.dimensions.profile = profile;
                    }
                    record(conn, minute, hour, increment);
                    (increment, true)
                }
                None => {
                    let mut conn = ActiveConnection {
                        id: sample.id.clone(),
                        started_at: sample.started_at,
                        first_seen_at: frame.wall_ms,
                        counters: sample.counters,
                        dimensions: sample.dimensions,
                        minutes: BTreeMap::new(),
                        hours: BTreeMap::new(),
                    };
                    conn.dimensions.profile = profile.map(str::to_owned);
                    record(&mut conn, minute, hour, sample.counters);
                    self.active.insert(sample.id.clone(), conn);
                    self.dirty.insert(sample.id.clone());
                    (sample.counters, false)
                }
            };
            if let Some(elapsed) = elapsed
                && existed
            {
                self.rates
                    .insert(sample.id.clone(), rate(increment, elapsed));
            }
            seen.insert(sample.id);
        }

        let gone: Vec<_> = self
            .active
            .extract_if(|id, _| !seen.contains(id.as_str()))
            .map(|(_, c)| c)
            .collect();
        for conn in gone {
            self.close(conn, frame.wall_ms, sequence());
        }

        self.meta.last_sample_at = Some(frame.wall_ms);
        self.last_mono = Some(frame.mono);
    }

    /// The core connection is gone: rates are unknown, baselines are kept for the next frame.
    pub fn disconnect(&mut self) {
        self.rates.clear();
        self.current_rate = None;
        self.last_mono = None;
    }

    /// Closed since the last batch, so not in the store yet.
    pub fn pending_closed(&self) -> &[PendingClosed] {
        &self.closed
    }

    /// Metadata has a fixed numeric allowance plus the instance id. Active aux records can
    /// follow in separate transactions, so they do not trigger early flushes.
    pub fn pending_bytes(&self) -> u64 {
        self.pending_bytes
            .saturating_add(128 + self.meta.instance_id.as_ref().map_or(0, |s| s.len() as u64))
    }

    /// Drains what the store has not seen yet, and resumes observing: the batch is the retry of
    /// a refused one, if there was one. Live connections drop the minute buckets that no
    /// minute-tier range reaches at `now_ms` any more; the hour buckets hold that traffic.
    pub fn take_batch(&mut self, now_ms: i64) -> FlushBatch {
        self.refused = false;
        self.pending_bytes = 0;
        let cutoff = minute_cutoff(now_ms);
        for conn in self.active.values_mut() {
            let kept = conn.minutes.split_off(&cutoff);
            if !conn.minutes.is_empty() {
                self.dirty.insert(conn.id.clone());
            }
            conn.minutes = kept;
        }

        FlushBatch {
            meta: self.meta.clone(),
            active: self
                .dirty
                .drain()
                .filter_map(|id| self.active.get(&id).cloned())
                .collect(),
            removed: std::mem::take(&mut self.removed),
            minutes: drain_usage(&mut self.minutes),
            hours: drain_usage(&mut self.hours),
            closed: std::mem::take(&mut self.closed),
            closed_buckets: std::mem::take(&mut self.closed_buckets),
        }
    }

    /// Takes back a batch the store did not accept, so that the next one carries it again: what
    /// it held merges with what accumulated since; the next flush uses the current retention policy.
    /// Frames are ignored until then, which keeps the buffers bounded by the live connections
    /// while the store keeps refusing; the rates are unknown meanwhile, as after `disconnect`.
    pub fn requeue(&mut self, batch: FlushBatch) {
        self.refused = true;
        self.disconnect();
        self.pending_bytes = self
            .pending_bytes
            .saturating_add(closed_estimate(&batch.closed, &batch.closed_buckets));
        for (rows, pending) in [
            (batch.minutes, &mut self.minutes),
            (batch.hours, &mut self.hours),
        ] {
            for (bucket, dimensions, usage) in rows {
                add_usage(pending, bucket, &dimensions, usage);
            }
        }
        self.closed.splice(0..0, batch.closed);
        self.closed_buckets.splice(0..0, batch.closed_buckets);
        // Still first: a connection may have reused one of these ids since.
        self.removed.splice(0..0, batch.removed);
        // A connection that is gone by now was removed above; the others go out as they are now.
        self.dirty.extend(
            batch
                .active
                .into_iter()
                .map(|conn| conn.id)
                .filter(|id| self.active.contains_key(id)),
        );
    }

    /// The ids of the live connections that satisfy every filter, sorted.
    pub fn active_ids(&self, filters: &[TrafficFilter]) -> Vec<String> {
        let mut ids: Vec<String> = self
            .active
            .values()
            .filter(|conn| crate::key::matches(&conn.dimensions, filters))
            .map(|conn| conn.id.clone())
            .collect();
        ids.sort_unstable();
        ids
    }

    /// `stored_closed` is how many closed connections the store holds.
    pub fn summary(&self, stored_closed: u64) -> TrafficSummary {
        TrafficSummary {
            last_sample_at: self.meta.last_sample_at,
            active_connections: self.active.len() as u64,
            closed_connections: stored_closed + self.closed.len() as u64,
            current_rate: self.current_rate,
        }
    }

    /// One row per live connection: its traffic in the buckets of `tier` from `from` on, one
    /// connection, and its current rate.
    pub fn active_rows(
        &self,
        tier: Tier,
        from: Option<u32>,
    ) -> impl Iterator<Item = Row<'_>> + Clone {
        self.active.values().map(move |conn| {
            let buckets = match tier {
                Tier::Minute => &conn.minutes,
                Tier::Hour => &conn.hours,
            };
            let bytes = buckets
                .range(from.unwrap_or(0)..)
                .fold(Bytes::default(), |sum, (_, bytes)| {
                    sum.saturating_add(*bytes)
                });
            (
                &conn.dimensions,
                Usage {
                    bytes,
                    connections: 1,
                },
                self.rates.get(&conn.id).copied(),
            )
        })
    }

    /// Upper bound before range/filter selection, so query maps reserve once without collecting
    /// rows. Optional/flattened iterators otherwise lose the known lengths of these buffers.
    pub(crate) fn row_capacity(&self, tier: Tier, scope: TrafficScope) -> usize {
        let pending = match tier {
            Tier::Minute => self.minutes.len(),
            Tier::Hour => self.hours.len(),
        };
        match scope {
            TrafficScope::All => pending + self.active.len(),
            TrafficScope::Active => self.active.len(),
            TrafficScope::Closed => pending,
        }
    }

    /// The usage of closed connections that is not stored yet, from bucket `from` on.
    pub fn pending_rows(
        &self,
        tier: Tier,
        from: Option<u32>,
    ) -> impl Iterator<Item = Row<'_>> + Clone {
        let pending = match tier {
            Tier::Minute => &self.minutes,
            Tier::Hour => &self.hours,
        };
        pending
            .iter()
            .filter(move |((bucket, _), _)| from.is_none_or(|from| *bucket >= from))
            .map(|((_, dimensions), usage)| (&**dimensions, *usage, None))
    }

    /// Moves the connection's buckets under its final dimensions into the pending usage and counts
    /// it in the buckets of the close time.
    fn close(&mut self, conn: ActiveConnection, closed_at: i64, seq: EventSeq) {
        self.dirty.remove(&conn.id);
        self.removed.push(conn.id.clone());

        let bytes = conn.bytes();
        let dimensions = Arc::new(conn.dimensions);
        let buckets = ClosedBuckets {
            minutes: conn.minutes,
            hours: conn.hours,
        };
        let closed = PendingClosed {
            id: conn.id,
            started_at: conn.started_at,
            first_seen_at: conn.first_seen_at,
            closed_at,
            bytes,
            dimensions,
            seq,
        };
        closed_usage(&closed, &buckets, &mut self.minutes, &mut self.hours);
        self.pending_bytes = self.pending_bytes.saturating_add(closed_estimate(
            std::slice::from_ref(&closed),
            std::slice::from_ref(&buckets),
        ));
        self.closed.push(closed);
        self.closed_buckets.push(buckets);
    }
}

fn closed_usage(
    conn: &PendingClosed,
    buckets: &ClosedBuckets,
    minutes: &mut Pending,
    hours: &mut Pending,
) {
    for (buckets, pending, close_bucket) in [
        (&buckets.minutes, minutes, minute_of(conn.closed_at)),
        (&buckets.hours, hours, hour_of(conn.closed_at)),
    ] {
        for (&bucket, &bytes) in buckets {
            add_usage(
                pending,
                bucket,
                &conn.dimensions,
                Usage {
                    bytes,
                    connections: 0,
                },
            );
        }
        add_usage(
            pending,
            close_bucket,
            &conn.dimensions,
            Usage {
                bytes: Bytes::default(),
                connections: 1,
            },
        );
    }
}

fn closed_estimate(closed: &[PendingClosed], buckets: &[ClosedBuckets]) -> u64 {
    closed
        .iter()
        .zip(buckets)
        .map(|(c, b)| {
            let d = &c.dimensions;
            // Each text can appear in several labels (origin, displays and canonical keys).
            // Ten bytes per string/sequence bound postcard's length prefixes; numeric fields
            // follow Input::bytes (32 per series point, 56 per event, plus id and aux key).
            let text = d.process.len()
                + d.source.len()
                + d.inbound.len()
                + d.target.len()
                + d.protocol.len()
                + d.rule.kind.len()
                + d.rule.payload.len()
                + d.profile.as_ref().map_or(0, String::len)
                + d.source_region.len()
                + d.destination_region.len()
                + d.chains.iter().map(String::len).sum::<usize>();
            let labels = 4 * text as u64 + 128 + 16 * d.chains.len() as u64;
            (b.minutes.len() as u64 + b.hours.len() as u64 + 2) * (32 + labels)
                + 56
                + labels
                + 2 * c.id.len() as u64
                + 1
        })
        .sum()
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

/// Counts `increment` in the given buckets; zero leaves no bucket behind.
fn record(conn: &mut ActiveConnection, minute: u32, hour: u32, increment: Bytes) {
    if increment == Bytes::default() {
        return;
    }
    for (buckets, bucket) in [(&mut conn.minutes, minute), (&mut conn.hours, hour)] {
        let slot = buckets.entry(bucket).or_default();
        *slot = slot.saturating_add(increment);
    }
}

fn add_usage(pending: &mut Pending, bucket: u32, dimensions: &Arc<Dimensions>, usage: Usage) {
    let slot = pending.entry((bucket, Arc::clone(dimensions))).or_default();
    *slot = slot.saturating_add(usage);
}

fn drain_usage(pending: &mut Pending) -> Vec<(u32, Arc<Dimensions>, Usage)> {
    pending
        .drain()
        .map(|((bucket, dimensions), usage)| (bucket, dimensions, usage))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{ReportRequest, TrafficReport, report};

    fn sequence_source() -> impl FnMut() -> EventSeq {
        let mut next = 0;
        move || {
            let seq = EventSeq {
                incarnation: 1,
                seq: next,
            };
            next += 1;
            seq
        }
    }

    const INSTANCE: &str = "core-1";
    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;

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
            destination_basis: None,
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

    fn take(session: &mut Session, now_ms: i64) -> FlushBatch {
        session.take_batch(now_ms)
    }

    fn usage(upload: u64, download: u64, connections: u64) -> Usage {
        Usage {
            bytes: bytes(upload, download),
            connections,
        }
    }

    fn buckets<const N: usize>(buckets: [(u32, Bytes); N]) -> BTreeMap<u32, Bytes> {
        buckets.into_iter().collect()
    }

    /// The rows as (bucket, process, usage), ordered.
    fn usage_rows(rows: &[(u32, Arc<Dimensions>, Usage)]) -> Vec<(u32, String, Usage)> {
        let mut rows: Vec<_> = rows
            .iter()
            .map(|(bucket, d, usage)| (*bucket, d.process.clone(), *usage))
            .collect();
        rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        rows
    }

    /// The live connections' rates summed per group.
    fn rates_by(session: &Session, dimension: Dimension) -> HashMap<String, Rate> {
        let request = ReportRequest {
            query: TrafficQuery {
                range: TrafficRange::All,
                scope: TrafficScope::Active,
                filters: Vec::new(),
            },
            metric: Metric::Bytes,
            rankings: vec![dimension],
            ranking_limit: 200,
            topology: None,
        };
        report(session.active_rows(Tier::Hour, None), &request).rankings[0]
            .groups
            .iter()
            .filter_map(|g| g.current_rate.map(|rate| (g.key.clone(), rate)))
            .collect()
    }

    #[test]
    fn first_observation_counts_full_counters() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                2_000,
                0,
                bytes(300, 900),
                vec![sample("a", 100, 400)],
            ),
            None,
            &mut sequence,
        );

        assert_eq!(session.meta().global_counters, Some(bytes(300, 900)));
        assert_eq!(session.meta().instance_id.as_deref(), Some(INSTANCE));
        assert_eq!(session.meta().last_sample_at, Some(2_000));

        let batch = take(&mut session, 2_000);
        assert_eq!(batch.active.len(), 1);
        let conn = &batch.active[0];
        assert_eq!(conn.counters, bytes(100, 400));
        assert_eq!(conn.minutes, buckets([(0, bytes(100, 400))]));
        assert_eq!(conn.hours, buckets([(0, bytes(100, 400))]));
        assert_eq!(conn.bytes(), bytes(100, 400));
        assert_eq!(conn.first_seen_at, 2_000);
    }

    #[test]
    fn later_frames_count_deltas() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(100, 100),
                vec![sample("a", 100, 100)],
            ),
            None,
            &mut sequence,
        );
        take(&mut session, 1_000);
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(150, 260),
                vec![sample("a", 130, 190)],
            ),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, 2_000);
        assert_eq!(batch.active[0].counters, bytes(130, 190));
        assert_eq!(batch.active[0].bytes(), bytes(130, 190));
        assert_eq!(batch.active[0].hours, buckets([(0, bytes(130, 190))]));
    }

    #[test]
    fn unchanged_connections_are_not_upserted_again() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(10, 10), vec![sample("a", 10, 10)]),
            None,
            &mut sequence,
        );
        assert_eq!(take(&mut session, 1_000).active.len(), 1);
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(10, 10),
                vec![sample("a", 10, 10)],
            ),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, 2_000);
        assert!(batch.active.is_empty());
        assert!(batch.removed.is_empty());
        assert!(batch.minutes.is_empty());
        assert!(batch.hours.is_empty());
        assert!(batch.closed.is_empty());

        // A dimension change alone is a change.
        let moved = Sample {
            dimensions: dims("firefox"),
            ..sample("a", 10, 10)
        };
        session.observe(
            frame(INSTANCE, 3_000, 2_000, bytes(10, 10), vec![moved]),
            None,
            &mut sequence,
        );
        let batch = take(&mut session, 3_000);
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].dimensions.process, "firefox");
    }

    #[test]
    fn counter_reset_counts_the_current_value() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(1_000, 1_000),
                vec![sample("a", 500, 500)],
            ),
            None,
            &mut sequence,
        );
        take(&mut session, 1_000);
        // Connection download restarted, upload kept growing.
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(40, 1_200),
                vec![sample("a", 600, 20)],
            ),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, 2_000);
        assert_eq!(batch.active[0].bytes(), bytes(600, 520));
        assert_eq!(batch.active[0].counters, bytes(600, 20));
    }

    #[test]
    fn missing_connection_is_closed_with_its_bytes() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(30, 30),
                vec![sample("a", 10, 10), sample("b", 20, 20)],
            ),
            None,
            &mut sequence,
        );
        take(&mut session, 1_000);
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(35, 35),
                vec![sample("b", 25, 25)],
            ),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, 2_000);
        assert_eq!(batch.closed.len(), 1);
        let closed = &batch.closed[0];
        assert_eq!(closed.id, "a");
        assert_eq!(closed.closed_at, 2_000);
        assert_eq!(closed.bytes, bytes(10, 10));
        assert_eq!(closed.first_seen_at, 1_000);
        assert_eq!(batch.removed, ["a"]);
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].id, "b");
        assert_eq!(session.summary(0).active_connections, 1);
    }

    #[test]
    fn active_ids_are_the_matching_live_connections_sorted() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        let wget = Sample {
            dimensions: dims("wget"),
            ..sample("b", 1, 1)
        };
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(3, 3),
                vec![sample("c", 1, 1), wget, sample("a", 1, 1)],
            ),
            None,
            &mut sequence,
        );

        let curl = [TrafficFilter {
            dimension: Dimension::Process,
            value: "curl".into(),
        }];
        assert_eq!(session.active_ids(&curl), ["a", "c"]);
        assert_eq!(session.active_ids(&[]), ["a", "b", "c"]);
    }

    #[test]
    fn summary_counts_stored_and_unflushed_closed_connections() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(30, 30),
                vec![sample("a", 10, 10), sample("b", 20, 20)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(35, 35),
                vec![sample("b", 25, 25)],
            ),
            None,
            &mut sequence,
        );

        let summary = session.summary(5);
        assert_eq!(summary.closed_connections, 6);
        assert_eq!(summary.active_connections, 1);
        assert_eq!(summary.last_sample_at, Some(2_000));
        take(&mut session, 2_000);
        assert_eq!(session.summary(6).closed_connections, 6);
    }

    #[test]
    fn instance_change_closes_everything_and_restarts_baselines() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(100, 100),
                vec![sample("a", 100, 100)],
            ),
            None,
            &mut sequence,
        );
        take(&mut session, 1_000);

        // The new core reuses the connection id with small counters.
        session.observe(
            frame("core-2", 5_000, 9_000, bytes(7, 7), vec![sample("a", 5, 5)]),
            None,
            &mut sequence,
        );

        assert_eq!(session.meta().instance_id.as_deref(), Some("core-2"));
        assert_eq!(session.summary(0).current_rate, None);

        let batch = take(&mut session, 5_000);
        assert_eq!(batch.closed.len(), 1);
        assert_eq!(batch.closed[0].closed_at, 1_000);
        assert_eq!(batch.closed[0].bytes, bytes(100, 100));
        // The old connection is removed and the new one with its id is upserted after it.
        assert_eq!(batch.removed, ["a"]);
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].bytes(), bytes(5, 5));
        // Its buckets reached the usage, with the close counted at the last sample.
        assert_eq!(
            usage_rows(&batch.hours),
            [(0, "curl".to_owned(), usage(100, 100, 1))]
        );
    }

    #[test]
    fn closing_writes_the_buckets_under_the_final_dimensions() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        let moving = |process: &str, upload| Sample {
            dimensions: dims(process),
            ..sample("a", upload, 0)
        };
        // Hour 0, then hour 1 under other dimensions, closed during hour 2.
        session.observe(
            frame(
                INSTANCE,
                10 * MINUTE,
                0,
                bytes(0, 0),
                vec![moving("curl", 10)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                HOUR + 5 * MINUTE,
                1,
                bytes(0, 0),
                vec![moving("firefox", 25)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(INSTANCE, 2 * HOUR + MINUTE, 2, bytes(0, 0), vec![]),
            None,
            &mut sequence,
        );

        let closed = session.pending_closed();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].bytes, bytes(25, 0));
        assert_eq!(closed[0].dimensions.process, "firefox");

        let batch = take(&mut session, 2 * HOUR + MINUTE);
        assert!(batch.active.is_empty());
        assert_eq!(batch.removed, ["a"]);
        // Every bucket of the connection sits under the final dimensions, and the close is
        // counted once in the bucket it happened in.
        assert_eq!(
            usage_rows(&batch.hours),
            [
                (0, "firefox".to_owned(), usage(10, 0, 0)),
                (1, "firefox".to_owned(), usage(15, 0, 0)),
                (2, "firefox".to_owned(), usage(0, 0, 1)),
            ]
        );
        assert_eq!(
            usage_rows(&batch.minutes),
            [
                (10, "firefox".to_owned(), usage(10, 0, 0)),
                (65, "firefox".to_owned(), usage(15, 0, 0)),
                (121, "firefox".to_owned(), usage(0, 0, 1)),
            ]
        );
        let total = batch
            .hours
            .iter()
            .fold(Usage::default(), |sum, (_, _, u)| sum.saturating_add(*u));
        assert_eq!(total, usage(25, 0, 1));
        assert_eq!(total.bytes, batch.closed[0].bytes);
    }

    #[test]
    fn pending_rows_follow_the_tier_and_the_start() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                10 * MINUTE,
                0,
                bytes(0, 0),
                vec![sample("a", 10, 0)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                HOUR + 5 * MINUTE,
                1,
                bytes(0, 0),
                vec![sample("a", 40, 0)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(INSTANCE, HOUR + 6 * MINUTE, 2, bytes(0, 0), vec![]),
            None,
            &mut sequence,
        );

        let sum = |tier, from| {
            session
                .pending_rows(tier, from)
                .fold(Usage::default(), |sum, (_, u, rate)| {
                    assert_eq!(rate, None);
                    sum.saturating_add(u)
                })
        };
        assert_eq!(sum(Tier::Hour, None), usage(40, 0, 1));
        assert_eq!(sum(Tier::Hour, Some(1)), usage(30, 0, 1));
        assert_eq!(sum(Tier::Hour, Some(2)), usage(0, 0, 0));
        assert_eq!(sum(Tier::Minute, Some(11)), usage(30, 0, 1));
        // The close sits in minute 66, after the last bucket with traffic.
        assert_eq!(sum(Tier::Minute, Some(66)), usage(0, 0, 1));
    }

    #[test]
    fn a_connection_across_the_hour_splits_between_two_buckets() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                HOUR - 1_000,
                0,
                bytes(0, 0),
                vec![sample("a", 100, 10)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                HOUR + 1_000,
                2_000,
                bytes(0, 0),
                vec![sample("a", 250, 40)],
            ),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, HOUR + 1_000);
        let conn = &batch.active[0];
        assert_eq!(
            conn.hours,
            buckets([(0, bytes(100, 10)), (1, bytes(150, 30))])
        );
        assert_eq!(
            conn.minutes,
            buckets([(59, bytes(100, 10)), (60, bytes(150, 30))])
        );

        let rows = |from| session.active_rows(Tier::Hour, from).next().unwrap().1;
        assert_eq!(rows(None).bytes, bytes(250, 40));
        assert_eq!(rows(Some(1)).bytes, bytes(150, 30));
        assert_eq!(rows(Some(2)).bytes, Bytes::default());
        assert_eq!(rows(Some(2)).connections, 1);
    }

    #[test]
    fn minute_buckets_older_than_the_minute_tier_are_dropped_on_flush() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        let now = 1_000 * MINUTE;
        // 400 minutes ago, 360 (the cutoff itself), 300 and now.
        let at = |minutes_ago: i64| now - minutes_ago * MINUTE;
        for (i, minutes_ago) in [400, 360, 300, 0].into_iter().enumerate() {
            session.observe(
                frame(
                    INSTANCE,
                    at(minutes_ago),
                    i as u64,
                    bytes(0, 0),
                    vec![sample("a", (i as u64 + 1) * 10, 0)],
                ),
                None,
                &mut sequence,
            );
        }
        let before = take(&mut session, at(400));
        assert_eq!(before.active[0].minutes.len(), 4);

        let batch = take(&mut session, now);
        assert_eq!(batch.active.len(), 1, "a dropped bucket is a change");
        let conn = &batch.active[0];
        assert_eq!(
            conn.minutes.keys().copied().collect::<Vec<_>>(),
            [640, 700, 1000]
        );
        // The hours keep everything.
        assert_eq!(conn.bytes(), bytes(40, 0));
        assert_eq!(conn.hours.values().copied().sum_bytes(), bytes(40, 0));

        assert!(take(&mut session, now).active.is_empty());
    }

    trait SumBytes {
        fn sum_bytes(self) -> Bytes;
    }

    impl<I: Iterator<Item = Bytes>> SumBytes for I {
        fn sum_bytes(self) -> Bytes {
            self.fold(Bytes::default(), Bytes::saturating_add)
        }
    }

    #[test]
    fn unchanged_observations_keep_the_first_profile_without_an_upsert() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(10, 10), vec![sample("a", 10, 10)]),
            Some("first"),
            &mut sequence,
        );
        take(&mut session, 1_000);
        let mut unchanged = sample("a", 10, 10);
        unchanged.dimensions.profile = Some("reported".into());
        session.observe(
            frame(INSTANCE, 2_000, 1_000, bytes(10, 10), vec![unchanged]),
            Some("current"),
            &mut sequence,
        );
        assert!(take(&mut session, 2_000).active.is_empty());
        let mut changed = sample("a", 10, 10);
        changed.dimensions.target = "late-sniff.example".into();
        session.observe(
            frame(INSTANCE, 3_000, 2_000, bytes(10, 10), vec![changed]),
            Some("current"),
            &mut sequence,
        );
        let updated = take(&mut session, 3_000).active;
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].dimensions.target, "late-sniff.example");
        assert_eq!(updated[0].dimensions.profile.as_deref(), Some("first"));
    }

    #[test]
    fn profile_is_set_on_first_sight_and_never_rewritten() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(0, 0), vec![sample("a", 1, 1)]),
            Some("p1"),
            &mut sequence,
        );
        take(&mut session, 1_000);

        // The profile switches: "a" keeps its own, "b" gets the new one, nothing is cleared.
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(0, 0),
                vec![sample("a", 5, 5), sample("b", 7, 7)],
            ),
            Some("p2"),
            &mut sequence,
        );
        let batch = take(&mut session, 2_000);
        let by_id = |id: &str| batch.active.iter().find(|c| c.id == id).unwrap();
        assert_eq!(by_id("a").dimensions.profile.as_deref(), Some("p1"));
        assert_eq!(by_id("a").bytes(), bytes(5, 5));
        assert_eq!(by_id("b").dimensions.profile.as_deref(), Some("p2"));
        assert_eq!(by_id("b").bytes(), bytes(7, 7));
        assert!(batch.closed.is_empty());

        // Closed under the profile it was first seen with, even when the core reports another.
        let mut reported = sample("a", 5, 5);
        reported.dimensions.profile = Some("p3".into());
        session.observe(
            frame(
                INSTANCE,
                3_000,
                2_000,
                bytes(0, 0),
                vec![reported, sample("b", 7, 7)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(INSTANCE, 4_000, 3_000, bytes(0, 0), vec![]),
            Some("p9"),
            &mut sequence,
        );
        let closed = take(&mut session, 4_000).closed;
        assert_eq!(closed.len(), 2);
        let profile_of = |id: &str| {
            closed
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .dimensions
                .profile
                .clone()
        };
        assert_eq!(profile_of("a").as_deref(), Some("p1"));
        assert_eq!(profile_of("b").as_deref(), Some("p2"));
    }

    #[test]
    fn rates_need_elapsed_time_and_an_existing_connection() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(100, 100),
                vec![sample("a", 100, 100)],
            ),
            None,
            &mut sequence,
        );
        assert_eq!(session.summary(0).current_rate, None);
        assert!(rates_by(&session, Dimension::Process).is_empty());

        session.observe(
            frame(
                INSTANCE,
                3_000,
                2_000,
                bytes(300, 500),
                vec![sample("a", 200, 300), sample("b", 50, 50)],
            ),
            None,
            &mut sequence,
        );
        assert_eq!(
            session.summary(0).current_rate,
            Some(Rate {
                upload: 100,
                download: 200
            })
        );
        // "b" is new in this frame, so only "a" contributes.
        assert_eq!(
            rates_by(&session, Dimension::Process).get("curl"),
            Some(&Rate {
                upload: 50,
                download: 100
            })
        );

        // No monotonic progress: rates are unknown again, not stale.
        session.observe(
            frame(
                INSTANCE,
                4_000,
                2_000,
                bytes(300, 500),
                vec![sample("a", 200, 300)],
            ),
            None,
            &mut sequence,
        );
        assert_eq!(session.summary(0).current_rate, None);
        assert!(rates_by(&session, Dimension::Process).is_empty());
    }

    #[test]
    fn current_rate_sums_connections_per_group() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        let other = |id: &str, up, down| Sample {
            dimensions: dims("firefox"),
            ..sample(id, up, down)
        };
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(0, 0),
                vec![sample("a", 0, 0), sample("b", 0, 0), other("c", 0, 0)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(60, 0),
                vec![sample("a", 10, 0), sample("b", 20, 0), other("c", 30, 0)],
            ),
            None,
            &mut sequence,
        );

        let by_process = rates_by(&session, Dimension::Process);
        assert_eq!(by_process["curl"].upload, 30);
        assert_eq!(by_process["firefox"].upload, 30);
        assert_eq!(rates_by(&session, Dimension::Exit)["Node-A"].upload, 60);
    }

    #[test]
    fn rates_are_whole_bytes_per_second_rounded_down() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(0, 0), vec![sample("a", 0, 0)]),
            None,
            &mut sequence,
        );
        // 10 bytes over 3s is 3.33 B/s; 2 bytes over 3s is 0.67 B/s.
        session.observe(
            frame(
                INSTANCE,
                4_000,
                3_000,
                bytes(10, 2),
                vec![sample("a", 10, 2)],
            ),
            None,
            &mut sequence,
        );

        let expected = Rate {
            upload: 3,
            download: 0,
        };
        assert_eq!(session.summary(0).current_rate, Some(expected));
        assert_eq!(
            rates_by(&session, Dimension::Process).get("curl"),
            Some(&expected)
        );
    }

    #[test]
    fn rates_saturate() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(0, 0),
                vec![sample("a", 0, 0), sample("b", 0, 0)],
            ),
            None,
            &mut sequence,
        );
        // u64::MAX bytes within a millisecond overflows a u64 rate; two such connections
        // overflow their sum.
        session.observe(
            frame(
                INSTANCE,
                1_001,
                1,
                bytes(u64::MAX, 0),
                vec![sample("a", u64::MAX, 0), sample("b", u64::MAX, 0)],
            ),
            None,
            &mut sequence,
        );

        assert_eq!(session.summary(0).current_rate.unwrap().upload, u64::MAX);
        assert_eq!(
            rates_by(&session, Dimension::Process)["curl"],
            Rate {
                upload: u64::MAX,
                download: 0
            }
        );
    }

    #[test]
    fn disconnect_clears_rates_but_keeps_baselines() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(100, 100),
                vec![sample("a", 100, 100)],
            ),
            None,
            &mut sequence,
        );
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(150, 150),
                vec![sample("a", 150, 150)],
            ),
            None,
            &mut sequence,
        );
        assert!(session.summary(0).current_rate.is_some());

        session.disconnect();
        assert_eq!(session.summary(0).current_rate, None);
        assert!(rates_by(&session, Dimension::Process).is_empty());
        assert_eq!(session.summary(0).active_connections, 1);

        take(&mut session, 2_000);
        session.observe(
            frame(
                INSTANCE,
                9_000,
                8_000,
                bytes(160, 160),
                vec![sample("a", 160, 160)],
            ),
            None,
            &mut sequence,
        );
        // Baselines survived: only the delta is counted, and no rate is known yet.
        assert_eq!(take(&mut session, 9_000).active[0].bytes(), bytes(160, 160));
        assert_eq!(session.summary(0).current_rate, None);
    }

    #[test]
    fn restore_counts_only_the_delta_since_the_last_flush() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(
                INSTANCE,
                1_000,
                0,
                bytes(100, 100),
                vec![sample("a", 100, 100)],
            ),
            Some("p1"),
            &mut sequence,
        );
        let batch = take(&mut session, 1_000);

        let mut restored = Session::restore(batch.meta, batch.active);
        restored.observe(
            frame(
                INSTANCE,
                7_000,
                0,
                bytes(120, 140),
                vec![sample("a", 120, 140)],
            ),
            Some("p2"),
            &mut sequence,
        );

        assert_eq!(restored.summary(0).current_rate, None);
        let batch = take(&mut restored, 7_000);
        assert!(batch.closed.is_empty());
        assert_eq!(batch.active[0].bytes(), bytes(120, 140));
        // The restored connection keeps the profile it was stored with.
        assert_eq!(batch.active[0].dimensions.profile.as_deref(), Some("p1"));
    }

    #[test]
    fn take_batch_drains_pending_state() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(10, 10), vec![sample("a", 10, 10)]),
            None,
            &mut sequence,
        );
        session.observe(
            frame(INSTANCE, 2_000, 1_000, bytes(10, 10), vec![]),
            None,
            &mut sequence,
        );

        let batch = take(&mut session, 2_000);
        assert_eq!(batch.meta.last_sample_at, Some(2_000));
        assert_eq!(batch.closed.len(), 1);
        assert_eq!(batch.removed, ["a"]);
        assert_eq!(batch.hours.len(), 1);
        assert_eq!(batch.minutes.len(), 1);

        let again = take(&mut session, 2_000);
        assert!(again.closed.is_empty());
        assert!(again.removed.is_empty());
        assert!(again.minutes.is_empty());
        assert!(again.hours.is_empty());
        assert_eq!(session.pending_rows(Tier::Hour, None).count(), 0);
        assert_eq!(again.meta, batch.meta);
    }

    // ---- everything = live + closed -------------------------------------------------------

    /// A fixed run of frames covering several hours, with dimension changes, closes, profile
    /// switches and an instance switch, and the traffic it must account for.
    struct Scenario {
        frames: Vec<(Frame, &'static str)>,
        minutes: BTreeMap<u32, Bytes>,
        hours: BTreeMap<u32, Bytes>,
    }

    fn scenario() -> Scenario {
        const BASE: i64 = 100 * HOUR + 17_000;
        // (id, first step, last step exclusive, seed)
        let connections = [
            ("a", 0, 48, 1u64),
            ("b", 3, 17, 2),
            ("c", 10, 31, 3),
            ("d", 25, 48, 4),
            ("e", 30, 36, 5),
            ("f", 40, 44, 6),
        ];
        let mut cumulative: HashMap<&str, Bytes> = HashMap::new();
        let mut scenario = Scenario {
            frames: Vec::new(),
            minutes: BTreeMap::new(),
            hours: BTreeMap::new(),
        };
        for step in 0..48u64 {
            let wall_ms = BASE + step as i64 * 8 * MINUTE;
            // The core restarts: its counters start over and every connection is new to it.
            let instance = if step < 36 { "core-1" } else { "core-2" };
            if step == 36 {
                cumulative.clear();
            }
            let mut samples = Vec::new();
            for &(id, from, to, seed) in &connections {
                if !(from..to).contains(&step) {
                    continue;
                }
                let grown = if (step + seed) % 5 == 0 {
                    0
                } else {
                    ((step * 7 + seed * 13) % 11 + 1) * 100
                };
                let increment = bytes(grown, grown * 3);
                let total = cumulative.entry(id).or_default();
                *total = total.saturating_add(increment);
                for (map, bucket) in [
                    (&mut scenario.minutes, minute_of(wall_ms)),
                    (&mut scenario.hours, hour_of(wall_ms)),
                ] {
                    let slot = map.entry(bucket).or_default();
                    *slot = slot.saturating_add(increment);
                }
                let mut dimensions = dims(if id == "c" && step >= 20 {
                    "wget"
                } else {
                    "curl"
                });
                dimensions.target = format!("host-{}.com", seed % 3);
                dimensions.chains[0] = format!("Node-{}", seed % 2);
                samples.push(Sample {
                    id: id.into(),
                    started_at: 1_000,
                    counters: *total,
                    dimensions,
                });
            }
            let profile = if step < 24 { "p1" } else { "p2" };
            scenario.frames.push((
                frame(
                    instance,
                    wall_ms,
                    step * 8 * 60_000,
                    Bytes::default(),
                    samples,
                ),
                profile,
            ));
        }
        scenario
    }

    /// The store, kept in memory: what a flush adds up.
    #[derive(Default)]
    struct MemStore {
        minutes: Pending,
        hours: Pending,
        closed: usize,
    }

    impl MemStore {
        fn apply(&mut self, batch: &FlushBatch) {
            for (target, rows) in [
                (&mut self.minutes, &batch.minutes),
                (&mut self.hours, &batch.hours),
            ] {
                for (bucket, dimensions, usage) in rows {
                    add_usage(target, *bucket, dimensions, *usage);
                }
            }
            self.closed += batch.closed.len();
        }

        /// Per dimension combination, from bucket `from` on.
        fn usage(&self, tier: Tier, from: Option<u32>) -> Vec<(Arc<Dimensions>, Usage)> {
            let table = match tier {
                Tier::Minute => &self.minutes,
                Tier::Hour => &self.hours,
            };
            let mut sums: HashMap<Arc<Dimensions>, Usage> = HashMap::new();
            for ((bucket, dimensions), usage) in table {
                if from.is_none_or(|from| *bucket >= from) {
                    let slot = sums.entry(Arc::clone(dimensions)).or_default();
                    *slot = slot.saturating_add(*usage);
                }
            }
            sums.into_iter().collect()
        }
    }

    const RANGES: [TrafficRange; 6] = [
        TrafficRange::LastHour,
        TrafficRange::Last6Hours,
        TrafficRange::Last24Hours,
        TrafficRange::Last7Days,
        TrafficRange::Last30Days,
        TrafficRange::All,
    ];

    /// Plays the scenario, flushing after every `flush_every` frames; returns the end state and
    /// the time of the last frame.
    fn play(scenario: &Scenario, flush_every: Option<usize>) -> (Session, MemStore, i64) {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        let mut store = MemStore::default();
        let mut now = 0;
        for (i, (frame, profile)) in scenario.frames.iter().enumerate() {
            now = frame.wall_ms;
            session.observe(frame.clone(), Some(*profile), &mut sequence);
            if flush_every.is_some_and(|n| (i + 1) % n == 0) {
                store.apply(&session.take_batch(now));
            }
        }
        (session, store, now)
    }

    fn report_of<'a>(rows: &[Row<'a>], scope: TrafficScope) -> TrafficReport {
        report(
            rows.iter().copied(),
            &ReportRequest {
                query: TrafficQuery {
                    range: TrafficRange::All,
                    scope,
                    filters: Vec::new(),
                },
                metric: Metric::Bytes,
                rankings: Dimension::ALL.to_vec(),
                ranking_limit: 200,
                topology: None,
            },
        )
    }

    #[test]
    fn all_is_active_plus_closed_for_every_range_tier_and_dimension() {
        let scenario = scenario();
        for flush_every in [None, Some(1), Some(5), Some(13)] {
            let (session, store, now) = play(&scenario, flush_every);
            assert!(session.summary(0).active_connections > 0);
            let closed_total = store.closed + session.pending_closed().len();

            for range in RANGES {
                let (tier, from) = (range.tier(), range.start(now));
                let stored = store.usage(tier, from);
                let closed: Vec<Row> = stored
                    .iter()
                    .map(|(d, u)| (&**d, *u, None))
                    .chain(session.pending_rows(tier, from))
                    .collect();
                let active: Vec<Row> = session.active_rows(tier, from).collect();
                let all: Vec<Row> = closed.iter().chain(&active).copied().collect();
                let context = format!("{range:?} flushing every {flush_every:?}");

                let (all_r, active_r, closed_r) = (
                    report_of(&all, TrafficScope::All),
                    report_of(&active, TrafficScope::Active),
                    report_of(&closed, TrafficScope::Closed),
                );
                assert_eq!(
                    all_r.total,
                    active_r.total.saturating_add(closed_r.total),
                    "{context}"
                );
                for (i, ranking) in all_r.rankings.iter().enumerate() {
                    let side = |r: &TrafficReport, key: &str| {
                        r.rankings[i]
                            .groups
                            .iter()
                            .find(|g| g.key == key)
                            .map(|g| g.usage)
                            .unwrap_or_default()
                    };
                    for group in &ranking.groups {
                        assert_eq!(
                            group.usage,
                            side(&active_r, &group.key).saturating_add(side(&closed_r, &group.key)),
                            "{context}, {:?} {}",
                            ranking.dimension,
                            group.key
                        );
                    }
                    assert_eq!(
                        ranking.distinct as usize,
                        ranking.groups.len(),
                        "{context}, {:?}",
                        ranking.dimension
                    );
                }

                // The traffic is what the frames carried, no more and no less.
                let oracle = match tier {
                    Tier::Minute => &scenario.minutes,
                    Tier::Hour => &scenario.hours,
                };
                let expected = oracle
                    .range(from.unwrap_or(0)..)
                    .map(|(_, b)| *b)
                    .sum_bytes();
                assert_eq!(all_r.total.bytes, expected, "{context}");
            }

            // Every closed connection is counted once, wherever its usage is kept.
            let stored = store.usage(Tier::Hour, None);
            let closed: Vec<Row> = stored
                .iter()
                .map(|(d, u)| (&**d, *u, None))
                .chain(session.pending_rows(Tier::Hour, None))
                .collect();
            assert_eq!(
                report_of(&closed, TrafficScope::Closed).total.connections,
                closed_total as u64,
                "flushing every {flush_every:?}"
            );
        }
    }

    #[test]
    fn where_the_flushes_fall_does_not_change_any_report() {
        let scenario = scenario();
        let reports = |flush_every| {
            let (session, store, now) = play(&scenario, flush_every);
            RANGES.map(|range| {
                let (tier, from) = (range.tier(), range.start(now));
                let stored = store.usage(tier, from);
                let rows: Vec<Row> = stored
                    .iter()
                    .map(|(d, u)| (&**d, *u, None))
                    .chain(session.pending_rows(tier, from))
                    .chain(session.active_rows(tier, from))
                    .collect();
                report_of(&rows, TrafficScope::All)
            })
        };
        let reference = reports(None);
        for flush_every in [1, 2, 7, 13] {
            assert_eq!(reports(Some(flush_every)), reference, "every {flush_every}");
        }
    }

    #[test]
    fn a_requeued_batch_is_written_exactly_once() {
        let mut sequence = sequence_source();
        let scenario = scenario();
        let now = scenario.frames.last().unwrap().0.wall_ms;

        let mut session = Session::new();
        let mut store = MemStore::default();
        // The frames a refused session ignored.
        let mut ignored = HashSet::new();
        let mut refused = false;
        // What the store holds of the live connections.
        let mut stored_active: HashMap<String, ActiveConnection> = HashMap::new();
        let mut attempts = 0;
        let mut flush = |session: &mut Session, store: &mut MemStore, now, fail: bool| {
            let batch = session.take_batch(now);
            if fail {
                session.requeue(batch);
                return;
            }
            store.apply(&batch);
            for id in &batch.removed {
                stored_active.remove(id);
            }
            for conn in &batch.active {
                stored_active.insert(conn.id.clone(), conn.clone());
            }
        };
        for (i, (frame, profile)) in scenario.frames.iter().enumerate() {
            if refused {
                ignored.insert(i);
            }
            session.observe(frame.clone(), Some(*profile), &mut sequence);
            if (i + 1) % 3 == 0 {
                attempts += 1;
                // Two failures in a row, then one that works.
                refused = attempts % 3 != 0;
                flush(&mut session, &mut store, frame.wall_ms, refused);
            }
        }
        flush(&mut session, &mut store, now, false);

        assert!(!ignored.is_empty());
        // What a store that never refuses holds after the frames that were not ignored.
        let mut reference_session = Session::new();
        let mut reference = MemStore::default();
        for (i, (frame, profile)) in scenario.frames.iter().enumerate() {
            if !ignored.contains(&i) {
                reference_session.observe(frame.clone(), Some(*profile), &mut sequence);
            }
        }
        reference.apply(&reference_session.take_batch(now));

        let sorted = |rows: Vec<(Arc<Dimensions>, Usage)>| {
            let mut rows: Vec<_> = rows.into_iter().map(|(d, u)| ((*d).clone(), u)).collect();
            rows.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
            rows
        };
        for tier in [Tier::Minute, Tier::Hour] {
            assert_eq!(
                sorted(store.usage(tier, None)),
                sorted(reference.usage(tier, None)),
                "{tier:?}"
            );
        }
        assert_eq!(store.closed, reference.closed);
        let mut live: Vec<_> = stored_active.into_values().collect();
        let mut expected: Vec<_> = session.active.values().cloned().collect();
        live.sort_by(|a, b| a.id.cmp(&b.id));
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(live, expected);
    }

    /// What a session holds that a flush has not written: these must not grow while it is refused.
    fn buffered(session: &Session) -> [usize; 7] {
        [
            session.active.len(),
            session.dirty.len(),
            session.minutes.len(),
            session.hours.len(),
            session.closed.len(),
            session.removed.len(),
            session.rates.len(),
        ]
    }

    #[test]
    fn a_refused_session_ignores_frames_and_loses_nothing_it_held() {
        let mut sequence = sequence_source();
        const LAST_ACCEPTED: usize = 20;
        const RECOVERED: usize = 34;
        let scenario = scenario();
        let flushes = [9, RECOVERED];
        let ignored = LAST_ACCEPTED + 1..=RECOVERED;

        // The same frames minus the ignored ones, on a store that never refuses.
        let mut reference = Session::new();
        let mut reference_store = MemStore::default();
        for (i, (frame, profile)) in scenario.frames.iter().enumerate() {
            if ignored.contains(&i) {
                continue;
            }
            reference.observe(frame.clone(), Some(*profile), &mut sequence);
            if flushes.contains(&i) {
                reference_store.apply(&reference.take_batch(frame.wall_ms));
            }
        }
        let now = scenario.frames.last().unwrap().0.wall_ms;
        reference_store.apply(&reference.take_batch(now));

        let mut session = Session::new();
        let mut store = MemStore::default();
        let mut stored_active: HashMap<String, ActiveConnection> = HashMap::new();
        let mut commit = |store: &mut MemStore, batch: &FlushBatch| {
            store.apply(batch);
            for id in &batch.removed {
                stored_active.remove(id);
            }
            for conn in &batch.active {
                stored_active.insert(conn.id.clone(), conn.clone());
            }
        };
        let mut held = None;
        for (i, (frame, profile)) in scenario.frames.iter().enumerate() {
            session.observe(frame.clone(), Some(*profile), &mut sequence);
            if i == 9 {
                commit(&mut store, &session.take_batch(frame.wall_ms));
            }
            if i == LAST_ACCEPTED {
                // The store refuses from here on.
                let batch = session.take_batch(frame.wall_ms);
                session.requeue(batch);
                let buffers = buffered(&session);
                assert!(buffers[2..6].iter().all(|&n| n > 0), "{buffers:?}");
                held = Some(buffers);
            } else if ignored.contains(&i) {
                // Nothing a frame carries is kept, nor the rates it would have given.
                assert_eq!(Some(buffered(&session)), held, "frame {i}");
                assert_eq!(session.summary(0).current_rate, None);
                if i % 5 == 0 && i < RECOVERED {
                    // Another refused retry.
                    let batch = session.take_batch(frame.wall_ms);
                    session.requeue(batch);
                    assert_eq!(Some(buffered(&session)), held, "retry at frame {i}");
                }
            }
            if i == RECOVERED {
                let batch = session.take_batch(frame.wall_ms);
                assert_eq!(batch.closed.len(), held.unwrap()[4]);
                commit(&mut store, &batch);
                // Held and written once: the next batch has none of it.
                let again = session.take_batch(frame.wall_ms);
                assert!(again.closed.is_empty() && again.removed.is_empty());
                assert!(again.minutes.is_empty() && again.hours.is_empty());
            }
        }
        commit(&mut store, &session.take_batch(now));

        let all = |session: &Session, store: &MemStore, range: TrafficRange| {
            let (tier, from) = (range.tier(), range.start(now));
            let stored = store.usage(tier, from);
            let rows: Vec<Row> = stored
                .iter()
                .map(|(d, u)| (&**d, *u, None))
                .chain(session.pending_rows(tier, from))
                .chain(session.active_rows(tier, from))
                .collect();
            report_of(&rows, TrafficScope::All)
        };
        for range in RANGES {
            // Recovered, the totals are those of the frames that were taken, no more.
            assert_eq!(
                all(&session, &store, range),
                all(&reference, &reference_store, range),
                "{range:?}"
            );
        }
        assert_eq!(store.closed, reference_store.closed);
        // The live connections the store holds are the live connections now.
        let mut live: Vec<_> = stored_active.into_values().collect();
        let mut expected: Vec<_> = session.active.values().cloned().collect();
        live.sort_by(|a, b| a.id.cmp(&b.id));
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(live, expected);
    }

    #[test]
    fn a_refused_session_resumes_with_the_delta_since_the_last_accepted_frame() {
        let mut sequence = sequence_source();
        let mut session = Session::new();
        session.observe(
            frame(INSTANCE, 1_000, 0, bytes(100, 0), vec![sample("a", 100, 0)]),
            None,
            &mut sequence,
        );
        let batch = take(&mut session, 1_000);
        session.requeue(batch);

        // Ignored, whatever it carries.
        session.observe(
            frame(
                INSTANCE,
                2_000,
                1_000,
                bytes(300, 0),
                vec![sample("a", 300, 0), sample("b", 5, 0)],
            ),
            None,
            &mut sequence,
        );
        assert_eq!(session.summary(0).active_connections, 1);
        assert_eq!(session.summary(0).last_sample_at, Some(1_000));

        let batch = take(&mut session, 3_000);
        assert_eq!(batch.active.len(), 1);
        assert_eq!(batch.active[0].bytes(), bytes(100, 0));

        // Accepted again: the whole growth since the frame before the outage, counted once.
        session.observe(
            frame(
                INSTANCE,
                4_000,
                3_000,
                bytes(400, 0),
                vec![sample("a", 400, 0)],
            ),
            None,
            &mut sequence,
        );
        let rows: Vec<_> = session.active_rows(Tier::Hour, None).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.bytes, bytes(400, 0));
        assert_eq!(take(&mut session, 4_000).active[0].bytes(), bytes(400, 0));
        // No rate from the first frame after the outage.
        assert_eq!(session.summary(0).current_rate, None);
        assert!(rates_by(&session, Dimension::Process).is_empty());
    }
}
