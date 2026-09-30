use crate::{model::*, ports::TrafficStore, topology::path_dimensions};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

const SCHEMA_VERSION: u64 = 2;
const META: TableDefinition<&str, u64> = TableDefinition::new("traffic_meta");
const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("traffic_sessions_v1");
const CONNECTIONS: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("traffic_connections_v1");
const ORDER: TableDefinition<(&str, u64, &str), &str> =
    TableDefinition::new("traffic_connections_order_v1");
const FACTS: TableDefinition<(&str, &str, u64, u64), &[u8]> =
    TableDefinition::new("traffic_attribution_facts_v1");
const MEMBERS: TableDefinition<(&str, &str, &str), u64> =
    TableDefinition::new("traffic_path_members_v1");
const PATH_COUNTS: TableDefinition<(&str, &str), u64> =
    TableDefinition::new("traffic_path_counts_v1");
const GROUP_TOTALS: TableDefinition<(&str, &str, &str), &[u8]> =
    TableDefinition::new("traffic_group_totals_v1");
const RANKINGS: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("traffic_top500_v2");
const FILTER_INDEX: TableDefinition<(&str, &str, &str, u64, &str), u64> =
    TableDefinition::new("traffic_connection_filters_v1");
const RECEIPTS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("traffic_receipts_v1");

/// A dedicated, durable database. The cache budget covers redb pages, not process RSS.
pub struct RedbTrafficStore {
    db: Arc<Database>,
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError> {
    match tokio::task::spawn_blocking(operation).await {
        Ok(result) => result,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(StoreError::Unavailable(error.to_string())),
    }
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(value).map_err(|error| StoreError::Corrupt(error.to_string()))
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    serde_json::from_slice(bytes).map_err(|error| StoreError::Corrupt(error.to_string()))
}

fn storage_error(error: impl Into<redb::Error>) -> StoreError {
    match error.into() {
        redb::Error::Corrupted(message) => StoreError::Corrupt(message),
        redb::Error::UpgradeRequired(version) => StoreError::IncompatibleSchema(format!(
            "redb file format {version} requires explicit upgrade"
        )),
        error @ (redb::Error::TableTypeMismatch { .. }
        | redb::Error::TypeDefinitionChanged { .. }
        | redb::Error::TableIsMultimap(_)
        | redb::Error::TableIsNotMultimap(_)) => StoreError::IncompatibleSchema(error.to_string()),
        redb::Error::Io(error) if error.kind() == std::io::ErrorKind::StorageFull => {
            StoreError::CapacityExhausted(error.to_string())
        }
        error => StoreError::Unavailable(error.to_string()),
    }
}

fn durable_transaction(db: &Database) -> Result<redb::WriteTransaction, StoreError> {
    let mut transaction = db.begin_write().map_err(storage_error)?;
    transaction
        .set_durability(Durability::Immediate)
        .map_err(storage_error)?;
    Ok(transaction)
}

fn commit(transaction: redb::WriteTransaction) -> Result<(), StoreError> {
    // Once commit starts, IO failure cannot prove which side of the durable boundary ran.
    transaction
        .commit()
        .map_err(|error| StoreError::UnknownOutcome(error.to_string()))
}

impl RedbTrafficStore {
    pub async fn open(path: impl Into<PathBuf>, cache_bytes: usize) -> Result<Self, StoreError> {
        let path = path.into();
        blocking(move || {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|error| StoreError::Unavailable(error.to_string()))?;
            }
            let existed = path.exists();
            let db = Database::builder()
                .set_cache_size(cache_bytes)
                .create(&path)
                .map_err(storage_error)?;
            let transaction = durable_transaction(&db)?;
            {
                let mut meta = transaction.open_table(META).map_err(storage_error)?;
                let version = meta
                    .get("schema")
                    .map_err(storage_error)?
                    .map(|v| v.value());
                if version != Some(SCHEMA_VERSION) && (version.is_some() || existed) {
                    return Err(StoreError::IncompatibleSchema(format!(
                        "expected {SCHEMA_VERSION}, found {version:?}"
                    )));
                }
                meta.insert("schema", SCHEMA_VERSION)
                    .map_err(storage_error)?;
            }
            transaction.open_table(SESSIONS).map_err(storage_error)?;
            transaction.open_table(CONNECTIONS).map_err(storage_error)?;
            transaction.open_table(ORDER).map_err(storage_error)?;
            transaction.open_table(FACTS).map_err(storage_error)?;
            transaction.open_table(MEMBERS).map_err(storage_error)?;
            transaction.open_table(PATH_COUNTS).map_err(storage_error)?;
            transaction
                .open_table(GROUP_TOTALS)
                .map_err(storage_error)?;
            transaction
                .open_table(FILTER_INDEX)
                .map_err(storage_error)?;
            transaction.open_table(RECEIPTS).map_err(storage_error)?;
            transaction.open_table(RANKINGS).map_err(storage_error)?;
            commit(transaction)?;
            Ok(Self { db: Arc::new(db) })
        })
        .await
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&Database) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let db = self.db.clone();
        blocking(move || operation(&db)).await
    }
}

fn read_session(db: &redb::ReadTransaction, id: &SessionId) -> TrafficResult<SessionRecord> {
    decode(
        db.open_table(SESSIONS)
            .map_err(storage_error)?
            .get(id.0.as_str())
            .map_err(storage_error)?
            .ok_or(StoreError::NotFound)?
            .value(),
    )
}

fn write_session(tx: &redb::WriteTransaction, session: &SessionRecord) -> TrafficResult<()> {
    tx.open_table(SESSIONS)
        .map_err(storage_error)?
        .insert(session.id.0.as_str(), encode(session)?.as_slice())
        .map_err(storage_error)?;
    Ok(())
}

fn write_connection(tx: &redb::WriteTransaction, record: &ConnectionRecord) -> TrafficResult<()> {
    let mut connections = tx.open_table(CONNECTIONS).map_err(storage_error)?;
    let previous: Option<ConnectionRecord> = connections
        .get((record.session_id.0.as_str(), record.id.as_str()))
        .map_err(storage_error)?
        .map(|v| decode(v.value()))
        .transpose()?;
    if previous.as_ref().is_some_and(|old| {
        old.first_observed_sequence != record.first_observed_sequence
            || old.first_observed_at != record.first_observed_at
    }) {
        return Err(StoreError::Conflict(
            "immutable connection order changed".into(),
        ));
    }
    if previous.as_ref().is_none_or(|old| {
        old.dimensions != record.dimensions
            || matches!(old.status, ConnectionStatus::Active)
                != matches!(record.status, ConnectionStatus::Active)
    }) {
        let mut index = tx.open_table(FILTER_INDEX).map_err(storage_error)?;
        if let Some(previous) = &previous {
            for (kind, key) in index_keys(previous)? {
                if kind != "state" {
                    continue;
                }
                index
                    .remove((
                        record.session_id.0.as_str(),
                        kind,
                        key.as_str(),
                        record.first_observed_sequence.0,
                        record.id.as_str(),
                    ))
                    .map_err(storage_error)?;
            }
        }
        for (kind, key) in index_keys(record)? {
            index
                .insert(
                    (
                        record.session_id.0.as_str(),
                        kind,
                        key.as_str(),
                        record.first_observed_sequence.0,
                        record.id.as_str(),
                    ),
                    0,
                )
                .map_err(storage_error)?;
        }
    }
    connections
        .insert(
            (record.session_id.0.as_str(), record.id.as_str()),
            encode(record)?.as_slice(),
        )
        .map_err(storage_error)?;
    if previous.is_none() {
        tx.open_table(ORDER)
            .map_err(storage_error)?
            .insert(
                (
                    record.session_id.0.as_str(),
                    record.first_observed_sequence.0,
                    record.id.as_str(),
                ),
                record.id.as_str(),
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn index_keys(record: &ConnectionRecord) -> TrafficResult<Vec<(&'static str, String)>> {
    let d = &record.dimensions;
    Ok(vec![
        (
            "state",
            matches!(record.status, ConnectionStatus::Active).to_string(),
        ),
        ("process", d.process.clone()),
        ("source", d.source.clone()),
        ("target", d.target.clone()),
        ("protocol", d.protocol.clone()),
        ("exit", d.exit.clone()),
        (
            "rule-reported",
            serde_json::to_string(&(&d.rule.kind, &d.rule.payload))
                .map_err(|e| StoreError::InvalidData(e.to_string()))?,
        ),
        (
            "rule-context",
            serde_json::to_string(&(&d.rule.kind, &d.rule.payload, &d.rule.context))
                .map_err(|e| StoreError::InvalidData(e.to_string()))?,
        ),
        (
            "path",
            serde_json::to_string(&d.path).map_err(|e| StoreError::InvalidData(e.to_string()))?,
        ),
    ])
}

#[async_trait::async_trait]
impl TrafficStore for RedbTrafficStore {
    async fn connections_by_ids(
        &self,
        session: SessionId,
        ids: Vec<String>,
    ) -> TrafficResult<BTreeMap<String, ConnectionRecord>> {
        self.run(move |db| {
            let tx = db.begin_read().map_err(storage_error)?;
            read_session(&tx, &session)?;
            let table = tx.open_table(CONNECTIONS).map_err(storage_error)?;
            let mut records = BTreeMap::new();
            for id in ids {
                if let Some(record) = table
                    .get((session.0.as_str(), id.as_str()))
                    .map_err(storage_error)?
                {
                    records.insert(id, decode(record.value())?);
                }
            }
            Ok(records)
        })
        .await
    }
    async fn latest_session(&self, host: HostId) -> TrafficResult<Option<SessionRecord>> {
        self.run(move |db| {
            let tx = db.begin_read().map_err(storage_error)?;
            let mut latest: Option<SessionRecord> = None;
            for row in tx
                .open_table(SESSIONS)
                .map_err(storage_error)?
                .iter()
                .map_err(storage_error)?
            {
                let (_, value) = row.map_err(storage_error)?;
                let session: SessionRecord = decode(value.value())?;
                if session.host == host
                    && latest.as_ref().is_none_or(|previous| {
                        (session.attached_at, &session.id) > (previous.attached_at, &previous.id)
                    })
                {
                    latest = Some(session);
                }
            }
            Ok(latest)
        })
        .await
    }
    async fn recover(&self, host: HostId) -> TrafficResult<RecoveryState> {
        self.run(move |db| {
            let tx = db.begin_read().map_err(storage_error)?;
            let records = tx.open_table(SESSIONS).map_err(storage_error)?;
            let connections = tx.open_table(CONNECTIONS).map_err(storage_error)?;
            let mut sessions = Vec::new();
            for row in records.iter().map_err(storage_error)? {
                let (_, value) = row.map_err(storage_error)?;
                let session: SessionRecord = decode(value.value())?;
                if session.host != host || session.ended_at.is_some() {
                    continue;
                }
                let mut active_connections = Vec::new();
                for row in connections
                    .range((session.id.0.as_str(), "")..=(session.id.0.as_str(), "\u{10ffff}"))
                    .map_err(storage_error)?
                {
                    let (_, value) = row.map_err(storage_error)?;
                    let record: ConnectionRecord = decode(value.value())?;
                    if matches!(record.status, ConnectionStatus::Active) {
                        active_connections.push(record);
                    }
                }
                sessions.push(RecoveredSession {
                    session,
                    active_connections,
                });
            }
            Ok(RecoveryState { sessions })
        })
        .await
    }

    async fn begin_session(&self, new: NewSession) -> TrafficResult<SessionRecord> {
        self.run(move |db| {
            let tx = durable_transaction(db)?;
            let id = new.id();
            let existing = tx
                .open_table(SESSIONS)
                .map_err(storage_error)?
                .get(id.0.as_str())
                .map_err(storage_error)?
                .map(|v| decode::<SessionRecord>(v.value()))
                .transpose()?;
            if let Some(existing) = existing {
                if existing.host != new.host
                    || existing.instance_id != new.instance_id
                    || existing.process_started_at != new.process_started_at
                {
                    return Err(StoreError::Conflict("session identity changed".into()));
                }
                return Ok(existing);
            }
            let session = SessionRecord {
                id,
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
                    Vec::new()
                },
                freshness: Freshness::Unavailable,
                observed_connections: UInt(0),
            };
            write_session(&tx, &session)?;
            commit(tx)?;
            Ok(session)
        })
        .await
    }

    async fn commit_observation(&self, batch: ObservationCommit) -> TrafficResult<CommitReceipt> {
        self.run(move |db| {
            if crate::accounting::observation_digest(&batch)? != batch.digest {
                return Err(StoreError::Conflict(
                    "observation payload does not match digest".into(),
                ));
            }
            let tx = durable_transaction(db)?;
            let id = &batch.session.id;
            let existing: SessionRecord = decode(
                tx.open_table(SESSIONS)
                    .map_err(storage_error)?
                    .get(id.0.as_str())
                    .map_err(storage_error)?
                    .ok_or(StoreError::NotFound)?
                    .value(),
            )?;
            let replay: Option<CommitReceipt> = tx
                .open_table(RECEIPTS)
                .map_err(storage_error)?
                .get((id.0.as_str(), batch.sequence.0))
                .map_err(storage_error)?
                .map(|v| decode(v.value()))
                .transpose()?;
            if let Some(receipt) = replay {
                if receipt.position.digest != batch.digest {
                    return Err(StoreError::Conflict("observation digest changed".into()));
                }
                return Ok(receipt);
            }
            if existing.ended_at.is_some()
                || existing.position != batch.expected_previous
                || batch.sequence.0
                    != existing
                        .position
                        .sequence
                        .0
                        .checked_add(1)
                        .ok_or_else(|| StoreError::InvalidData("sequence exhausted".into()))?
                || batch.session.position
                    != (CommittedPosition {
                        sequence: batch.sequence,
                        digest: batch.digest.clone(),
                    })
                || existing.host != batch.session.host
                || existing.instance_id != batch.session.instance_id
                || batch.session.ended_at.is_some()
            {
                return Err(StoreError::Conflict(
                    "observation predecessor or identity mismatch".into(),
                ));
            }
            for record in &batch.connections {
                if record.session_id != *id || record.first_observed_sequence > batch.sequence {
                    return Err(StoreError::InvalidData(
                        "connection belongs to wrong observation".into(),
                    ));
                }
                write_connection(&tx, record)?;
            }
            {
                let mut facts = tx.open_table(FACTS).map_err(storage_error)?;
                let mut members = tx.open_table(MEMBERS).map_err(storage_error)?;
                let mut aggregate_deltas = BTreeMap::<(&str, String), Bytes>::new();
                let mut membership_deltas = BTreeMap::<String, u64>::new();
                for fact in &batch.facts {
                    if fact.session_id != *id || fact.sequence != batch.sequence {
                        return Err(StoreError::InvalidData(
                            "fact belongs to wrong observation".into(),
                        ));
                    }
                    let minute = fact
                        .minute
                        .map(|v| {
                            v.0.checked_add(1)
                                .ok_or_else(|| StoreError::InvalidData("minute exhausted".into()))
                        })
                        .transpose()?
                        .unwrap_or(0);
                    let key = (
                        id.0.as_str(),
                        fact.connection_id.as_str(),
                        fact.segment.0,
                        minute,
                    );
                    let previous: Option<AttributionFact> = facts
                        .get(key)
                        .map_err(storage_error)?
                        .map(|v| decode(v.value()))
                        .transpose()?;
                    let mut merged = fact.clone();
                    if let Some(previous) = previous {
                        if previous.dimensions != fact.dimensions {
                            return Err(StoreError::Conflict(
                                "attribution segment identity changed".into(),
                            ));
                        }
                        merged.bytes = previous.bytes.checked_add(&fact.bytes)?;
                        merged.interval_from = match (previous.interval_from, fact.interval_from) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            _ => None,
                        };
                        merged.interval_until = previous.interval_until.max(fact.interval_until);
                    }
                    facts
                        .insert(key, encode(&merged)?.as_slice())
                        .map_err(storage_error)?;
                    let dimensions = serde_json::to_string(&path_dimensions(&fact.dimensions))
                        .map_err(|e| StoreError::InvalidData(e.to_string()))?;
                    if members
                        .get((
                            id.0.as_str(),
                            dimensions.as_str(),
                            fact.connection_id.as_str(),
                        ))
                        .map_err(storage_error)?
                        .is_none()
                    {
                        *membership_deltas.entry(dimensions.clone()).or_default() += 1;
                        members
                            .insert(
                                (
                                    id.0.as_str(),
                                    dimensions.as_str(),
                                    fact.connection_id.as_str(),
                                ),
                                0,
                            )
                            .map_err(storage_error)?;
                    }
                    if fact.minute.is_none() {
                        let delta = aggregate_deltas
                            .entry(("topology-unallocated", dimensions.clone()))
                            .or_default();
                        *delta = delta.checked_add(&fact.bytes)?;
                    }
                    let path_delta = aggregate_deltas
                        .entry(("topology", dimensions))
                        .or_default();
                    *path_delta = path_delta.checked_add(&fact.bytes)?;
                    for group in [
                        GroupBy::Process,
                        GroupBy::Source,
                        GroupBy::Target,
                        GroupBy::Protocol,
                        GroupBy::Rule,
                        GroupBy::Exit,
                        GroupBy::Path,
                    ] {
                        let group_kind = group_kind(&group);
                        let group_key = crate::accounting::group_key(&fact.dimensions, &group);
                        let delta = aggregate_deltas.entry((group_kind, group_key)).or_default();
                        *delta = delta.checked_add(&fact.bytes)?;
                        if fact.minute.is_none() && matches!(group, GroupBy::Rule) {
                            let key = crate::accounting::group_key(&fact.dimensions, &group);
                            let delta = aggregate_deltas
                                .entry(("rule-unallocated", key))
                                .or_default();
                            *delta = delta.checked_add(&fact.bytes)?;
                        }
                    }
                }
                let mut totals = tx.open_table(GROUP_TOTALS).map_err(storage_error)?;
                let mut rankings = tx.open_table(RANKINGS).map_err(storage_error)?;
                let mut changed_rankings = BTreeMap::new();
                for ((kind, key), delta) in aggregate_deltas {
                    let previous = totals
                        .get((id.0.as_str(), kind, key.as_str()))
                        .map_err(storage_error)?
                        .map(|v| decode::<Bytes>(v.value()))
                        .transpose()?
                        .unwrap_or_default();
                    let updated = previous.checked_add(&delta)?;
                    if matches!(
                        kind,
                        "process" | "source" | "target" | "protocol" | "rule" | "exit" | "path"
                    ) {
                        if !changed_rankings.contains_key(kind) {
                            let rows: Vec<(String, Bytes)> = rankings
                                .get((id.0.as_str(), kind))
                                .map_err(storage_error)?
                                .map(|value| decode(value.value()))
                                .transpose()?
                                .unwrap_or_default();
                            let ordered: BTreeMap<_, _> = rows
                                .into_iter()
                                .map(|(key, bytes)| {
                                    ((std::cmp::Reverse(weight(&bytes)), key), bytes)
                                })
                                .collect();
                            changed_rankings.insert(kind, ordered);
                        }
                        let ranked = changed_rankings.get_mut(kind).expect("ranking initialized");
                        ranked.remove(&(std::cmp::Reverse(weight(&previous)), key.clone()));
                        let candidate = (std::cmp::Reverse(weight(&updated)), key.clone());
                        if ranked.len() < 500
                            || ranked
                                .last_key_value()
                                .is_some_and(|(worst, _)| candidate < *worst)
                        {
                            ranked.insert(candidate, updated.clone());
                            if ranked.len() > 500 {
                                ranked.pop_last();
                            }
                        }
                    }
                    totals
                        .insert(
                            (id.0.as_str(), kind, key.as_str()),
                            encode(&updated)?.as_slice(),
                        )
                        .map_err(storage_error)?;
                }
                for (kind, ranked) in changed_rankings {
                    let rows: Vec<_> = ranked
                        .into_iter()
                        .map(|((_, key), bytes)| (key, bytes))
                        .collect();
                    rankings
                        .insert((id.0.as_str(), kind), encode(&rows)?.as_slice())
                        .map_err(storage_error)?;
                }
                let mut counts = tx.open_table(PATH_COUNTS).map_err(storage_error)?;
                for (key, delta) in membership_deltas {
                    let previous = counts
                        .get((id.0.as_str(), key.as_str()))
                        .map_err(storage_error)?
                        .map(|v| v.value())
                        .unwrap_or(0);
                    counts
                        .insert(
                            (id.0.as_str(), key.as_str()),
                            previous.checked_add(delta).ok_or_else(|| {
                                StoreError::InvalidData("path membership exhausted".into())
                            })?,
                        )
                        .map_err(storage_error)?;
                }
            }
            write_session(&tx, &batch.session)?;
            let receipt = CommitReceipt {
                session_id: id.clone(),
                position: batch.session.position,
            };
            tx.open_table(RECEIPTS)
                .map_err(storage_error)?
                .remove((id.0.as_str(), existing.position.sequence.0))
                .map_err(storage_error)?;
            tx.open_table(RECEIPTS)
                .map_err(storage_error)?
                .insert(
                    (id.0.as_str(), batch.sequence.0),
                    encode(&receipt)?.as_slice(),
                )
                .map_err(storage_error)?;
            commit(tx)?;
            Ok(receipt)
        })
        .await
    }

    async fn committed_position(&self, id: SessionId) -> TrafficResult<CommittedPosition> {
        Ok(self.session(id).await?.position)
    }
    async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord> {
        self.run(move |db| read_session(&db.begin_read().map_err(storage_error)?, &id))
            .await
    }
    async fn connection(
        &self,
        session: SessionId,
        id: String,
    ) -> TrafficResult<Option<ConnectionRecord>> {
        self.run(move |db| {
            let tx = db.begin_read().map_err(storage_error)?;
            read_session(&tx, &session)?;
            tx.open_table(CONNECTIONS)
                .map_err(storage_error)?
                .get((session.0.as_str(), id.as_str()))
                .map_err(storage_error)?
                .map(|v| decode(v.value()))
                .transpose()
        })
        .await
    }

    async fn finish_session(&self, end: SessionEnd) -> TrafficResult<CommitReceipt> {
        self.run(move |db| {
            let tx = durable_transaction(db)?;
            let mut session: SessionRecord = decode(
                tx.open_table(SESSIONS)
                    .map_err(storage_error)?
                    .get(end.session_id.0.as_str())
                    .map_err(storage_error)?
                    .ok_or(StoreError::NotFound)?
                    .value(),
            )?;
            let end_digest = crate::accounting::digest(&end)?;
            if let Some(detected) = session.ended_at {
                if detected != end.detected_at || session.position.digest != end_digest {
                    return Err(StoreError::Conflict(
                        "session already ended with another result".into(),
                    ));
                }
                return Ok(CommitReceipt {
                    session_id: session.id,
                    position: session.position,
                });
            }
            // Walk one ordered key at a time; archived history never becomes a temporary Vec.
            let mut after = (0, String::new());
            loop {
                let next = {
                    let index = tx.open_table(FILTER_INDEX).map_err(storage_error)?;
                    index
                        .range((
                            std::ops::Bound::Excluded((
                                session.id.0.as_str(),
                                "state",
                                "true",
                                after.0,
                                after.1.as_str(),
                            )),
                            std::ops::Bound::Included((
                                session.id.0.as_str(),
                                "state",
                                "true",
                                u64::MAX,
                                "\u{10ffff}",
                            )),
                        ))
                        .map_err(storage_error)?
                        .next()
                        .transpose()
                        .map_err(storage_error)?
                        .map(|(key, _)| {
                            let key = key.value();
                            (key.3, key.4.to_owned())
                        })
                };
                let Some(next) = next else { break };
                after = next;
                let mut record: ConnectionRecord = decode(
                    tx.open_table(CONNECTIONS)
                        .map_err(storage_error)?
                        .get((session.id.0.as_str(), after.1.as_str()))
                        .map_err(storage_error)?
                        .ok_or_else(|| StoreError::Corrupt("orphan order index".into()))?
                        .value(),
                )?;
                if matches!(record.status, ConnectionStatus::Active) {
                    record.status = ConnectionStatus::Closed {
                        detected_at: end.detected_at,
                        reason: end.reason.clone(),
                        final_counters_exact: false,
                    };
                    write_connection(&tx, &record)?;
                }
            }
            session.ended_at = Some(end.detected_at);
            session.freshness = Freshness::Ended;
            let previous_sequence = session.position.sequence;
            session.position = CommittedPosition {
                sequence: UInt(
                    session
                        .position
                        .sequence
                        .0
                        .checked_add(1)
                        .ok_or_else(|| StoreError::InvalidData("sequence exhausted".into()))?,
                ),
                digest: end_digest,
            };
            write_session(&tx, &session)?;
            let receipt = CommitReceipt {
                session_id: session.id,
                position: session.position,
            };
            tx.open_table(RECEIPTS)
                .map_err(storage_error)?
                .remove((receipt.session_id.0.as_str(), previous_sequence.0))
                .map_err(storage_error)?;
            tx.open_table(RECEIPTS)
                .map_err(storage_error)?
                .insert(
                    (receipt.session_id.0.as_str(), receipt.position.sequence.0),
                    encode(&receipt)?.as_slice(),
                )
                .map_err(storage_error)?;
            commit(tx)?;
            Ok(receipt)
        })
        .await
    }

    async fn query_connections(&self, query: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        self.run(move |db| connections_query(db, query)).await
    }
    async fn query_usage(&self, query: UsageQuery) -> TrafficResult<UsageResult> {
        self.run(move |db| usage_query(db, query)).await
    }
    async fn query_topology(&self, query: TopologyQuery) -> TrafficResult<TopologyResult> {
        self.run(move |db| topology_query(db, query)).await
    }

    async fn prune(&self, policy: RetentionPolicy) -> TrafficResult<PruneReport> {
        self.run(move |db| {
            let tx = durable_transaction(db)?;
            let mut newest: Option<(UInt, SessionId)> = None;
            if policy.keep_last_ended {
                for row in tx
                    .open_table(SESSIONS)
                    .map_err(storage_error)?
                    .iter()
                    .map_err(storage_error)?
                {
                    let (_, value) = row.map_err(storage_error)?;
                    let session: SessionRecord = decode(value.value())?;
                    if session.host == policy.host
                        && let Some(end) = session.ended_at
                        && newest.as_ref().is_none_or(|(time, _)| end > *time)
                    {
                        newest = Some((end, session.id));
                    }
                }
            }
            let mut removed = Vec::new();
            loop {
                let candidate = {
                    let sessions = tx.open_table(SESSIONS).map_err(storage_error)?;
                    let mut candidate = None;
                    for row in sessions.iter().map_err(storage_error)? {
                        let (_, value) = row.map_err(storage_error)?;
                        let session: SessionRecord = decode(value.value())?;
                        if session.host == policy.host
                            && session.ended_at.is_some()
                            && policy.protected_session.as_ref() != Some(&session.id)
                            && newest.as_ref().is_none_or(|(_, id)| *id != session.id)
                        {
                            candidate = Some(session.id);
                            break;
                        }
                    }
                    candidate
                };
                let Some(id) = candidate else { break };
                tx.open_table(CONNECTIONS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(ORDER)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(FILTER_INDEX)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(FACTS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(MEMBERS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(PATH_COUNTS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(GROUP_TOTALS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(RANKINGS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(RECEIPTS)
                    .map_err(storage_error)?
                    .retain(|key, _| key.0 != id.0)
                    .map_err(storage_error)?;
                tx.open_table(SESSIONS)
                    .map_err(storage_error)?
                    .remove(id.0.as_str())
                    .map_err(storage_error)?;
                removed.push(id);
            }
            commit(tx)?;
            Ok(PruneReport { removed })
        })
        .await
    }
    async fn flush(&self) -> TrafficResult<()> {
        self.run(move |db| commit(durable_transaction(db)?)).await
    }
}

const MAX_AGGREGATES: usize = 4096;

fn connection_lifecycle_matches(
    record: &ConnectionRecord,
    filter: &ConnectionFilter,
) -> TrafficResult<bool> {
    let parse = |value: &str| {
        chrono::DateTime::parse_from_rfc3339(value)
            .map_err(|e| StoreError::InvalidData(format!("invalid connection timestamp: {e}")))
    };
    let from = filter.started_after.as_deref().map(parse).transpose()?;
    let until = filter.started_before.as_deref().map(parse).transpose()?;
    if from.is_some() || until.is_some() {
        let Some(started) = record
            .sample
            .started_at
            .as_deref()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        else {
            return Ok(false);
        };
        if from.is_some_and(|from| started < from) || until.is_some_and(|until| started >= until) {
            return Ok(false);
        }
    }
    Ok(filter
        .status
        .is_none_or(|active| active == matches!(record.status, ConnectionStatus::Active)))
}

fn index_filter(filter: &ConnectionFilter) -> TrafficResult<Option<(&'static str, String)>> {
    if let Some(rule) = &filter.rule {
        let (kind, key) = if rule.context.is_some() {
            (
                "rule-context",
                serde_json::to_string(&(&rule.kind, &rule.payload, &rule.context)),
            )
        } else {
            (
                "rule-reported",
                serde_json::to_string(&(&rule.kind, &rule.payload)),
            )
        };
        return Ok(Some((
            kind,
            key.map_err(|e| StoreError::InvalidData(e.to_string()))?,
        )));
    }
    for (kind, value) in [
        ("process", &filter.process),
        ("target", &filter.target),
        ("exit", &filter.exit),
        ("protocol", &filter.protocol),
        ("source", &filter.source),
    ] {
        if let Some(value) = value {
            return Ok(Some((kind, value.clone())));
        }
    }
    if let Some(path) = &filter.path {
        return Ok(Some((
            "path",
            serde_json::to_string(path).map_err(|e| StoreError::InvalidData(e.to_string()))?,
        )));
    }
    if let Some(active) = filter.status {
        return Ok(Some(("state", active.to_string())));
    }
    Ok(None)
}

fn connections_query(db: &Database, query: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
    let tx = db.begin_read().map_err(storage_error)?;
    let session = read_session(&tx, &query.session_id).map_err(|e| {
        if query.cursor.is_some() && matches!(e, StoreError::NotFound) {
            StoreError::InvalidCursor
        } else {
            e
        }
    })?;
    let digest = crate::accounting::digest(&query.filter)?;
    validate_filter(&query.filter)?;
    let (high_watermark, after_sequence, after_id) = match &query.cursor {
        Some(cursor) => {
            if cursor.session_id != query.session_id
                || cursor.query_digest != digest
                || cursor.high_watermark > session.position.sequence
                || cursor.last_sequence > cursor.high_watermark
            {
                return Err(StoreError::InvalidCursor);
            }
            (
                cursor.high_watermark,
                cursor.last_sequence,
                cursor.last_id.clone(),
            )
        }
        None => (session.position.sequence, UInt(0), String::new()),
    };
    let limit = if query.limit == 0 {
        100
    } else {
        usize::from(query.limit).min(500)
    };
    let index = tx.open_table(ORDER).map_err(storage_error)?;
    let filter_index = tx.open_table(FILTER_INDEX).map_err(storage_error)?;
    let records = tx.open_table(CONNECTIONS).map_err(storage_error)?;
    let facts = tx.open_table(FACTS).map_err(storage_error)?;
    let mut connections = Vec::with_capacity(limit + 1);
    let indexed = index_filter(&query.filter)?;
    let candidates: Box<dyn Iterator<Item = TrafficResult<String>> + '_> =
        if let Some((kind, key)) = &indexed {
            Box::new(
                filter_index
                    .range((
                        std::ops::Bound::Excluded((
                            session.id.0.as_str(),
                            *kind,
                            key.as_str(),
                            after_sequence.0,
                            after_id.as_str(),
                        )),
                        std::ops::Bound::Included((
                            session.id.0.as_str(),
                            *kind,
                            key.as_str(),
                            high_watermark.0,
                            "\u{10ffff}",
                        )),
                    ))
                    .map_err(storage_error)?
                    .map(|row| {
                        row.map(|(key, _)| key.value().4.to_owned())
                            .map_err(storage_error)
                    }),
            )
        } else {
            Box::new(
                index
                    .range((
                        std::ops::Bound::Excluded((
                            session.id.0.as_str(),
                            after_sequence.0,
                            after_id.as_str(),
                        )),
                        std::ops::Bound::Included((
                            session.id.0.as_str(),
                            high_watermark.0,
                            "\u{10ffff}",
                        )),
                    ))
                    .map_err(storage_error)?
                    .map(|row| {
                        row.map(|(_, id)| id.value().to_owned())
                            .map_err(storage_error)
                    }),
            )
        };
    for id in candidates {
        let id = id?;
        let record: ConnectionRecord = decode(
            records
                .get((session.id.0.as_str(), id.as_str()))
                .map_err(storage_error)?
                .ok_or_else(|| StoreError::Corrupt("orphan connection index".into()))?
                .value(),
        )?;
        if !connection_lifecycle_matches(&record, &query.filter)? {
            continue;
        }
        let mut matches = crate::accounting::matches_dimensions(&record.dimensions, &query.filter);
        if !matches {
            for row in facts
                .range(
                    (session.id.0.as_str(), record.id.as_str(), 0, 0)
                        ..=(
                            session.id.0.as_str(),
                            record.id.as_str(),
                            u64::MAX,
                            u64::MAX,
                        ),
                )
                .map_err(storage_error)?
            {
                let (_, value) = row.map_err(storage_error)?;
                let fact: AttributionFact = decode(value.value())?;
                if crate::accounting::matches_dimensions(&fact.dimensions, &query.filter) {
                    matches = true;
                    break;
                }
            }
        }
        if matches {
            connections.push(record);
            if connections.len() > limit {
                break;
            }
        }
    }
    let more = connections.len() > limit;
    connections.truncate(limit);
    let next_cursor = if more {
        connections.last().map(|record| ConnectionCursor {
            session_id: session.id.clone(),
            query_digest: digest,
            high_watermark,
            last_sequence: record.first_observed_sequence,
            last_id: record.id.clone(),
        })
    } else {
        None
    };
    Ok(ConnectionPage {
        meta: QueryMeta {
            session,
            cross_page_snapshot: false,
        },
        connections,
        next_cursor,
    })
}

fn validate_filter(filter: &ConnectionFilter) -> TrafficResult<()> {
    let parse = |time: &str| {
        chrono::DateTime::parse_from_rfc3339(time)
            .map_err(|e| StoreError::InvalidData(format!("invalid query timestamp: {e}")))
    };
    let from = filter.started_after.as_deref().map(parse).transpose()?;
    let until = filter.started_before.as_deref().map(parse).transpose()?;
    if from.zip(until).is_some_and(|(from, until)| from >= until) {
        return Err(StoreError::InvalidData(
            "connection time range is empty or inverted".into(),
        ));
    }
    Ok(())
}

fn validate_scope(scope: &QueryScope) -> TrafficResult<()> {
    if let QueryScope::MinuteWindow { from, until } = scope
        && (from.0 >= until.0 || from.0 % 60000 != 0 || until.0 % 60000 != 0)
    {
        return Err(StoreError::InvalidData(
            "minute window requires increasing UTC minute-aligned millisecond boundaries".into(),
        ));
    }
    if let QueryScope::MinuteWindow { from, until } = scope
        && (until.0 - from.0) / 60000 > MAX_AGGREGATES as u64
    {
        return Err(StoreError::QueryTooBroad);
    }
    Ok(())
}

/// Streams durable facts through one read transaction. No historical connection list is retained.
fn scan_facts(
    tx: &redb::ReadTransaction,
    session: &SessionRecord,
    filter: &ConnectionFilter,
    scope: &QueryScope,
    mut consume: impl FnMut(AttributionFact) -> TrafficResult<()>,
) -> TrafficResult<()> {
    validate_scope(scope)?;
    validate_filter(filter)?;
    let records = tx.open_table(CONNECTIONS).map_err(storage_error)?;
    let facts = tx.open_table(FACTS).map_err(storage_error)?;
    let mut visit = |fact: AttributionFact| -> TrafficResult<()> {
        if !crate::accounting::matches_dimensions(&fact.dimensions, filter) {
            return Ok(());
        }
        if filter.status.is_some()
            || filter.started_after.is_some()
            || filter.started_before.is_some()
            || matches!(scope, QueryScope::Live)
        {
            let record: ConnectionRecord = decode(
                records
                    .get((session.id.0.as_str(), fact.connection_id.as_str()))
                    .map_err(storage_error)?
                    .ok_or_else(|| StoreError::Corrupt("orphan attribution fact".into()))?
                    .value(),
            )?;
            if !connection_lifecycle_matches(&record, filter)?
                || (matches!(scope, QueryScope::Live)
                    && !matches!(record.status, ConnectionStatus::Active))
            {
                return Ok(());
            }
        }
        if let QueryScope::MinuteWindow { from, until } = scope {
            if fact
                .minute
                .is_some_and(|minute| minute.0 < from.0 / 60000 || minute.0 >= until.0 / 60000)
            {
                return Ok(());
            }
            if fact.minute.is_none()
                && (fact.interval_until < *from
                    || fact.interval_from.is_some_and(|start| start >= *until))
            {
                return Ok(());
            }
        }
        consume(fact)
    };
    if matches!(scope, QueryScope::Live) || filter.status == Some(true) {
        let active = tx.open_table(FILTER_INDEX).map_err(storage_error)?;
        for row in active
            .range(
                (session.id.0.as_str(), "state", "true", 0, "")
                    ..=(
                        session.id.0.as_str(),
                        "state",
                        "true",
                        u64::MAX,
                        "\u{10ffff}",
                    ),
            )
            .map_err(storage_error)?
        {
            let (key, _) = row.map_err(storage_error)?;
            let id = key.value().4;
            for row in facts
                .range(
                    (session.id.0.as_str(), id, 0, 0)
                        ..=(session.id.0.as_str(), id, u64::MAX, u64::MAX),
                )
                .map_err(storage_error)?
            {
                let (_, value) = row.map_err(storage_error)?;
                visit(decode(value.value())?)?;
            }
        }
    } else {
        for row in facts
            .range(
                (session.id.0.as_str(), "", 0, 0)
                    ..=(session.id.0.as_str(), "\u{10ffff}", u64::MAX, u64::MAX),
            )
            .map_err(storage_error)?
        {
            let (_, value) = row.map_err(storage_error)?;
            visit(decode(value.value())?)?;
        }
    }
    Ok(())
}

fn bounded_add<K: Ord>(
    groups: &mut BTreeMap<K, Bytes>,
    key: K,
    bytes: &Bytes,
) -> TrafficResult<()> {
    if !groups.contains_key(&key) && groups.len() == MAX_AGGREGATES {
        return Err(StoreError::QueryTooBroad);
    }
    let previous = groups.entry(key).or_default();
    *previous = previous.checked_add(bytes)?;
    Ok(())
}

fn weight(bytes: &Bytes) -> u128 {
    u128::from(bytes.upload.0) + u128::from(bytes.download.0)
}

fn group_kind(group: &GroupBy) -> &'static str {
    match group {
        GroupBy::Process => "process",
        GroupBy::Source => "source",
        GroupBy::Target => "target",
        GroupBy::Protocol => "protocol",
        GroupBy::Rule => "rule",
        GroupBy::Exit => "exit",
        GroupBy::Path => "path",
    }
}

fn session_usage(
    tx: &redb::ReadTransaction,
    session: SessionRecord,
    group_by: Option<GroupBy>,
    limit: u16,
) -> TrafficResult<UsageResult> {
    let mut ranked = Vec::<(String, Bytes)>::new();
    let mut other = Bytes::default();
    if let Some(group) = group_by {
        let rankings = tx.open_table(RANKINGS).map_err(storage_error)?;
        let limit = usize::from(limit).clamp(1, 500);
        ranked = rankings
            .get((session.id.0.as_str(), group_kind(&group)))
            .map_err(storage_error)?
            .map(|value| decode(value.value()))
            .transpose()?
            .unwrap_or_default();
        ranked.truncate(limit);
        let mut listed = Bytes::default();
        for (_, bytes) in &ranked {
            listed = listed.checked_add(bytes)?;
        }
        other = Bytes {
            upload: UInt(
                session
                    .attributed_bytes
                    .upload
                    .0
                    .checked_sub(listed.upload.0)
                    .ok_or_else(|| {
                        StoreError::Corrupt("ranking upload exceeds session total".into())
                    })?,
            ),
            download: UInt(
                session
                    .attributed_bytes
                    .download
                    .0
                    .checked_sub(listed.download.0)
                    .ok_or_else(|| {
                        StoreError::Corrupt("ranking download exceeds session total".into())
                    })?,
            ),
        };
    }
    Ok(UsageResult {
        total: session.attributed_bytes.clone(),
        time_unallocated: session.time_unallocated.clone(),
        meta: QueryMeta {
            session,
            cross_page_snapshot: true,
        },
        groups: ranked
            .into_iter()
            .map(|(key, bytes)| UsageGroup {
                key,
                bytes,
                current_rate: None,
            })
            .collect(),
        other,
        minutes: Vec::new(),
        current_rate: None,
    })
}

fn usage_query(db: &Database, query: UsageQuery) -> TrafficResult<UsageResult> {
    let tx = db.begin_read().map_err(storage_error)?;
    let session = read_session(&tx, &query.session_id)?;
    if matches!(query.scope, QueryScope::Session)
        && matches!(query.group_by, None | Some(GroupBy::Rule))
        && let Some(rule) = &query.filter.rule
    {
        let mut remaining = query.filter.clone();
        remaining.rule = None;
        if remaining == ConnectionFilter::default() {
            let totals = tx.open_table(GROUP_TOTALS).map_err(storage_error)?;
            let mut total = Bytes::default();
            let mut time_unallocated = Bytes::default();
            let mut ranked = Vec::<(String, Bytes)>::new();
            let mut other = Bytes::default();
            let limit = usize::from(query.limit).clamp(1, 500);
            for (kind, target) in [
                ("rule", &mut total),
                ("rule-unallocated", &mut time_unallocated),
            ] {
                for row in totals
                    .range(
                        (session.id.0.as_str(), kind, "")
                            ..=(session.id.0.as_str(), kind, "\u{10ffff}"),
                    )
                    .map_err(storage_error)?
                {
                    let (key, value) = row.map_err(storage_error)?;
                    let group_key = key.value().2;
                    let key: RuleKey = serde_json::from_str(group_key)
                        .map_err(|e| StoreError::Corrupt(e.to_string()))?;
                    if key.kind == rule.kind
                        && key.payload == rule.payload
                        && rule
                            .context
                            .as_ref()
                            .is_none_or(|context| Some(context) == key.context.as_ref())
                    {
                        let bytes: Bytes = decode(value.value())?;
                        *target = target.checked_add(&bytes)?;
                        if kind == "rule" && query.group_by.is_some() {
                            ranked.push((group_key.to_owned(), bytes));
                            ranked.sort_by(|(ka, a), (kb, b)| {
                                weight(b).cmp(&weight(a)).then_with(|| ka.cmp(kb))
                            });
                            if ranked.len() > limit {
                                let (_, discarded) = ranked.pop().expect("ranked overflow");
                                other = other.checked_add(&discarded)?;
                            }
                        }
                    }
                }
            }
            return Ok(UsageResult {
                meta: QueryMeta {
                    session,
                    cross_page_snapshot: true,
                },
                total,
                groups: ranked
                    .into_iter()
                    .map(|(key, bytes)| UsageGroup {
                        key,
                        bytes,
                        current_rate: None,
                    })
                    .collect(),
                other,
                time_unallocated,
                minutes: Vec::new(),
                current_rate: None,
            });
        }
    }
    if matches!(query.scope, QueryScope::Session) && query.filter == ConnectionFilter::default() {
        return session_usage(&tx, session, query.group_by, query.limit);
    }
    let mut total = Bytes::default();
    let mut time_unallocated = Bytes::default();
    let mut groups = BTreeMap::new();
    let mut minutes = BTreeMap::new();
    scan_facts(&tx, &session, &query.filter, &query.scope, |fact| {
        if fact.minute.is_none() {
            time_unallocated = time_unallocated.checked_add(&fact.bytes)?;
        }
        if matches!(query.scope, QueryScope::MinuteWindow { .. }) && fact.minute.is_none() {
            return Ok(());
        }
        total = total.checked_add(&fact.bytes)?;
        if let Some(group) = &query.group_by {
            bounded_add(
                &mut groups,
                crate::accounting::group_key(&fact.dimensions, group),
                &fact.bytes,
            )?;
        }
        if matches!(query.scope, QueryScope::MinuteWindow { .. })
            && let Some(minute) = fact.minute
        {
            bounded_add(&mut minutes, minute, &fact.bytes)?;
        }
        Ok(())
    })?;
    let mut ranked: Vec<_> = groups.into_iter().collect();
    ranked.sort_by(|(ka, a), (kb, b)| weight(b).cmp(&weight(a)).then_with(|| ka.cmp(kb)));
    let limit = usize::from(query.limit).clamp(1, 500);
    let mut other = Bytes::default();
    for (_, bytes) in ranked.iter().skip(limit) {
        other = other.checked_add(bytes)?;
    }
    ranked.truncate(limit);
    Ok(UsageResult {
        meta: QueryMeta {
            session,
            cross_page_snapshot: true,
        },
        total,
        groups: ranked
            .into_iter()
            .map(|(key, bytes)| UsageGroup {
                key,
                bytes,
                current_rate: None,
            })
            .collect(),
        other,
        time_unallocated,
        minutes: minutes
            .into_iter()
            .map(|(minute, bytes)| MinuteBucket { minute, bytes })
            .collect(),
        current_rate: None,
    })
}

fn scan_topology_facts(
    tx: &redb::ReadTransaction,
    session: &SessionRecord,
    filter: &ConnectionFilter,
    scope: &QueryScope,
    mut consume: impl FnMut(AttributionFact) -> TrafficResult<()>,
) -> TrafficResult<()> {
    let records = tx.open_table(CONNECTIONS).map_err(storage_error)?;
    scan_facts(tx, session, filter, scope, |fact| {
        if matches!(scope, QueryScope::Live) {
            let record: ConnectionRecord = decode(
                records
                    .get((session.id.0.as_str(), fact.connection_id.as_str()))
                    .map_err(storage_error)?
                    .ok_or_else(|| StoreError::Corrupt("orphan topology fact".into()))?
                    .value(),
            )?;
            if path_dimensions(&record.dimensions) != path_dimensions(&fact.dimensions) {
                return Ok(());
            }
        }
        consume(fact)
    })
}

fn topology_query(db: &Database, query: TopologyQuery) -> TrafficResult<TopologyResult> {
    let tx = db.begin_read().map_err(storage_error)?;
    let session = read_session(&tx, &query.session_id)?;
    let mut paths = BTreeMap::<Dimensions, Bytes>::new();
    let mut time_unallocated = Bytes::default();
    let fast_session = matches!(query.scope, QueryScope::Session)
        && query.filter.status.is_none()
        && query.filter.started_after.is_none()
        && query.filter.started_before.is_none()
        && query.filter.target.is_none()
        && query.filter.protocol.is_none()
        && query.filter.source.is_none();
    let limit = usize::from(query.limit).clamp(1, 500);
    let mut fast_ranked = Vec::new();
    let mut fast_other = Bytes::default();
    if fast_session {
        let totals = tx.open_table(GROUP_TOTALS).map_err(storage_error)?;
        for row in totals
            .range(
                (session.id.0.as_str(), "topology", "")
                    ..=(session.id.0.as_str(), "topology", "\u{10ffff}"),
            )
            .map_err(storage_error)?
        {
            let (key, value) = row.map_err(storage_error)?;
            let dimensions: Dimensions = serde_json::from_str(key.value().2)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?;
            if !crate::accounting::matches_dimensions(&dimensions, &query.filter) {
                continue;
            }
            if let Some(unallocated) = totals
                .get((session.id.0.as_str(), "topology-unallocated", key.value().2))
                .map_err(storage_error)?
            {
                time_unallocated = time_unallocated.checked_add(&decode(unallocated.value())?)?;
            }
            fast_ranked.push((dimensions, decode::<Bytes>(value.value())?));
            fast_ranked
                .sort_by(|(da, a), (db, b)| weight(b).cmp(&weight(a)).then_with(|| da.cmp(db)));
            if fast_ranked.len() > limit {
                let (_, bytes) = fast_ranked.pop().expect("rank overflow");
                fast_other = fast_other.checked_add(&bytes)?;
            }
        }
    } else {
        scan_topology_facts(&tx, &session, &query.filter, &query.scope, |fact| {
            if fact.minute.is_none() {
                time_unallocated = time_unallocated.checked_add(&fact.bytes)?;
            }
            if matches!(query.scope, QueryScope::MinuteWindow { .. }) && fact.minute.is_none() {
                return Ok(());
            }
            bounded_add(&mut paths, path_dimensions(&fact.dimensions), &fact.bytes)
        })?;
    }
    let mut ranked: Vec<_> = if fast_session {
        fast_ranked
    } else {
        paths.into_iter().collect()
    };
    ranked.sort_by(|(da, a), (db, b)| weight(b).cmp(&weight(a)).then_with(|| da.cmp(db)));
    let mut other = fast_other;
    for (_, bytes) in ranked.iter().skip(limit) {
        other = other.checked_add(bytes)?;
    }
    ranked.truncate(limit);
    let mut paths = Vec::with_capacity(ranked.len());
    let membership_totals = tx.open_table(PATH_COUNTS).map_err(storage_error)?;
    let scoped_members = if !fast_session {
        let mut counts: BTreeMap<Dimensions, (String, u64)> = ranked
            .iter()
            .map(|(d, _)| (d.clone(), (String::new(), 0)))
            .collect();
        scan_topology_facts(&tx, &session, &query.filter, &query.scope, |fact| {
            if matches!(query.scope, QueryScope::MinuteWindow { .. }) && fact.minute.is_none() {
                return Ok(());
            }
            if let Some((last, count)) = counts.get_mut(&path_dimensions(&fact.dimensions))
                && (*count == 0 || *last != fact.connection_id)
            {
                *last = fact.connection_id;
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| StoreError::InvalidData("membership count exhausted".into()))?;
            }
            Ok(())
        })?;
        Some(counts)
    } else {
        None
    };
    for (dimensions, bytes) in ranked {
        let dimension_key = serde_json::to_string(&dimensions)
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        let memberships = if let Some(counts) = &scoped_members {
            counts
                .get(&dimensions)
                .map(|(_, count)| *count)
                .unwrap_or(0)
        } else {
            membership_totals
                .get((session.id.0.as_str(), dimension_key.as_str()))
                .map_err(storage_error)?
                .map(|v| v.value())
                .unwrap_or(0)
        };
        paths.push(TopologyPath {
            dimensions,
            bytes,
            memberships: UInt(memberships),
            current_rate: None,
        });
    }
    let (nodes, edges) = crate::topology::project(paths.clone())?;
    Ok(TopologyResult {
        meta: QueryMeta {
            session,
            cross_page_snapshot: true,
        },
        paths,
        nodes,
        edges,
        other,
        time_unallocated,
    })
}

#[cfg(test)]
mod blocking_tests {
    #[tokio::test]
    #[should_panic(expected = "storage adapter panic")]
    async fn blocking_panics_propagate() {
        let _ = super::blocking::<()>(|| panic!("storage adapter panic")).await;
    }
}
