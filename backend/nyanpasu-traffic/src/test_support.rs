//! In-memory test adapter. Production hosts must inject durable storage.
use crate::{
    accounting::{digest, group_key, matches_dimensions, observation_digest},
    model::*,
    ports::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
#[derive(Clone, Default)]
struct Data {
    sessions: BTreeMap<SessionId, SessionRecord>,
    connections: BTreeMap<(SessionId, String), ConnectionRecord>,
    facts: Vec<AttributionFact>,
    ends: BTreeMap<SessionId, SessionEnd>,
}
#[derive(Default)]
pub struct FakeTrafficStore {
    data: Mutex<Data>,
}
fn session(data: &Data, id: &SessionId) -> TrafficResult<SessionRecord> {
    data.sessions.get(id).cloned().ok_or(StoreError::NotFound)
}
fn record_matches(data: &Data, r: &ConnectionRecord, f: &ConnectionFilter) -> bool {
    f.status
        .is_none_or(|active| active == matches!(r.status, ConnectionStatus::Active))
        && f.started_after.as_ref().is_none_or(|s| {
            r.sample.started_at.as_ref().is_some_and(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .ok()
                    .zip(chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|(t, s)| t >= s)
            })
        })
        && f.started_before.as_ref().is_none_or(|s| {
            r.sample.started_at.as_ref().is_some_and(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .ok()
                    .zip(chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|(t, s)| t <= s)
            })
        })
        && (matches_dimensions(&r.dimensions, f)
            || data.facts.iter().any(|a| {
                a.session_id == r.session_id
                    && a.connection_id == r.id
                    && matches_dimensions(&a.dimensions, f)
            }))
}
fn fact_matches(
    data: &Data,
    fact: &AttributionFact,
    filter: &ConnectionFilter,
    scope: &QueryScope,
) -> bool {
    if !matches_dimensions(&fact.dimensions, filter) {
        return false;
    }
    let Some(record) = data
        .connections
        .get(&(fact.session_id.clone(), fact.connection_id.clone()))
    else {
        return false;
    };
    if !filter
        .status
        .is_none_or(|a| a == matches!(record.status, ConnectionStatus::Active))
    {
        return false;
    }
    match scope {
        QueryScope::Live => matches!(record.status, ConnectionStatus::Active),
        QueryScope::Session => true,
        QueryScope::MinuteWindow { from, until } => match fact.minute {
            Some(m) => m.0 >= from.0 / 60000 && m.0 < until.0 / 60000,
            None => {
                fact.interval_until >= *from
                    && fact.interval_from.is_none_or(|start| start < *until)
            }
        },
    }
}
#[async_trait::async_trait]
impl TrafficStore for FakeTrafficStore {
    async fn recover(&self, host: HostId) -> TrafficResult<RecoveryState> {
        let data = self.data.lock().unwrap();
        Ok(RecoveryState {
            sessions: data
                .sessions
                .values()
                .filter(|s| s.host == host && s.ended_at.is_none())
                .map(|s| RecoveredSession {
                    session: s.clone(),
                    active_connections: data
                        .connections
                        .values()
                        .filter(|r| {
                            r.session_id == s.id && matches!(r.status, ConnectionStatus::Active)
                        })
                        .cloned()
                        .collect(),
                })
                .collect(),
        })
    }
    async fn latest_session(&self, host: HostId) -> TrafficResult<Option<SessionRecord>> {
        let data = self.data.lock().unwrap();
        Ok(data
            .sessions
            .values()
            .filter(|s| s.host == host)
            .max_by_key(|s| s.attached_at)
            .cloned())
    }
    async fn begin_session(&self, new: NewSession) -> TrafficResult<SessionRecord> {
        let mut data = self.data.lock().unwrap();
        let id = new.id();
        if let Some(old) = data.sessions.get(&id) {
            return Ok(old.clone());
        }
        let s = SessionRecord {
            id: id.clone(),
            host: new.host,
            instance_id: new.instance_id,
            process_started_at: new.process_started_at,
            attached_at: new.attached_at,
            first_sample_at: None,
            last_sample_at: None,
            ended_at: None,
            core_reported_bytes: Bytes::default(),
            attributed_bytes: Bytes::default(),
            time_unallocated: Bytes::default(),
            global_counters: None,
            last_monotonic_ns: None,
            source_generation: UInt(0),
            position: CommittedPosition {
                sequence: UInt(0),
                digest: String::new(),
            },
            quality: if new.late_attach {
                vec![Quality::LateAttach]
            } else {
                vec![]
            },
            freshness: Freshness::Stale,
            observed_connections: UInt(0),
        };
        data.sessions.insert(id, s.clone());
        Ok(s)
    }
    async fn commit_observation(&self, batch: ObservationCommit) -> TrafficResult<CommitReceipt> {
        let mut data = self.data.lock().unwrap();
        let old = session(&data, &batch.session.id)?;
        if observation_digest(&batch)? != batch.digest {
            return Err(StoreError::Conflict("invalid digest".into()));
        }
        if old.position.sequence == batch.sequence && old.position.digest == batch.digest {
            return Ok(CommitReceipt {
                session_id: old.id,
                position: old.position,
            });
        }
        if old.ended_at.is_some()
            || old.position != batch.expected_previous
            || batch.sequence.0 != old.position.sequence.0 + 1
        {
            return Err(StoreError::Conflict("previous position mismatch".into()));
        }
        if batch
            .connections
            .iter()
            .any(|r| r.session_id != batch.session.id)
            || batch.facts.iter().any(|r| r.session_id != batch.session.id)
        {
            return Err(StoreError::InvalidData("cross-session update".into()));
        }
        let mut next = data.clone();
        for r in batch.connections {
            next.connections
                .insert((r.session_id.clone(), r.id.clone()), r);
        }
        next.facts.extend(batch.facts);
        let receipt = CommitReceipt {
            session_id: batch.session.id.clone(),
            position: batch.session.position.clone(),
        };
        next.sessions
            .insert(batch.session.id.clone(), batch.session);
        *data = next;
        Ok(receipt)
    }
    async fn committed_position(&self, id: SessionId) -> TrafficResult<CommittedPosition> {
        Ok(session(&self.data.lock().unwrap(), &id)?.position)
    }
    async fn finish_session(&self, end: SessionEnd) -> TrafficResult<CommitReceipt> {
        let mut data = self.data.lock().unwrap();
        let mut s = session(&data, &end.session_id)?;
        if let Some(previous) = data.ends.get(&end.session_id) {
            if previous == &end {
                return Ok(CommitReceipt {
                    session_id: s.id,
                    position: s.position,
                });
            }
            return Err(StoreError::Conflict("different end".into()));
        }
        s.position = CommittedPosition {
            sequence: UInt(s.position.sequence.0 + 1),
            digest: digest(&end)?,
        };
        s.ended_at = Some(end.detected_at);
        s.freshness = Freshness::Ended;
        for r in data.connections.values_mut().filter(|r| {
            r.session_id == end.session_id && matches!(r.status, ConnectionStatus::Active)
        }) {
            r.status = ConnectionStatus::Closed {
                detected_at: end.detected_at,
                reason: end.reason.clone(),
                final_counters_exact: false,
            };
        }
        let receipt = CommitReceipt {
            session_id: s.id.clone(),
            position: s.position.clone(),
        };
        data.sessions.insert(s.id.clone(), s);
        data.ends.insert(end.session_id.clone(), end);
        Ok(receipt)
    }
    async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord> {
        session(&self.data.lock().unwrap(), &id)
    }
    async fn connection(
        &self,
        id: SessionId,
        connection: String,
    ) -> TrafficResult<Option<ConnectionRecord>> {
        let data = self.data.lock().unwrap();
        session(&data, &id)?;
        Ok(data.connections.get(&(id, connection)).cloned())
    }
    async fn query_connections(&self, q: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        let data = self.data.lock().unwrap();
        let s = session(&data, &q.session_id)?;
        let query_digest = digest(&q.filter)?;
        let high = q
            .cursor
            .as_ref()
            .map(|c| c.high_watermark)
            .unwrap_or(s.position.sequence);
        if q.cursor
            .as_ref()
            .is_some_and(|c| c.session_id != q.session_id || c.query_digest != query_digest)
        {
            return Err(StoreError::InvalidCursor);
        }
        if q.limit > 500 {
            return Err(StoreError::QueryTooBroad);
        }
        let limit = if q.limit == 0 { 100 } else { q.limit as usize };
        let mut rows: Vec<_> = data
            .connections
            .values()
            .filter(|r| {
                r.session_id == q.session_id
                    && r.first_observed_sequence <= high
                    && record_matches(&data, r, &q.filter)
                    && q.cursor.as_ref().is_none_or(|c| {
                        (r.first_observed_sequence, &r.id) > (c.last_sequence, &c.last_id)
                    })
            })
            .cloned()
            .collect();
        rows.sort_by_key(|r| (r.first_observed_sequence, r.id.clone()));
        let more = rows.len() > limit;
        rows.truncate(limit);
        let cursor = if more {
            rows.last().map(|r| ConnectionCursor {
                session_id: q.session_id,
                query_digest,
                high_watermark: high,
                last_sequence: r.first_observed_sequence,
                last_id: r.id.clone(),
            })
        } else {
            None
        };
        Ok(ConnectionPage {
            meta: QueryMeta {
                session: s,
                cross_page_snapshot: true,
            },
            connections: rows,
            next_cursor: cursor,
        })
    }
    async fn query_usage(&self, q: UsageQuery) -> TrafficResult<UsageResult> {
        let data = self.data.lock().unwrap();
        let s = session(&data, &q.session_id)?;
        let mut total = Bytes::default();
        let mut groups: BTreeMap<String, Bytes> = BTreeMap::new();
        let mut minutes: BTreeMap<UInt, Bytes> = BTreeMap::new();
        let mut unallocated = Bytes::default();
        for f in data.facts.iter().filter(|f| f.session_id == q.session_id) {
            if fact_matches(&data, f, &q.filter, &q.scope) && f.minute.is_none() {
                unallocated = unallocated.checked_add(&f.bytes)?;
            }
            if fact_matches(&data, f, &q.filter, &q.scope)
                && !(matches!(q.scope, QueryScope::MinuteWindow { .. }) && f.minute.is_none())
            {
                total = total.checked_add(&f.bytes)?;
                if let Some(g) = &q.group_by {
                    let bytes = groups.entry(group_key(&f.dimensions, g)).or_default();
                    *bytes = bytes.checked_add(&f.bytes)?;
                }
                if let Some(m) = f.minute {
                    let bytes = minutes.entry(m).or_default();
                    *bytes = bytes.checked_add(&f.bytes)?;
                }
            }
        }
        let mut groups: Vec<_> = groups
            .into_iter()
            .map(|(key, bytes)| UsageGroup {
                key,
                bytes,
                current_rate: None,
            })
            .collect();
        groups.sort_by_key(|g| {
            std::cmp::Reverse(g.bytes.download.0 as u128 + g.bytes.upload.0 as u128)
        });
        let limit = if q.limit == 0 { 100 } else { q.limit as usize };
        let mut other = Bytes::default();
        for g in groups.iter().skip(limit) {
            other = other.checked_add(&g.bytes)?;
        }
        groups.truncate(limit);
        Ok(UsageResult {
            meta: QueryMeta {
                session: s,
                cross_page_snapshot: false,
            },
            total,
            groups,
            other,
            time_unallocated: unallocated,
            minutes: minutes
                .into_iter()
                .map(|(minute, bytes)| MinuteBucket { minute, bytes })
                .collect(),
            current_rate: None,
        })
    }
    async fn query_topology(&self, q: TopologyQuery) -> TrafficResult<TopologyResult> {
        let data = self.data.lock().unwrap();
        let s = session(&data, &q.session_id)?;
        let mut paths: BTreeMap<Dimensions, TopologyPath> = BTreeMap::new();
        let mut unallocated = Bytes::default();
        let mut members: BTreeMap<Dimensions, BTreeSet<String>> = BTreeMap::new();
        for f in data.facts.iter().filter(|f| f.session_id == q.session_id) {
            if fact_matches(&data, f, &q.filter, &q.scope) && f.minute.is_none() {
                unallocated = unallocated.checked_add(&f.bytes)?;
            }
            if fact_matches(&data, f, &q.filter, &q.scope)
                && !(matches!(q.scope, QueryScope::MinuteWindow { .. }) && f.minute.is_none())
            {
                let dimensions = crate::topology::path_dimensions(&f.dimensions);
                if matches!(q.scope, QueryScope::Live)
                    && data
                        .connections
                        .get(&(f.session_id.clone(), f.connection_id.clone()))
                        .is_some_and(|r| {
                            crate::topology::path_dimensions(&r.dimensions) != dimensions
                        })
                {
                    continue;
                }
                let count = members.entry(dimensions.clone()).or_default();
                count.insert(f.connection_id.clone());
                let membership_count = count.len() as u64;
                let p = paths.entry(dimensions.clone()).or_insert(TopologyPath {
                    dimensions,
                    bytes: Bytes::default(),
                    memberships: UInt(0),
                    current_rate: None,
                });
                p.bytes = p.bytes.checked_add(&f.bytes)?;
                p.memberships = UInt(membership_count);
            }
        }
        let mut paths: Vec<_> = paths.into_values().collect();
        paths.sort_by_key(|p| {
            std::cmp::Reverse(p.bytes.upload.0 as u128 + p.bytes.download.0 as u128)
        });
        let limit = if q.limit == 0 { 100 } else { q.limit as usize };
        let mut other = Bytes::default();
        for p in paths.iter().skip(limit) {
            other = other.checked_add(&p.bytes)?;
        }
        paths.truncate(limit);
        let (nodes, edges) = crate::topology::project(paths.clone())?;
        Ok(TopologyResult {
            meta: QueryMeta {
                session: s,
                cross_page_snapshot: false,
            },
            paths,
            nodes,
            edges,
            other,
            time_unallocated: unallocated,
        })
    }
    async fn prune(&self, p: RetentionPolicy) -> TrafficResult<PruneReport> {
        let mut data = self.data.lock().unwrap();
        let mut ended: Vec<_> = data
            .sessions
            .values()
            .filter(|s| s.host == p.host && s.ended_at.is_some())
            .map(|s| (s.ended_at, s.id.clone()))
            .collect();
        ended.sort();
        if p.keep_last_ended {
            ended.pop();
        }
        let removed: Vec<_> = ended
            .into_iter()
            .map(|(_, id)| id)
            .filter(|id| p.protected_session.as_ref() != Some(id))
            .collect();
        for id in &removed {
            data.sessions.remove(id);
            data.ends.remove(id);
        }
        data.connections.retain(|(id, _), _| !removed.contains(id));
        data.facts.retain(|f| !removed.contains(&f.session_id));
        Ok(PruneReport { removed })
    }
    async fn flush(&self) -> TrafficResult<()> {
        Ok(())
    }
}
pub struct FakeClock {
    pub wall: std::sync::atomic::AtomicU64,
    pub monotonic: std::sync::atomic::AtomicU64,
}
impl FakeClock {
    pub fn new(wall: u64, monotonic: u64) -> Self {
        Self {
            wall: wall.into(),
            monotonic: monotonic.into(),
        }
    }
}
impl Clock for FakeClock {
    fn wall_time(&self) -> UInt {
        UInt(self.wall.load(std::sync::atomic::Ordering::SeqCst))
    }
    fn monotonic_ns(&self) -> UInt {
        UInt(self.monotonic.load(std::sync::atomic::Ordering::SeqCst))
    }
}
pub struct IdleSource;
#[async_trait::async_trait]
impl TrafficSource for IdleSource {
    async fn connect(
        &self,
        _: SourceBinding,
        _: UInt,
        _: std::sync::Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream> {
        Ok(Box::pin(futures_util::stream::pending()))
    }
}
