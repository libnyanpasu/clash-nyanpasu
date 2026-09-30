use nyanpasu_traffic::{model::*, ports::TrafficStore, topology::path_dimensions};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, path::PathBuf};
use tokio::sync::Mutex;
use turso::{
    Connection, Value,
    transaction::{Transaction, TransactionBehavior},
};

const TABLES: [(&str, usize); 10] = [
    ("sessions", 1),
    ("connections", 2),
    ("ordering", 3),
    ("facts", 4),
    ("members", 3),
    ("path_counts", 2),
    ("totals", 3),
    ("rankings", 2),
    ("filters", 5),
    ("receipts", 2),
];

/// Experimental local Rust Turso engine adapter. Each command holds one connection
/// lock and one transaction; there is no operation queue or in-memory history cache.
pub struct TursoTrafficStore {
    _db: turso::Database,
    connection: Mutex<StoreConnection>,
}

struct StoreConnection {
    conn: Connection,
    failed: bool,
}
impl std::ops::Deref for StoreConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.conn
    }
}

fn error(error: turso::Error) -> StoreError {
    match &error {
        turso::Error::Corrupt(_) => StoreError::Corrupt(error.to_string()),
        turso::Error::IoError(std::io::ErrorKind::StorageFull, _) => {
            StoreError::CapacityExhausted(error.to_string())
        }
        _ => StoreError::Unavailable(error.to_string()),
    }
}
fn encode(value: &impl Serialize) -> TrafficResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| StoreError::InvalidData(e.to_string()))
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> TrafficResult<T> {
    serde_json::from_slice(bytes).map_err(|e| StoreError::Corrupt(e.to_string()))
}
fn key(value: u64) -> String {
    format!("{value:020}")
}
fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}
fn params(keys: &[&str]) -> Vec<Value> {
    keys.iter().map(|k| text(*k)).collect()
}
fn predicate(count: usize) -> String {
    (0..count)
        .map(|i| format!("k{i} = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}
async fn rows(conn: &Connection, sql: &str, args: Vec<Value>) -> TrafficResult<turso::Rows> {
    conn.prepare_cached(sql)
        .await
        .map_err(error)?
        .query(args)
        .await
        .map_err(error)
}
async fn execute(conn: &Connection, sql: &str, args: Vec<Value>) -> TrafficResult<u64> {
    conn.prepare_cached(sql)
        .await
        .map_err(error)?
        .execute(args)
        .await
        .map_err(error)
}
fn blob(row: &turso::Row, column: usize) -> TrafficResult<Vec<u8>> {
    match row.get_value(column).map_err(error)? {
        Value::Blob(value) => Ok(value),
        _ => Err(StoreError::Corrupt("expected traffic blob".into())),
    }
}
fn string(row: &turso::Row, column: usize) -> TrafficResult<String> {
    match row.get_value(column).map_err(error)? {
        Value::Text(value) => Ok(value),
        _ => Err(StoreError::Corrupt("expected traffic key".into())),
    }
}
async fn get<T: DeserializeOwned>(
    conn: &Connection,
    table: &str,
    keys: &[&str],
) -> TrafficResult<Option<T>> {
    let sql = format!("SELECT value FROM {table} WHERE {}", predicate(keys.len()));
    let mut result = rows(conn, &sql, params(keys)).await?;
    let row = result.next().await.map_err(error)?;
    let value = row.map(|row| decode(&blob(&row, 0)?)).transpose()?;
    while result.next().await.map_err(error)?.is_some() {}
    Ok(value)
}
async fn put(
    conn: &Connection,
    table: &str,
    keys: &[&str],
    value: &impl Serialize,
) -> TrafficResult<()> {
    let placeholders = (1..=keys.len() + 1)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("INSERT OR REPLACE INTO {table} VALUES ({placeholders})");
    let mut values = params(keys);
    values.push(Value::Blob(encode(value)?));
    execute(conn, &sql, values).await?;
    Ok(())
}
async fn delete(conn: &Connection, table: &str, keys: &[&str]) -> TrafficResult<()> {
    execute(
        conn,
        &format!("DELETE FROM {table} WHERE {}", predicate(keys.len())),
        params(keys),
    )
    .await?;
    Ok(())
}
async fn scan(
    conn: &Connection,
    table: &str,
    keys: &[&str],
    columns: &str,
) -> TrafficResult<turso::Rows> {
    let suffix = if keys.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", predicate(keys.len()))
    };
    // Composite primary-key order preserves connection contiguity for membership scans.
    let arity = TABLES
        .iter()
        .find(|(name, _)| *name == table)
        .expect("known table")
        .1;
    let order = (0..arity)
        .map(|i| format!("k{i}"))
        .collect::<Vec<_>>()
        .join(",");
    rows(
        conn,
        &format!("SELECT {columns} FROM {table}{suffix} ORDER BY {order}"),
        params(keys),
    )
    .await
}
async fn read_session(conn: &Connection, id: &SessionId) -> TrafficResult<SessionRecord> {
    get(conn, "sessions", &[&id.0])
        .await?
        .ok_or(StoreError::NotFound)
}
async fn commit(tx: Transaction<'_>) -> TrafficResult<()> {
    tx.commit()
        .await
        .map_err(|e| StoreError::UnknownOutcome(e.to_string()))
}

impl TursoTrafficStore {
    async fn lock(&self) -> TrafficResult<tokio::sync::MutexGuard<'_, StoreConnection>> {
        let conn = self.connection.lock().await;
        if conn.failed {
            return Err(StoreError::Unavailable(
                "transaction cleanup failed; reopen store".into(),
            ));
        }
        Ok(conn)
    }
    async fn write<T: Send>(
        &self,
        operation: impl for<'a> FnOnce(
            &'a Connection,
        ) -> futures_util::future::BoxFuture<'a, TrafficResult<T>>
        + Send,
    ) -> TrafficResult<T> {
        let mut conn = self.lock().await?;
        let tx = Transaction::new(&mut conn.conn, TransactionBehavior::Immediate)
            .await
            .map_err(error)?;
        match operation(&tx).await {
            Ok(value) => {
                if let Err(commit_error) = commit(tx).await {
                    conn.failed = cleanup(&conn.conn).await.is_err();
                    return Err(commit_error);
                }
                Ok(value)
            }
            Err(operation_error) => {
                if let Err(rollback_error) = tx.rollback().await {
                    conn.failed = cleanup(&conn.conn).await.is_err();
                    return Err(error(rollback_error));
                }
                Err(operation_error)
            }
        }
    }
    pub async fn open(path: impl Into<PathBuf>, cache_bytes: usize) -> TrafficResult<Self> {
        let path = path.into();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Unavailable(e.to_string()))?;
        }
        let existed = path.exists();
        let path_string = path
            .to_str()
            .ok_or_else(|| StoreError::InvalidData("database path is not UTF-8".into()))?;
        let db = turso::Builder::new_local(path_string)
            .build()
            .await
            .map_err(error)?;
        let mut conn = db.connect().map_err(error)?;
        conn.execute("PRAGMA synchronous = FULL", ())
            .await
            .map_err(error)?;
        conn.execute(
            format!("PRAGMA cache_size = -{}", (cache_bytes / 1024).max(1)),
            (),
        )
        .await
        .map_err(error)?;
        let durability = Self::settings(&conn).await?;
        if durability.0 != "wal" || durability.1 != 2 {
            return Err(StoreError::Unavailable(format!(
                "required WAL/FULL, found {durability:?}"
            )));
        }
        let mut version = conn.query("PRAGMA user_version", ()).await.map_err(error)?;
        let version_value = version
            .next()
            .await
            .map_err(error)?
            .ok_or_else(|| StoreError::Corrupt("missing user_version".into()))?
            .get_value(0)
            .map_err(error)?;
        while version.next().await.map_err(error)?.is_some() {}
        drop(version);
        if version_value != Value::Integer(1) && (existed || version_value != Value::Integer(0)) {
            return Err(StoreError::IncompatibleSchema(format!(
                "expected Turso traffic schema 1, found {version_value:?}"
            )));
        }
        let tx = Transaction::new(&mut conn, TransactionBehavior::Immediate)
            .await
            .map_err(error)?;
        for (table, arity) in TABLES {
            let keys = (0..arity)
                .map(|i| format!("k{i} TEXT NOT NULL"))
                .collect::<Vec<_>>()
                .join(",");
            let primary = (0..arity)
                .map(|i| format!("k{i}"))
                .collect::<Vec<_>>()
                .join(",");
            tx.execute(format!("CREATE TABLE IF NOT EXISTS {table} ({keys},value BLOB NOT NULL,PRIMARY KEY ({primary}))"), ()).await.map_err(error)?;
        }
        tx.execute("PRAGMA user_version = 1", ())
            .await
            .map_err(error)?;
        commit(tx).await?;
        Ok(Self {
            _db: db,
            connection: Mutex::new(StoreConnection {
                conn,
                failed: false,
            }),
        })
    }

    async fn settings(conn: &Connection) -> TrafficResult<(String, i64, i64)> {
        let mut values = Vec::new();
        for sql in [
            "PRAGMA journal_mode",
            "PRAGMA synchronous",
            "PRAGMA cache_size",
        ] {
            let mut result = conn.query(sql, ()).await.map_err(error)?;
            values.push(
                result
                    .next()
                    .await
                    .map_err(error)?
                    .ok_or_else(|| StoreError::Unavailable(format!("no result from {sql}")))?
                    .get_value(0)
                    .map_err(error)?,
            );
            while result.next().await.map_err(error)?.is_some() {}
        }
        match &values[..] {
            [
                Value::Text(mode),
                Value::Integer(sync),
                Value::Integer(cache),
            ] => Ok((mode.clone(), *sync, *cache)),
            _ => Err(StoreError::Unavailable(format!(
                "unexpected durability settings {values:?}"
            ))),
        }
    }

    /// Read back settings on the connection that executes all real commands.
    pub async fn durability_settings(&self) -> TrafficResult<(String, i64, i64)> {
        Self::settings(&*self.lock().await?).await
    }
}

async fn cleanup(conn: &Connection) -> TrafficResult<()> {
    // query() consumes Transaction::Drop's deferred rollback marker, whereas
    // prepare_cached() does not. Never read cached statements past this boundary.
    let mut rows = conn.query("SELECT 1", ()).await.map_err(error)?;
    while rows.next().await.map_err(error)?.is_some() {}
    Ok(())
}

async fn write_connection(conn: &Connection, record: &ConnectionRecord) -> TrafficResult<()> {
    let id = &record.session_id.0;
    let previous: Option<ConnectionRecord> = get(conn, "connections", &[id, &record.id]).await?;
    if previous.as_ref().is_some_and(|old| {
        old.first_observed_sequence != record.first_observed_sequence
            || old.first_observed_at != record.first_observed_at
    }) {
        return Err(StoreError::Conflict(
            "immutable connection order changed".into(),
        ));
    }
    let seq = key(record.first_observed_sequence.0);
    if previous.as_ref().is_none_or(|old| {
        old.dimensions != record.dimensions
            || matches!(old.status, ConnectionStatus::Active)
                != matches!(record.status, ConnectionStatus::Active)
    }) {
        if let Some(old) = &previous {
            for (kind, value) in index_keys(old)? {
                if kind == "state" {
                    delete(conn, "filters", &[id, kind, &value, &seq, &record.id]).await?;
                }
            }
        }
        for (kind, value) in index_keys(record)? {
            put(
                conn,
                "filters",
                &[id, kind, &value, &seq, &record.id],
                &0_u64,
            )
            .await?;
        }
    }
    put(conn, "connections", &[id, &record.id], record).await?;
    if previous.is_none() {
        put(conn, "ordering", &[id, &seq, &record.id], &record.id).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl TrafficStore for TursoTrafficStore {
    async fn begin_session(&self, new: NewSession) -> TrafficResult<SessionRecord> {
        self.write(move |conn| {
            Box::pin(async move {
                let id = new.id();
                if let Some(existing) = get::<SessionRecord>(conn, "sessions", &[&id.0]).await? {
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
                        vec![]
                    },
                    freshness: Freshness::Unavailable,
                    observed_connections: UInt(0),
                };
                put(conn, "sessions", &[&session.id.0], &session).await?;
                Ok(session)
            })
        })
        .await
    }
    async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord> {
        read_session(&*self.lock().await?, &id).await
    }
    async fn committed_position(&self, id: SessionId) -> TrafficResult<CommittedPosition> {
        Ok(self.session(id).await?.position)
    }
    async fn connection(
        &self,
        session: SessionId,
        id: String,
    ) -> TrafficResult<Option<ConnectionRecord>> {
        let conn = self.lock().await?;
        read_session(&conn, &session).await?;
        get(&conn, "connections", &[&session.0, &id]).await
    }
    async fn connections_by_ids(
        &self,
        session: SessionId,
        ids: Vec<String>,
    ) -> TrafficResult<BTreeMap<String, ConnectionRecord>> {
        let conn = self.lock().await?;
        read_session(&conn, &session).await?;
        let mut result = BTreeMap::new();
        for id in ids {
            if let Some(record) = get(&conn, "connections", &[&session.0, &id]).await? {
                result.insert(id, record);
            }
        }
        Ok(result)
    }
    async fn latest_session(&self, host: HostId) -> TrafficResult<Option<SessionRecord>> {
        let conn = self.lock().await?;
        let mut result = scan(&conn, "sessions", &[], "value").await?;
        let mut latest: Option<SessionRecord> = None;
        while let Some(row) = result.next().await.map_err(error)? {
            let session: SessionRecord = decode(&blob(&row, 0)?)?;
            if session.host == host
                && latest.as_ref().is_none_or(|old| {
                    (session.attached_at, &session.id) > (old.attached_at, &old.id)
                })
            {
                latest = Some(session);
            }
        }
        Ok(latest)
    }
    async fn recover(&self, host: HostId) -> TrafficResult<RecoveryState> {
        let conn = self.lock().await?;
        let mut records = scan(&conn, "sessions", &[], "value").await?;
        let mut sessions = Vec::new();
        while let Some(row) = records.next().await.map_err(error)? {
            let session: SessionRecord = decode(&blob(&row, 0)?)?;
            if session.host != host || session.ended_at.is_some() {
                continue;
            }
            let mut connections = scan(&conn, "connections", &[&session.id.0], "value").await?;
            let mut active_connections = Vec::new();
            while let Some(row) = connections.next().await.map_err(error)? {
                let record: ConnectionRecord = decode(&blob(&row, 0)?)?;
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
    }
    async fn commit_observation(&self, batch: ObservationCommit) -> TrafficResult<CommitReceipt> {
        if nyanpasu_traffic::accounting::observation_digest(&batch)? != batch.digest {
            return Err(StoreError::Conflict(
                "observation payload does not match digest".into(),
            ));
        }
        self.write(move |conn| {
            Box::pin(async move {
                let id = &batch.session.id;
                let existing = read_session(conn, id).await?;
                let sequence = key(batch.sequence.0);
                if let Some(receipt) =
                    get::<CommitReceipt>(conn, "receipts", &[&id.0, &sequence]).await?
                {
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
                    write_connection(conn, record).await?;
                }
                let mut deltas: BTreeMap<(&str, String), Bytes> = BTreeMap::new();
                let mut member_deltas: BTreeMap<String, u64> = BTreeMap::new();
                for fact in &batch.facts {
                    if fact.session_id != *id || fact.sequence != batch.sequence {
                        return Err(StoreError::InvalidData(
                            "fact belongs to wrong observation".into(),
                        ));
                    }
                    let segment = key(fact.segment.0);
                    let minute = key(fact.minute.map(|m| m.0).unwrap_or(u64::MAX));
                    let keys = [&id.0[..], &fact.connection_id, &segment, &minute];
                    let previous: Option<AttributionFact> = get(conn, "facts", &keys).await?;
                    let mut merged = fact.clone();
                    if let Some(old) = previous {
                        if old.dimensions != fact.dimensions {
                            return Err(StoreError::Conflict(
                                "attribution segment identity changed".into(),
                            ));
                        }
                        merged.bytes = old.bytes.checked_add(&fact.bytes)?;
                        merged.interval_from = match (old.interval_from, fact.interval_from) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            _ => None,
                        };
                        merged.interval_until = old.interval_until.max(fact.interval_until);
                    }
                    put(conn, "facts", &keys, &merged).await?;
                    let dimensions = serde_json::to_string(&path_dimensions(&fact.dimensions))
                        .map_err(|e| StoreError::InvalidData(e.to_string()))?;
                    if get::<u64>(conn, "members", &[&id.0, &dimensions, &fact.connection_id])
                        .await?
                        .is_none()
                    {
                        *member_deltas.entry(dimensions.clone()).or_default() += 1;
                        put(
                            conn,
                            "members",
                            &[&id.0, &dimensions, &fact.connection_id],
                            &0_u64,
                        )
                        .await?;
                    }
                    if fact.minute.is_none() {
                        add_delta(
                            &mut deltas,
                            "topology-unallocated",
                            dimensions.clone(),
                            &fact.bytes,
                        )?;
                    }
                    add_delta(&mut deltas, "topology", dimensions, &fact.bytes)?;
                    for group in [
                        GroupBy::Process,
                        GroupBy::Source,
                        GroupBy::Target,
                        GroupBy::Protocol,
                        GroupBy::Rule,
                        GroupBy::Exit,
                        GroupBy::Path,
                    ] {
                        let group_key =
                            nyanpasu_traffic::accounting::group_key(&fact.dimensions, &group);
                        add_delta(
                            &mut deltas,
                            group_kind(&group),
                            group_key.clone(),
                            &fact.bytes,
                        )?;
                        if fact.minute.is_none() && matches!(group, GroupBy::Rule) {
                            add_delta(&mut deltas, "rule-unallocated", group_key, &fact.bytes)?;
                        }
                    }
                }
                let mut changed_rankings = BTreeMap::new();
                for ((kind, group_key), delta) in deltas {
                    let previous: Bytes = get(conn, "totals", &[&id.0, kind, &group_key])
                        .await?
                        .unwrap_or_default();
                    let updated = previous.checked_add(&delta)?;
                    if matches!(
                        kind,
                        "process" | "source" | "target" | "protocol" | "rule" | "exit" | "path"
                    ) {
                        if !changed_rankings.contains_key(kind) {
                            let ranked: Vec<(String, Bytes)> =
                                get(conn, "rankings", &[&id.0, kind])
                                    .await?
                                    .unwrap_or_default();
                            let ordered: BTreeMap<_, _> = ranked
                                .into_iter()
                                .map(|(k, b)| ((std::cmp::Reverse(weight(&b)), k), b))
                                .collect();
                            changed_rankings.insert(kind, ordered);
                        }
                        let ranked = changed_rankings.get_mut(kind).expect("ranking initialized");
                        ranked.remove(&(std::cmp::Reverse(weight(&previous)), group_key.clone()));
                        let candidate = (std::cmp::Reverse(weight(&updated)), group_key.clone());
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
                    put(conn, "totals", &[&id.0, kind, &group_key], &updated).await?;
                }
                for (kind, ranked) in changed_rankings {
                    let list: Vec<_> = ranked.into_iter().map(|((_, k), b)| (k, b)).collect();
                    put(conn, "rankings", &[&id.0, kind], &list).await?;
                }
                for (dimensions, delta) in member_deltas {
                    let previous: u64 = get(conn, "path_counts", &[&id.0, &dimensions])
                        .await?
                        .unwrap_or(0);
                    put(
                        conn,
                        "path_counts",
                        &[&id.0, &dimensions],
                        &previous.checked_add(delta).ok_or_else(|| {
                            StoreError::InvalidData("path membership exhausted".into())
                        })?,
                    )
                    .await?;
                }
                put(conn, "sessions", &[&id.0], &batch.session).await?;
                let receipt = CommitReceipt {
                    session_id: id.clone(),
                    position: batch.session.position,
                };
                delete(
                    conn,
                    "receipts",
                    &[&id.0, &key(existing.position.sequence.0)],
                )
                .await?;
                put(conn, "receipts", &[&id.0, &sequence], &receipt).await?;
                Ok(receipt)
            })
        })
        .await
    }
    async fn finish_session(&self, end: SessionEnd) -> TrafficResult<CommitReceipt> {
        self.write(move |conn| Box::pin(async move {
        let mut session = read_session(conn, &end.session_id).await?;
        let digest = nyanpasu_traffic::accounting::digest(&end)?;
        if let Some(detected) = session.ended_at {
            if detected != end.detected_at || session.position.digest != digest { return Err(StoreError::Conflict("session already ended with another result".into())); }
            return Ok(CommitReceipt { session_id: session.id, position: session.position });
        }
        let mut after = (key(0), String::new());
        loop {
            let mut active = rows(conn,"SELECT k3,k4 FROM filters WHERE k0=?1 AND k1='state' AND k2='true' AND (k3>?2 OR (k3=?2 AND k4>?3)) ORDER BY k3,k4 LIMIT 1",vec![text(&session.id.0),text(&after.0),text(&after.1)]).await?;
            let next = active.next().await.map_err(error)?.map(|row| Ok::<_,StoreError>((string(&row,0)?,string(&row,1)?))).transpose()?;
            while active.next().await.map_err(error)?.is_some() {}
            drop(active);
            let Some(next) = next else { break; };
            after = next;
            let mut record: ConnectionRecord = get(conn,"connections", &[&session.id.0,&after.1]).await?.ok_or_else(|| StoreError::Corrupt("orphan active index".into()))?;
            record.status = ConnectionStatus::Closed { detected_at: end.detected_at, reason: end.reason.clone(), final_counters_exact: false };
            write_connection(conn,&record).await?;
        }
        let previous = session.position.sequence;
        session.ended_at = Some(end.detected_at);
        session.freshness = Freshness::Ended;
        session.position = CommittedPosition { sequence: UInt(previous.0.checked_add(1).ok_or_else(|| StoreError::InvalidData("sequence exhausted".into()))?), digest };
        put(conn,"sessions", &[&session.id.0],&session).await?;
        let receipt = CommitReceipt { session_id: session.id, position: session.position };
        delete(conn,"receipts", &[&receipt.session_id.0,&key(previous.0)]).await?;
        put(conn,"receipts", &[&receipt.session_id.0,&key(receipt.position.sequence.0)],&receipt).await?;
        Ok(receipt)
        })).await
    }
    async fn query_connections(&self, query: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        connections_query(&*self.lock().await?, query).await
    }
    async fn query_usage(&self, query: UsageQuery) -> TrafficResult<UsageResult> {
        usage_query(&*self.lock().await?, query).await
    }
    async fn query_topology(&self, query: TopologyQuery) -> TrafficResult<TopologyResult> {
        topology_query(&*self.lock().await?, query).await
    }
    async fn prune(&self, policy: RetentionPolicy) -> TrafficResult<PruneReport> {
        self.write(move |conn| {
            Box::pin(async move {
                let mut sessions = scan(conn, "sessions", &[], "value").await?;
                let mut candidates = Vec::new();
                let mut newest: Option<(UInt, SessionId)> = None;
                while let Some(row) = sessions.next().await.map_err(error)? {
                    let session: SessionRecord = decode(&blob(&row, 0)?)?;
                    if session.host == policy.host
                        && let Some(end) = session.ended_at
                    {
                        if newest.as_ref().is_none_or(|(time, _)| end > *time) {
                            newest = Some((end, session.id.clone()));
                        }
                        candidates.push(session.id);
                    }
                }
                drop(sessions);
                let mut removed = Vec::new();
                for id in candidates {
                    if policy.protected_session.as_ref() == Some(&id)
                        || (policy.keep_last_ended
                            && newest.as_ref().is_some_and(|(_, newest)| *newest == id))
                    {
                        continue;
                    }
                    for (table, _) in TABLES {
                        delete(conn, table, &[&id.0]).await?;
                    }
                    removed.push(id);
                }
                Ok(PruneReport { removed })
            })
        })
        .await
    }
    async fn flush(&self) -> TrafficResult<()> {
        let conn = self.lock().await?;
        // Every write already committed with FULL; flush additionally checkpoints.
        let mut result = conn
            .query("PRAGMA wal_checkpoint", ())
            .await
            .map_err(error)?;
        while result.next().await.map_err(error)?.is_some() {}
        Ok(())
    }
}

fn add_delta<'a>(
    deltas: &mut BTreeMap<(&'a str, String), Bytes>,
    kind: &'a str,
    key: String,
    bytes: &Bytes,
) -> TrafficResult<()> {
    let delta = deltas.entry((kind, key)).or_default();
    *delta = delta.checked_add(bytes)?;
    Ok(())
}

async fn connections_query(
    conn: &Connection,
    query: ConnectionsQuery,
) -> TrafficResult<ConnectionPage> {
    let session = read_session(conn, &query.session_id).await.map_err(|e| {
        if query.cursor.is_some() && matches!(e, StoreError::NotFound) {
            StoreError::InvalidCursor
        } else {
            e
        }
    })?;
    validate_filter(&query.filter)?;
    let digest = nyanpasu_traffic::accounting::digest(&query.filter)?;
    let (watermark, after_seq, after_id) = match &query.cursor {
        Some(cursor) => {
            if cursor.session_id != session.id
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
    let indexed = index_filter(&query.filter)?;
    let mut args = vec![text(&session.id.0)];
    let (table, seq_col, id_col, prefix) = if let Some((kind, value)) = indexed {
        args.extend([text(kind), text(value)]);
        ("filters", "k3", "k4", "k0=?1 AND k1=?2 AND k2=?3")
    } else {
        ("ordering", "k1", "k2", "k0=?1")
    };
    let n = args.len() + 1;
    args.extend([
        text(key(after_seq.0)),
        text(after_id),
        text(key(watermark.0)),
    ]);
    let sql = format!(
        "SELECT {id_col} FROM {table} WHERE {prefix} AND ({seq_col}>?{n} OR ({seq_col}=?{n} AND {id_col}>?{})) AND {seq_col}<=?{} ORDER BY {seq_col},{id_col}",
        n + 1,
        n + 2
    );
    let mut candidates = rows(conn, &sql, args).await?;
    let mut connections = Vec::with_capacity(limit + 1);
    while let Some(row) = candidates.next().await.map_err(error)? {
        let id = string(&row, 0)?;
        let record: ConnectionRecord = get(conn, "connections", &[&session.id.0, &id])
            .await?
            .ok_or_else(|| StoreError::Corrupt("orphan connection index".into()))?;
        if !connection_lifecycle_matches(&record, &query.filter)? {
            continue;
        }
        let mut matched =
            nyanpasu_traffic::accounting::matches_dimensions(&record.dimensions, &query.filter);
        if !matched {
            let mut facts = scan(conn, "facts", &[&session.id.0, &id], "value").await?;
            while let Some(row) = facts.next().await.map_err(error)? {
                let fact: AttributionFact = decode(&blob(&row, 0)?)?;
                if nyanpasu_traffic::accounting::matches_dimensions(&fact.dimensions, &query.filter)
                {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            connections.push(record);
            if connections.len() > limit {
                break;
            }
        }
    }
    let more = connections.len() > limit;
    connections.truncate(limit);
    let next_cursor = if more {
        connections.last().map(|r| ConnectionCursor {
            session_id: session.id.clone(),
            query_digest: digest,
            high_watermark: watermark,
            last_sequence: r.first_observed_sequence,
            last_id: r.id.clone(),
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

async fn scan_facts(
    conn: &Connection,
    session: &SessionRecord,
    filter: &ConnectionFilter,
    scope: &QueryScope,
    topology: bool,
    mut consume: impl FnMut(AttributionFact) -> TrafficResult<()>,
) -> TrafficResult<()> {
    validate_scope(scope)?;
    validate_filter(filter)?;
    // A join against the state index confines live queries to active connections.
    let live = matches!(scope, QueryScope::Live) || filter.status == Some(true);
    let sql = if live {
        "SELECT f.value FROM filters a JOIN facts f ON f.k0=a.k0 AND f.k1=a.k4 WHERE a.k0=?1 AND a.k1='state' AND a.k2='true' ORDER BY f.k1,f.k2,f.k3"
    } else {
        "SELECT value FROM facts WHERE k0=?1 ORDER BY k1,k2,k3"
    };
    let mut facts = rows(conn, sql, vec![text(&session.id.0)]).await?;
    while let Some(row) = facts.next().await.map_err(error)? {
        let fact: AttributionFact = decode(&blob(&row, 0)?)?;
        if !nyanpasu_traffic::accounting::matches_dimensions(&fact.dimensions, filter) {
            continue;
        }
        if filter.status.is_some()
            || filter.started_after.is_some()
            || filter.started_before.is_some()
            || matches!(scope, QueryScope::Live)
        {
            let record: ConnectionRecord =
                get(conn, "connections", &[&session.id.0, &fact.connection_id])
                    .await?
                    .ok_or_else(|| StoreError::Corrupt("orphan attribution fact".into()))?;
            if !connection_lifecycle_matches(&record, filter)?
                || (matches!(scope, QueryScope::Live)
                    && !matches!(record.status, ConnectionStatus::Active))
            {
                continue;
            }
            if topology
                && matches!(scope, QueryScope::Live)
                && path_dimensions(&record.dimensions) != path_dimensions(&fact.dimensions)
            {
                continue;
            }
        }
        if let QueryScope::MinuteWindow { from, until } = scope {
            if fact
                .minute
                .is_some_and(|m| m.0 < from.0 / 60000 || m.0 >= until.0 / 60000)
            {
                continue;
            }
            if fact.minute.is_none()
                && (fact.interval_until < *from
                    || fact.interval_from.is_some_and(|start| start >= *until))
            {
                continue;
            }
        }
        consume(fact)?;
    }
    Ok(())
}
fn usage_result(
    session: SessionRecord,
    total: Bytes,
    unallocated: Bytes,
    ranked: Vec<(String, Bytes)>,
    other: Bytes,
    minutes: BTreeMap<UInt, Bytes>,
) -> UsageResult {
    UsageResult {
        meta: QueryMeta {
            session,
            cross_page_snapshot: true,
        },
        total,
        time_unallocated: unallocated,
        groups: ranked
            .into_iter()
            .map(|(key, bytes)| UsageGroup {
                key,
                bytes,
                current_rate: None,
            })
            .collect(),
        other,
        minutes: minutes
            .into_iter()
            .map(|(minute, bytes)| MinuteBucket { minute, bytes })
            .collect(),
        current_rate: None,
    }
}
fn ranked_groups(
    groups: BTreeMap<String, Bytes>,
    limit: u16,
) -> TrafficResult<(Vec<(String, Bytes)>, Bytes)> {
    let mut ranked: Vec<_> = groups.into_iter().collect();
    ranked.sort_by(|(ka, a), (kb, b)| weight(b).cmp(&weight(a)).then_with(|| ka.cmp(kb)));
    let limit = usize::from(limit).clamp(1, 500);
    let mut other = Bytes::default();
    for (_, bytes) in ranked.iter().skip(limit) {
        other = other.checked_add(bytes)?;
    }
    ranked.truncate(limit);
    Ok((ranked, other))
}
async fn usage_query(conn: &Connection, query: UsageQuery) -> TrafficResult<UsageResult> {
    let session = read_session(conn, &query.session_id).await?;
    if matches!(query.scope, QueryScope::Session) && query.filter == ConnectionFilter::default() {
        let mut ranked = Vec::<(String, Bytes)>::new();
        let mut other = Bytes::default();
        if let Some(group) = query.group_by {
            ranked = get(conn, "rankings", &[&session.id.0, group_kind(&group)])
                .await?
                .unwrap_or_default();
            ranked.truncate(usize::from(query.limit).clamp(1, 500));
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
                            StoreError::Corrupt("ranking upload exceeds total".into())
                        })?,
                ),
                download: UInt(
                    session
                        .attributed_bytes
                        .download
                        .0
                        .checked_sub(listed.download.0)
                        .ok_or_else(|| {
                            StoreError::Corrupt("ranking download exceeds total".into())
                        })?,
                ),
            };
        }
        return Ok(usage_result(
            session.clone(),
            session.attributed_bytes,
            session.time_unallocated,
            ranked,
            other,
            BTreeMap::new(),
        ));
    }
    if matches!(query.scope, QueryScope::Session)
        && matches!(query.group_by, None | Some(GroupBy::Rule))
        && let Some(rule) = &query.filter.rule
    {
        let mut remaining = query.filter.clone();
        remaining.rule = None;
        if remaining == ConnectionFilter::default() {
            let mut total = Bytes::default();
            let mut unallocated = Bytes::default();
            let mut ranked = Vec::new();
            let mut other = Bytes::default();
            let limit = usize::from(query.limit).clamp(1, 500);
            for (kind, target) in [("rule", &mut total), ("rule-unallocated", &mut unallocated)] {
                let mut groups = scan(conn, "totals", &[&session.id.0, kind], "k2,value").await?;
                while let Some(row) = groups.next().await.map_err(error)? {
                    let group_key = string(&row, 0)?;
                    let key: RuleKey = serde_json::from_str(&group_key)
                        .map_err(|e| StoreError::Corrupt(e.to_string()))?;
                    if key.kind == rule.kind
                        && key.payload == rule.payload
                        && rule
                            .context
                            .as_ref()
                            .is_none_or(|ctx| Some(ctx) == key.context.as_ref())
                    {
                        let bytes: Bytes = decode(&blob(&row, 1)?)?;
                        *target = target.checked_add(&bytes)?;
                        if kind == "rule" && query.group_by.is_some() {
                            ranked.push((group_key, bytes));
                            ranked.sort_by(|(ka, a), (kb, b)| {
                                weight(b).cmp(&weight(a)).then_with(|| ka.cmp(kb))
                            });
                            if ranked.len() > limit {
                                other =
                                    other.checked_add(&ranked.pop().expect("rank overflow").1)?;
                            }
                        }
                    }
                }
            }
            return Ok(usage_result(
                session,
                total,
                unallocated,
                ranked,
                other,
                BTreeMap::new(),
            ));
        }
    }
    let mut total = Bytes::default();
    let mut unallocated = Bytes::default();
    let mut groups = BTreeMap::new();
    let mut minutes = BTreeMap::new();
    scan_facts(conn, &session, &query.filter, &query.scope, false, |fact| {
        if fact.minute.is_none() {
            unallocated = unallocated.checked_add(&fact.bytes)?;
        }
        if matches!(query.scope, QueryScope::MinuteWindow { .. }) && fact.minute.is_none() {
            return Ok(());
        }
        total = total.checked_add(&fact.bytes)?;
        if let Some(group) = &query.group_by {
            bounded_add(
                &mut groups,
                nyanpasu_traffic::accounting::group_key(&fact.dimensions, group),
                &fact.bytes,
            )?;
        }
        if matches!(query.scope, QueryScope::MinuteWindow { .. })
            && let Some(minute) = fact.minute
        {
            bounded_add(&mut minutes, minute, &fact.bytes)?;
        }
        Ok(())
    })
    .await?;
    let (ranked, other) = ranked_groups(groups, query.limit)?;
    Ok(usage_result(
        session,
        total,
        unallocated,
        ranked,
        other,
        minutes,
    ))
}

async fn topology_query(conn: &Connection, query: TopologyQuery) -> TrafficResult<TopologyResult> {
    let session = read_session(conn, &query.session_id).await?;
    let fast = matches!(query.scope, QueryScope::Session)
        && query.filter.status.is_none()
        && query.filter.started_after.is_none()
        && query.filter.started_before.is_none()
        && query.filter.target.is_none()
        && query.filter.protocol.is_none()
        && query.filter.source.is_none();
    let limit = usize::from(query.limit).clamp(1, 500);
    let mut unallocated = Bytes::default();
    let mut other = Bytes::default();
    let mut ranked = Vec::<(Dimensions, Bytes)>::new();
    if fast {
        let mut groups = scan(conn, "totals", &[&session.id.0, "topology"], "k2,value").await?;
        while let Some(row) = groups.next().await.map_err(error)? {
            let dimension_key = string(&row, 0)?;
            let dimensions: Dimensions = serde_json::from_str(&dimension_key)
                .map_err(|e| StoreError::Corrupt(e.to_string()))?;
            if !nyanpasu_traffic::accounting::matches_dimensions(&dimensions, &query.filter) {
                continue;
            }
            if let Some(bytes) = get::<Bytes>(
                conn,
                "totals",
                &[&session.id.0, "topology-unallocated", &dimension_key],
            )
            .await?
            {
                unallocated = unallocated.checked_add(&bytes)?;
            }
            ranked.push((dimensions, decode(&blob(&row, 1)?)?));
            ranked.sort_by(|(da, a), (db, b)| weight(b).cmp(&weight(a)).then_with(|| da.cmp(db)));
            if ranked.len() > limit {
                other = other.checked_add(&ranked.pop().expect("rank overflow").1)?;
            }
        }
    } else {
        let mut groups = BTreeMap::new();
        scan_facts(conn, &session, &query.filter, &query.scope, true, |fact| {
            if fact.minute.is_none() {
                unallocated = unallocated.checked_add(&fact.bytes)?;
            }
            if matches!(query.scope, QueryScope::MinuteWindow { .. }) && fact.minute.is_none() {
                return Ok(());
            }
            bounded_add(&mut groups, path_dimensions(&fact.dimensions), &fact.bytes)
        })
        .await?;
        ranked = groups.into_iter().collect();
        ranked.sort_by(|(da, a), (db, b)| weight(b).cmp(&weight(a)).then_with(|| da.cmp(db)));
        for (_, bytes) in ranked.iter().skip(limit) {
            other = other.checked_add(bytes)?;
        }
        ranked.truncate(limit);
    }
    let mut counts: BTreeMap<Dimensions, (String, u64)> = ranked
        .iter()
        .map(|(d, _)| (d.clone(), (String::new(), 0)))
        .collect();
    if !fast {
        scan_facts(conn, &session, &query.filter, &query.scope, true, |fact| {
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
        })
        .await?;
    }
    let mut paths = Vec::new();
    for (dimensions, bytes) in ranked {
        let memberships = if fast {
            let dimension_key = serde_json::to_string(&dimensions)
                .map_err(|e| StoreError::InvalidData(e.to_string()))?;
            get::<u64>(conn, "path_counts", &[&session.id.0, &dimension_key])
                .await?
                .unwrap_or(0)
        } else {
            counts.get(&dimensions).map(|(_, n)| *n).unwrap_or(0)
        };
        paths.push(TopologyPath {
            dimensions,
            bytes,
            memberships: UInt(memberships),
            current_rate: None,
        });
    }
    let (nodes, edges) = nyanpasu_traffic::topology::project(paths.clone())?;
    Ok(TopologyResult {
        meta: QueryMeta {
            session,
            cross_page_snapshot: true,
        },
        paths,
        nodes,
        edges,
        other,
        time_unallocated: unallocated,
    })
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

#[cfg(test)]
mod tests {
    #[test]
    fn unsigned_order_keys_cover_full_range_and_ties() {
        let values = [0, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX];
        let keys: Vec<_> = values.into_iter().map(super::key).collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(keys.iter().all(|key| key.len() == 20));
        assert!((keys[3].clone(), "a") < (keys[3].clone(), "b"));
    }

    #[tokio::test]
    async fn dropped_transaction_cleanup_precedes_cached_reads() {
        let db = turso::Builder::new_local(":memory:").build().await.unwrap();
        let mut conn = db.connect().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER)", ())
            .await
            .unwrap();
        {
            let tx = turso::transaction::Transaction::new(
                &mut conn,
                turso::transaction::TransactionBehavior::Immediate,
            )
            .await
            .unwrap();
            tx.execute("INSERT INTO test VALUES (1)", ()).await.unwrap();
        }
        super::cleanup(&conn).await.unwrap();
        let mut statement = conn
            .prepare_cached("SELECT COUNT(*) FROM test")
            .await
            .unwrap();
        let mut rows = statement.query(()).await.unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get_value(0).unwrap(),
            turso::Value::Integer(0)
        );
        assert!(rows.next().await.unwrap().is_none());
    }
}
