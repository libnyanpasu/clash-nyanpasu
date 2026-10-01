use crate::{
    accounting::{FlushBatch, Prune},
    model::*,
    ports::{Flushed, TrafficStore},
    query::Usage,
};
use ::redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, Table, TableDefinition,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    fs,
    ops::Bound,
    path::Path,
    sync::{Arc, Mutex},
};

/// Statistics are disposable: a file written by another schema is wiped, never migrated.
const SCHEMA_VERSION: u64 = 2;

const VERSION_KEY: &str = "version";
const META_KEY: &str = "meta";

const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("schema");
/// JSON `SessionMeta` under `META_KEY`.
const SESSION: TableDefinition<&str, &[u8]> = TableDefinition::new("session");
/// Connection id -> JSON `ActiveConnection`.
const ACTIVE: TableDefinition<&str, &[u8]> = TableDefinition::new("active");
/// (closed_at, connection id) -> JSON `ClosedConnection`.
const CLOSED: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("closed");
/// Tuple id -> JSON `Dimensions`.
const TUPLES: TableDefinition<u64, &[u8]> = TableDefinition::new("tuples");
/// (minute bucket, tuple id) -> (upload, download, connections).
const MINUTE_USAGE: TableDefinition<(u32, u64), (u64, u64, u64)> =
    TableDefinition::new("minute_usage");
/// (hour bucket, tuple id) -> (upload, download, connections).
const HOUR_USAGE: TableDefinition<(u32, u64), (u64, u64, u64)> = TableDefinition::new("hour_usage");

/// The dimension combinations of the usage tables, interned: a combination appears in every
/// bucket it was active in, and a numeric key keeps the rows small and the per-combination sums
/// cheap.
#[derive(Default)]
struct Tuples {
    by_id: HashMap<u64, Arc<Dimensions>>,
    ids: HashMap<Arc<Dimensions>, u64>,
    /// Ids are never reused while the store is open.
    next: u64,
}

impl Tuples {
    fn insert(&mut self, id: u64, dimensions: Arc<Dimensions>) {
        self.ids.insert(Arc::clone(&dimensions), id);
        self.by_id.insert(id, dimensions);
        self.next = self.next.max(id + 1);
    }

    fn remove(&mut self, id: u64) {
        if let Some(dimensions) = self.by_id.remove(&id) {
            self.ids.remove(&dimensions);
        }
    }
}

/// Hands out tuple ids during one flush without touching the cache, which only learns about the
/// new ones once the transaction has committed.
struct Interner<'a> {
    known: &'a Tuples,
    added: HashMap<Arc<Dimensions>, u64>,
    next: u64,
}

impl Interner<'_> {
    fn id(
        &mut self,
        table: &mut Table<'_, u64, &[u8]>,
        dimensions: &Arc<Dimensions>,
    ) -> TrafficResult<u64> {
        if let Some(&id) = self
            .known
            .ids
            .get(dimensions)
            .or_else(|| self.added.get(dimensions))
        {
            return Ok(id);
        }
        let id = self.next;
        self.next += 1;
        table
            .insert(id, encode(&**dimensions)?.as_slice())
            .map_err(storage)?;
        self.added.insert(Arc::clone(dimensions), id);
        Ok(id)
    }
}

pub struct RedbTrafficStore {
    db: Database,
    // The cache mirrors the `tuples` table. It is an implementation detail of this adapter, not
    // shared state: the actor is the only caller, so the lock is never contended; it exists
    // because the port takes `&self`, and it is held from a flush's first read of the cache until
    // the cache reflects the commit.
    tuples: Mutex<Tuples>,
}

impl RedbTrafficStore {
    /// Opens the database at `path`, replacing it when it is corrupted or from another schema.
    pub fn open(path: &Path) -> TrafficResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        if let Some(db) = Self::init(path)? {
            return Self::new(db);
        }
        fs::remove_file(path).map_err(storage)?;
        Self::init(path)?
            .map(Self::new)
            .ok_or_else(|| TrafficError::Storage("fresh database was rejected".into()))?
    }

    fn new(db: Database) -> TrafficResult<Self> {
        let tuples = Self::load_tuples(&db)?;
        Ok(Self {
            db,
            tuples: Mutex::new(tuples),
        })
    }

    fn load_tuples(db: &Database) -> TrafficResult<Tuples> {
        let txn = db.begin_read().map_err(storage)?;
        let table = txn.open_table(TUPLES).map_err(storage)?;
        let mut tuples = Tuples::default();
        for entry in table.iter().map_err(storage)? {
            let (id, value) = entry.map_err(storage)?;
            tuples.insert(id.value(), Arc::new(decode(value.value())?));
        }
        Ok(tuples)
    }

    /// `None` when the file is unusable and may be deleted; any other failure is an error.
    fn init(path: &Path) -> TrafficResult<Option<Database>> {
        match Self::prepare(path) {
            Ok(db) => Ok(db),
            Err(e) if is_disposable(&e) => Ok(None),
            Err(e) => Err(storage(e)),
        }
    }

    fn prepare(path: &Path) -> Result<Option<Database>, ::redb::Error> {
        let db = Database::create(path)?;
        let txn = db.begin_write()?;
        {
            let mut schema = txn.open_table(SCHEMA)?;
            let stored = schema.get(VERSION_KEY)?.map(|v| v.value());
            match stored {
                Some(SCHEMA_VERSION) => {}
                Some(_) => return Ok(None),
                None => {
                    schema.insert(VERSION_KEY, SCHEMA_VERSION)?;
                }
            }
            txn.open_table(SESSION)?;
            txn.open_table(ACTIVE)?;
            txn.open_table(CLOSED)?;
            txn.open_table(TUPLES)?;
            txn.open_table(MINUTE_USAGE)?;
            txn.open_table(HOUR_USAGE)?;
        }
        txn.commit()?;
        Ok(Some(db))
    }

    fn tuples(&self) -> std::sync::MutexGuard<'_, Tuples> {
        self.tuples
            .lock()
            .expect("the tuple cache lock is poisoned")
    }
}

impl TrafficStore for RedbTrafficStore {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>> {
        let txn = self.db.begin_read().map_err(storage)?;
        let session = txn.open_table(SESSION).map_err(storage)?;
        let Some(meta) = session.get(META_KEY).map_err(storage)? else {
            return Ok(None);
        };
        let meta: SessionMeta = decode(meta.value())?;

        let table = txn.open_table(ACTIVE).map_err(storage)?;
        let mut active = Vec::new();
        for entry in table.iter().map_err(storage)? {
            let (_, value) = entry.map_err(storage)?;
            active.push(decode(value.value())?);
        }
        Ok(Some((meta, active)))
    }

    fn flush(&self, batch: &FlushBatch) -> TrafficResult<Flushed> {
        let mut tuples = self.tuples();
        let mut interner = Interner {
            known: &tuples,
            added: HashMap::new(),
            next: tuples.next,
        };

        let txn = self.db.begin_write().map_err(storage)?;
        let hours_pruned;
        {
            txn.open_table(SESSION)
                .map_err(storage)?
                .insert(META_KEY, encode(&batch.meta)?.as_slice())
                .map_err(storage)?;

            let mut active = txn.open_table(ACTIVE).map_err(storage)?;
            for id in &batch.removed {
                active.remove(id.as_str()).map_err(storage)?;
            }
            for conn in &batch.active {
                active
                    .insert(conn.id.as_str(), encode(conn)?.as_slice())
                    .map_err(storage)?;
            }

            let mut tuple_table = txn.open_table(TUPLES).map_err(storage)?;
            for (mut usage, rows) in [
                (
                    txn.open_table(MINUTE_USAGE).map_err(storage)?,
                    &batch.minutes,
                ),
                (txn.open_table(HOUR_USAGE).map_err(storage)?, &batch.hours),
            ] {
                for (bucket, dimensions, increment) in rows {
                    let id = interner.id(&mut tuple_table, dimensions)?;
                    add_usage(&mut usage, *bucket, id, *increment)?;
                }
            }

            let mut closed = txn.open_table(CLOSED).map_err(storage)?;
            for conn in &batch.closed {
                closed
                    .insert(
                        (closed_key(conn.closed_at), conn.id.as_str()),
                        encode(conn)?.as_slice(),
                    )
                    .map_err(storage)?;
            }

            drop(closed);
            hours_pruned = apply_prune(&txn, &batch.prune)?;
        }
        let (added, next) = (interner.added, interner.next);
        txn.commit().map_err(storage)?;

        for (dimensions, id) in added {
            tuples.insert(id, dimensions);
        }
        tuples.next = tuples.next.max(next);
        Ok(Flushed { hours_pruned })
    }

    fn closed_connections(
        &self,
        before: Option<&ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage> {
        let limit = limit.clamp(1, MAX_CLOSED_PAGE);
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(CLOSED).map_err(storage)?;
        let upper = before.map_or(Bound::Unbounded, |c| {
            Bound::Excluded((closed_key(c.closed_at), c.id.as_str()))
        });

        let mut connections: Vec<ClosedConnection> = Vec::new();
        let newest_first = table
            .range::<(u64, &str)>((Bound::Unbounded, upper))
            .map_err(storage)?
            .rev()
            .take(limit);
        for entry in newest_first {
            let (_, value) = entry.map_err(storage)?;
            connections.push(decode(value.value())?);
        }

        let next = if connections.len() == limit {
            connections.last().map(|c| ClosedCursor {
                closed_at: c.closed_at,
                id: c.id.clone(),
            })
        } else {
            None
        };
        Ok(ClosedPage { connections, next })
    }

    fn closed_count(&self) -> TrafficResult<u64> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(CLOSED).map_err(storage)?;
        table.len().map_err(storage)
    }

    fn usage(&self, tier: Tier, from: Option<u32>) -> TrafficResult<Vec<(Arc<Dimensions>, Usage)>> {
        let txn = self.db.begin_read().map_err(storage)?;
        let sums = match tier {
            Tier::Minute => sum_by_tuple(&txn.open_table(MINUTE_USAGE).map_err(storage)?, from)?,
            Tier::Hour => sum_by_tuple(&txn.open_table(HOUR_USAGE).map_err(storage)?, from)?,
        };

        let tuples = self.tuples();
        sums.into_iter()
            .map(|(id, usage)| {
                tuples
                    .by_id
                    .get(&id)
                    .map(|dimensions| (Arc::clone(dimensions), usage))
                    .ok_or_else(|| {
                        TrafficError::Storage(format!("usage refers to the unknown tuple {id}"))
                    })
            })
            .collect()
    }

    fn collect_tuples(&self, keep: &[Arc<Dimensions>]) -> TrafficResult<u64> {
        let mut tuples = self.tuples();
        let mut doomed: HashSet<u64> = tuples.by_id.keys().copied().collect();
        for dimensions in keep {
            if let Some(id) = tuples.ids.get(dimensions) {
                doomed.remove(id);
            }
        }

        let txn = self.db.begin_read().map_err(storage)?;
        for table in [MINUTE_USAGE, HOUR_USAGE] {
            let table = txn.open_table(table).map_err(storage)?;
            for entry in table.iter().map_err(storage)? {
                let (key, _) = entry.map_err(storage)?;
                doomed.remove(&key.value().1);
            }
        }
        if doomed.is_empty() {
            return Ok(0);
        }

        let txn = self.db.begin_write().map_err(storage)?;
        {
            let mut table = txn.open_table(TUPLES).map_err(storage)?;
            for id in &doomed {
                table.remove(*id).map_err(storage)?;
            }
        }
        txn.commit().map_err(storage)?;

        for id in &doomed {
            tuples.remove(*id);
        }
        Ok(doomed.len() as u64)
    }
}

fn add_usage(
    table: &mut Table<'_, (u32, u64), (u64, u64, u64)>,
    bucket: u32,
    tuple: u64,
    increment: Usage,
) -> TrafficResult<()> {
    let key = (bucket, tuple);
    let current = table
        .get(key)
        .map_err(storage)?
        .map(|v| usage_of(v.value()))
        .unwrap_or_default();
    let next = current.saturating_add(increment);
    table
        .insert(
            key,
            (next.bytes.upload, next.bytes.download, next.connections),
        )
        .map_err(storage)?;
    Ok(())
}

/// Deletes by time prefix; a bucket exactly at its cutoff is kept. Tells whether hour rows went.
fn apply_prune(txn: &WriteTransaction, prune: &Prune) -> TrafficResult<bool> {
    txn.open_table(MINUTE_USAGE)
        .map_err(storage)?
        .retain_in(..(prune.minutes_before, 0), |_, _| false)
        .map_err(storage)?;
    let mut hours_pruned = false;
    if let Some(before) = prune.hours_before {
        let mut hours = txn.open_table(HOUR_USAGE).map_err(storage)?;
        hours_pruned = hours
            .range::<(u32, u64)>(..(before, 0))
            .map_err(storage)?
            .next()
            .is_some();
        hours
            .retain_in(..(before, 0), |_, _| false)
            .map_err(storage)?;
    }
    if let Some(before) = prune.closed_before {
        txn.open_table(CLOSED)
            .map_err(storage)?
            .retain_in(..(closed_key(before), ""), |_, _| false)
            .map_err(storage)?;
    }
    Ok(hours_pruned)
}

/// The usage of each tuple in the buckets from `from` on.
fn sum_by_tuple(
    table: &impl ReadableTable<(u32, u64), (u64, u64, u64)>,
    from: Option<u32>,
) -> TrafficResult<HashMap<u64, Usage>> {
    let mut sums: HashMap<u64, Usage> = HashMap::new();
    for entry in table
        .range::<(u32, u64)>((from.unwrap_or(0), 0)..)
        .map_err(storage)?
    {
        let (key, value) = entry.map_err(storage)?;
        let slot = sums.entry(key.value().1).or_default();
        *slot = slot.saturating_add(usage_of(value.value()));
    }
    Ok(sums)
}

fn storage(e: impl Display) -> TrafficError {
    TrafficError::Storage(e.to_string())
}

fn is_disposable(e: &::redb::Error) -> bool {
    use ::redb::Error;
    match e {
        Error::Corrupted(_)
        | Error::UpgradeRequired(_)
        | Error::TableTypeMismatch { .. }
        | Error::TableIsMultimap(_)
        | Error::TableIsNotMultimap(_)
        | Error::TypeDefinitionChanged { .. } => true,
        // redb reports a file that is not a redb database (bad magic number) as invalid data.
        Error::Io(e) => e.kind() == std::io::ErrorKind::InvalidData,
        _ => false,
    }
}

fn encode<T: Serialize>(value: &T) -> TrafficResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(storage)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> TrafficResult<T> {
    serde_json::from_slice(bytes).map_err(storage)
}

fn usage_of((upload, download, connections): (u64, u64, u64)) -> Usage {
    Usage {
        bytes: Bytes { upload, download },
        connections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{ReportRequest, report};
    use tempfile::TempDir;

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

    fn bytes(upload: u64, download: u64) -> Bytes {
        Bytes { upload, download }
    }

    fn usage(upload: u64, download: u64, connections: u64) -> Usage {
        Usage {
            bytes: bytes(upload, download),
            connections,
        }
    }

    fn meta(last_sample_at: i64) -> SessionMeta {
        SessionMeta {
            last_sample_at: Some(last_sample_at),
            instance_id: Some("core-1".into()),
            global_counters: Some(bytes(50, 60)),
        }
    }

    fn active(id: &str) -> ActiveConnection {
        ActiveConnection {
            id: id.into(),
            started_at: 900,
            first_seen_at: 1_100,
            counters: bytes(10, 20),
            dimensions: dims("curl"),
            minutes: [(7, bytes(1, 2)), (8, bytes(2, 2))].into(),
            hours: [(0, bytes(3, 4))].into(),
        }
    }

    fn closed(id: &str, closed_at: i64) -> ClosedConnection {
        ClosedConnection {
            id: id.into(),
            started_at: 900,
            first_seen_at: 1_100,
            closed_at,
            bytes: bytes(1, 2),
            dimensions: dims("curl"),
        }
    }

    fn batch() -> FlushBatch {
        FlushBatch {
            meta: meta(2_000),
            active: Vec::new(),
            removed: Vec::new(),
            minutes: Vec::new(),
            hours: Vec::new(),
            closed: Vec::new(),
            prune: Prune::default(),
        }
    }

    type Row = (u32, Arc<Dimensions>, Usage);

    fn row(bucket: u32, process: &str, usage: Usage) -> Row {
        (bucket, Arc::new(dims(process)), usage)
    }

    fn db_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("traffic").join("traffic.redb")
    }

    fn open(dir: &TempDir) -> RedbTrafficStore {
        RedbTrafficStore::open(&db_path(dir)).unwrap()
    }

    /// (process, usage) of every stored combination, ordered.
    fn usage_of_tier(
        store: &RedbTrafficStore,
        tier: Tier,
        from: Option<u32>,
    ) -> Vec<(String, Usage)> {
        let mut rows: Vec<_> = store
            .usage(tier, from)
            .unwrap()
            .into_iter()
            .map(|(d, u)| (d.process.clone(), u))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    fn closed_ids(store: &RedbTrafficStore) -> Vec<String> {
        store
            .closed_connections(None, usize::MAX)
            .unwrap()
            .connections
            .into_iter()
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn flush_then_reopen_loads_the_session() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        assert_eq!(store.load().unwrap(), None);

        let batch = FlushBatch {
            active: vec![active("a")],
            ..batch()
        };
        store.flush(&batch).unwrap();
        drop(store);

        let (loaded, active) = open(&dir).load().unwrap().unwrap();
        assert_eq!(loaded, batch.meta);
        assert_eq!(active, batch.active);
    }

    #[test]
    fn flush_only_touches_the_named_active_connections() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                active: vec![active("a"), active("b"), active("c")],
                ..batch()
            })
            .unwrap();

        let mut changed = active("b");
        changed.counters = bytes(99, 99);
        store
            .flush(&FlushBatch {
                active: vec![changed.clone()],
                removed: vec!["a".into()],
                ..batch()
            })
            .unwrap();

        let (_, mut stored) = store.load().unwrap().unwrap();
        stored.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(stored, [changed, active("c")]);
    }

    #[test]
    fn a_reused_id_is_removed_before_it_is_upserted() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                active: vec![active("a")],
                ..batch()
            })
            .unwrap();

        let mut reborn = active("a");
        reborn.first_seen_at = 5_000;
        store
            .flush(&FlushBatch {
                active: vec![reborn.clone()],
                removed: vec!["a".into()],
                ..batch()
            })
            .unwrap();
        assert_eq!(store.load().unwrap().unwrap().1, [reborn]);

        // Removing what was never stored is harmless.
        store
            .flush(&FlushBatch {
                removed: vec!["never".into()],
                ..batch()
            })
            .unwrap();
    }

    #[test]
    fn usage_adds_up_per_combination_and_survives_a_reopen() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(1, "curl", usage(10, 20, 1)),
                    row(2, "curl", usage(1, 2, 0)),
                    row(1, "firefox", usage(5, 5, 1)),
                ],
                minutes: vec![row(61, "curl", usage(3, 3, 1))],
                ..batch()
            })
            .unwrap();
        store
            .flush(&FlushBatch {
                hours: vec![row(1, "curl", usage(100, 200, 1))],
                ..batch()
            })
            .unwrap();

        let check = |store: &RedbTrafficStore| {
            assert_eq!(
                usage_of_tier(store, Tier::Hour, None),
                [
                    ("curl".to_owned(), usage(111, 222, 2)),
                    ("firefox".to_owned(), usage(5, 5, 1))
                ]
            );
            // The start is inclusive and cuts whole buckets.
            assert_eq!(
                usage_of_tier(store, Tier::Hour, Some(2)),
                [("curl".to_owned(), usage(1, 2, 0))]
            );
            assert_eq!(usage_of_tier(store, Tier::Hour, Some(3)), []);
            // The tiers are separate tables.
            assert_eq!(
                usage_of_tier(store, Tier::Minute, None),
                [("curl".to_owned(), usage(3, 3, 1))]
            );
        };
        check(&store);
        drop(store);
        check(&open(&dir));
    }

    #[test]
    fn equal_dimensions_share_one_tuple() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(1, "curl", usage(1, 1, 0)),
                    row(2, "curl", usage(1, 1, 0)),
                ],
                minutes: vec![row(5, "curl", usage(1, 1, 0))],
                ..batch()
            })
            .unwrap();
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(3, "curl", usage(1, 1, 0)),
                    row(3, "wget", usage(1, 1, 0)),
                ],
                ..batch()
            })
            .unwrap();

        assert_eq!(store.tuples().by_id.len(), 2);
        drop(store);
        // The ids come back from the table and keep growing past the stored ones.
        let store = open(&dir);
        assert_eq!(store.tuples().by_id.len(), 2);
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(4, "curl", usage(1, 1, 0)),
                    row(4, "lftp", usage(1, 1, 0)),
                ],
                ..batch()
            })
            .unwrap();
        assert_eq!(store.tuples().by_id.len(), 3);
        assert_eq!(
            usage_of_tier(&store, Tier::Hour, None),
            [
                ("curl".to_owned(), usage(4, 4, 0)),
                ("lftp".to_owned(), usage(1, 1, 0)),
                ("wget".to_owned(), usage(1, 1, 0)),
            ]
        );
    }

    #[test]
    fn usage_saturates() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let huge = FlushBatch {
            hours: vec![row(1, "curl", usage(u64::MAX, 0, u64::MAX))],
            ..batch()
        };
        store.flush(&huge).unwrap();
        store.flush(&huge).unwrap();

        assert_eq!(
            usage_of_tier(&store, Tier::Hour, None),
            [("curl".to_owned(), usage(u64::MAX, 0, u64::MAX))]
        );
    }

    #[test]
    fn prune_deletes_before_each_cutoff_and_keeps_the_cutoff_itself() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let rows = || -> Vec<Row> {
            (5..=7)
                .map(|b| row(b, &format!("p{b}"), usage(1, 1, 1)))
                .collect()
        };
        store
            .flush(&FlushBatch {
                hours: rows(),
                minutes: rows(),
                closed: vec![closed("a", 100), closed("b", 200), closed("c", 300)],
                ..batch()
            })
            .unwrap();

        // Rows added by the same batch are pruned too.
        let flushed = store
            .flush(&FlushBatch {
                hours: vec![row(3, "late", usage(1, 1, 1))],
                minutes: vec![row(3, "late", usage(1, 1, 1))],
                prune: Prune {
                    minutes_before: 6,
                    hours_before: Some(7),
                    closed_before: Some(200),
                },
                ..batch()
            })
            .unwrap();
        assert!(flushed.hours_pruned);

        let processes = |tier| -> Vec<String> {
            usage_of_tier(&store, tier, None)
                .into_iter()
                .map(|(p, _)| p)
                .collect()
        };
        assert_eq!(processes(Tier::Minute), ["p6", "p7"]);
        assert_eq!(processes(Tier::Hour), ["p7"]);
        assert_eq!(closed_ids(&store), ["c", "b"]);
        assert_eq!(store.closed_count().unwrap(), 2);
    }

    #[test]
    fn prune_without_cutoffs_keeps_everything() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let flushed = store
            .flush(&FlushBatch {
                hours: vec![row(0, "old", usage(1, 1, 1))],
                minutes: vec![row(0, "old", usage(1, 1, 1))],
                closed: vec![closed("a", 0), closed("b", -5)],
                prune: Prune {
                    minutes_before: 0,
                    hours_before: None,
                    closed_before: None,
                },
                ..batch()
            })
            .unwrap();
        assert!(!flushed.hours_pruned);

        assert_eq!(usage_of_tier(&store, Tier::Hour, None).len(), 1);
        assert_eq!(usage_of_tier(&store, Tier::Minute, None).len(), 1);
        // Both close times land on the first key; the id breaks the tie.
        assert_eq!(closed_ids(&store), ["b", "a"]);
    }

    #[test]
    fn collect_tuples_keeps_the_referenced_and_the_requested() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(1, "orphan", usage(1, 1, 0)),
                    row(1, "kept", usage(1, 1, 0)),
                    row(1, "other", usage(1, 1, 0)),
                    row(9, "hourly", usage(1, 1, 0)),
                ],
                minutes: vec![row(9, "minutely", usage(1, 1, 0))],
                ..batch()
            })
            .unwrap();
        // Nothing is unreferenced yet.
        assert_eq!(store.collect_tuples(&[]).unwrap(), 0);

        let flushed = store
            .flush(&FlushBatch {
                prune: Prune {
                    minutes_before: 0,
                    hours_before: Some(2),
                    closed_before: None,
                },
                ..batch()
            })
            .unwrap();
        assert!(flushed.hours_pruned);
        // A cutoff that no hour row lies before deletes nothing.
        let flushed = store
            .flush(&FlushBatch {
                prune: Prune {
                    minutes_before: 0,
                    hours_before: Some(2),
                    closed_before: None,
                },
                ..batch()
            })
            .unwrap();
        assert!(!flushed.hours_pruned);
        let keep = [Arc::new(dims("kept"))];
        assert_eq!(store.collect_tuples(&keep).unwrap(), 2);
        assert_eq!(store.tuples().by_id.len(), 3);
        assert_eq!(store.collect_tuples(&keep).unwrap(), 0);

        // Still referenced by a usage row: the hour table and the minute table each count.
        assert_eq!(
            usage_of_tier(&store, Tier::Hour, None),
            [("hourly".to_owned(), usage(1, 1, 0))]
        );
        assert_eq!(usage_of_tier(&store, Tier::Minute, None).len(), 1);

        // A collected combination can come back under a new id.
        store
            .flush(&FlushBatch {
                hours: vec![
                    row(10, "orphan", usage(2, 2, 0)),
                    row(10, "kept", usage(3, 3, 0)),
                ],
                ..batch()
            })
            .unwrap();
        drop(store);
        let store = open(&dir);
        assert_eq!(
            usage_of_tier(&store, Tier::Hour, None),
            [
                ("hourly".to_owned(), usage(1, 1, 0)),
                ("kept".to_owned(), usage(3, 3, 0)),
                ("orphan".to_owned(), usage(2, 2, 0)),
            ]
        );
        // Everything is referenced again.
        assert_eq!(store.collect_tuples(&[]).unwrap(), 0);
    }

    #[test]
    fn closed_connections_page_newest_first() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                closed: vec![
                    closed("a", 10),
                    closed("b", 20),
                    closed("c", 20),
                    closed("d", 30),
                    closed("e", 40),
                ],
                ..batch()
            })
            .unwrap();

        let first = store.closed_connections(None, 2).unwrap();
        let ids = |page: &ClosedPage| {
            page.connections
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ["e", "d"]);
        assert_eq!(
            first.next,
            Some(ClosedCursor {
                closed_at: 30,
                id: "d".into()
            })
        );

        // Ties on closed_at fall back to the id, descending.
        let second = store.closed_connections(first.next.as_ref(), 2).unwrap();
        assert_eq!(ids(&second), ["c", "b"]);

        let third = store.closed_connections(second.next.as_ref(), 2).unwrap();
        assert_eq!(ids(&third), ["a"]);
        assert_eq!(third.next, None);
    }

    #[test]
    fn merged_pages_walk_stored_and_pending_connections_once() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                closed: vec![closed("b", 20), closed("d", 40), closed("f", 60)],
                ..batch()
            })
            .unwrap();
        let pending = [
            closed("g", 70),
            closed("e", 50),
            closed("c", 30),
            closed("a", 10),
        ];

        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let stored = store.closed_connections(before.as_ref(), 2).unwrap();
            let page = merge_closed_page(stored, &pending, before.as_ref(), 2);
            seen.extend(page.connections.iter().map(|c| c.id.clone()));
            match page.next {
                Some(next) => before = Some(next),
                None => break,
            }
        }
        assert_eq!(seen, ["g", "f", "e", "d", "c", "b", "a"]);
    }

    #[test]
    fn a_flush_between_pages_neither_skips_nor_repeats() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                closed: vec![closed("a", 10), closed("b", 20)],
                ..batch()
            })
            .unwrap();
        let pending = vec![closed("c", 30), closed("d", 40)];

        let stored = store.closed_connections(None, 2).unwrap();
        let first = merge_closed_page(stored, &pending, None, 2);
        let ids = |page: &ClosedPage| {
            page.connections
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ["d", "c"]);

        // The pending connections move to the store before the next page.
        store
            .flush(&FlushBatch {
                closed: pending,
                ..batch()
            })
            .unwrap();
        let before = first.next;
        let stored = store.closed_connections(before.as_ref(), 2).unwrap();
        let second = merge_closed_page(stored, &[], before.as_ref(), 2);
        assert_eq!(ids(&second), ["b", "a"]);
    }

    #[test]
    fn closed_connections_clamp_the_limit_and_negative_times() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                closed: vec![closed("old", -5), closed("new", 7)],
                ..batch()
            })
            .unwrap();

        let page = store.closed_connections(None, 0).unwrap();
        assert_eq!(page.connections.len(), 1);
        assert_eq!(page.connections[0].id, "new");

        let all = store.closed_connections(None, usize::MAX).unwrap();
        assert_eq!(all.connections.len(), 2);
        assert_eq!(all.next, None);
        assert_eq!(all.connections[1].id, "old");

        let cursor = ClosedCursor {
            closed_at: -5,
            id: "old".into(),
        };
        assert!(
            store
                .closed_connections(Some(&cursor), 10)
                .unwrap()
                .connections
                .is_empty()
        );
    }

    #[test]
    fn a_v1_file_is_replaced() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Database::create(&path).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(SCHEMA)
            .unwrap()
            .insert(VERSION_KEY, 1)
            .unwrap();
        txn.open_table(SESSION)
            .unwrap()
            .insert(META_KEY, br#"{"profile":"p1"}"#.as_slice())
            .unwrap();
        txn.open_table(TableDefinition::<&str, (u64, u64)>::new("topology"))
            .unwrap();
        txn.commit().unwrap();
        drop(db);

        let store = RedbTrafficStore::open(&path).unwrap();
        assert_eq!(store.load().unwrap(), None);
        assert!(usage_of_tier(&store, Tier::Hour, None).is_empty());
        // The fresh database is v2 and usable.
        store
            .flush(&FlushBatch {
                hours: vec![row(1, "curl", usage(1, 1, 1))],
                ..batch()
            })
            .unwrap();
        assert_eq!(usage_of_tier(&store, Tier::Hour, None).len(), 1);
        drop(store);

        let db = Database::open(&path).unwrap();
        let txn = db.begin_read().unwrap();
        assert_eq!(
            txn.open_table(SCHEMA)
                .unwrap()
                .get(VERSION_KEY)
                .unwrap()
                .unwrap()
                .value(),
            SCHEMA_VERSION
        );
        assert!(
            txn.open_table(TableDefinition::<&str, (u64, u64)>::new("topology"))
                .is_err()
        );
    }

    #[test]
    fn schema_mismatch_wipes_the_file() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch()).unwrap();
        drop(store);

        let db = Database::create(db_path(&dir)).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(SCHEMA)
            .unwrap()
            .insert(VERSION_KEY, SCHEMA_VERSION + 1)
            .unwrap();
        txn.commit().unwrap();
        drop(db);

        let store = open(&dir);
        assert_eq!(store.load().unwrap(), None);
        assert!(usage_of_tier(&store, Tier::Hour, None).is_empty());
        // The fresh database is usable.
        store.flush(&batch()).unwrap();
        assert!(store.load().unwrap().is_some());
    }

    #[test]
    fn table_type_mismatch_wipes_the_file() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Database::create(&path).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(TableDefinition::<&str, &str>::new("schema"))
            .unwrap();
        txn.commit().unwrap();
        drop(db);

        assert_eq!(RedbTrafficStore::open(&path).unwrap().load().unwrap(), None);
    }

    #[test]
    fn corrupted_file_is_replaced() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![0xAB; 64 * 1024]).unwrap();

        let store = RedbTrafficStore::open(&path).unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.flush(&batch()).unwrap();
    }

    #[test]
    fn a_database_that_is_already_open_is_not_deleted() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch()).unwrap();

        assert!(RedbTrafficStore::open(&db_path(&dir)).is_err());
        drop(store);

        assert!(open(&dir).load().unwrap().is_some());
    }

    /// 500 combinations over 1000 hours. Run in release mode:
    /// `cargo test --release -p nyanpasu-traffic usage_scan_500k_hour_rows -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark"]
    fn usage_scan_500k_hour_rows() {
        use crate::query::TopologyRequest;
        use std::time::Instant;

        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let tuples: Vec<Arc<Dimensions>> = (0..500)
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
                })
            })
            .collect();

        let hours: Vec<u32> = (0..1000).collect();
        for chunk in hours.chunks(50) {
            let rows = chunk
                .iter()
                .flat_map(|&hour| {
                    tuples
                        .iter()
                        .map(move |d| (hour, Arc::clone(d), usage(u64::from(hour) + 1, 2, 1)))
                })
                .collect();
            store
                .flush(&FlushBatch {
                    hours: rows,
                    ..batch()
                })
                .unwrap();
        }

        let started = Instant::now();
        let stored = store.usage(Tier::Hour, None).unwrap();
        let scan = started.elapsed();

        let request = ReportRequest {
            query: TrafficQuery {
                range: TrafficRange::All,
                scope: TrafficScope::Closed,
                filters: vec![TrafficFilter {
                    dimension: Dimension::Inbound,
                    value: "mixed-1".into(),
                }],
            },
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
                metric: Metric::Bytes,
                limit_per_layer: Some(7),
            }),
        };
        let started = Instant::now();
        let report = report(stored.iter().map(|(d, u)| (&**d, *u, None)), &request);
        let aggregate = started.elapsed();

        println!(
            "usage(Hour, None): {} tuples in {scan:?}; one report over them: {aggregate:?}",
            stored.len()
        );
        assert_eq!(stored.len(), 500);
        assert_eq!(stored[0].1.connections, 1000);
        assert!(report.total.connections > 0);
    }
}
