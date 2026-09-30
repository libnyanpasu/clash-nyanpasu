use crate::{accounting::FlushBatch, model::*, ports::TrafficStore};
use ::redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt::Display, fs, ops::Bound, path::Path};

/// Statistics are disposable: a file written by another schema is wiped, never migrated.
const SCHEMA_VERSION: u64 = 1;

const VERSION_KEY: &str = "version";
const META_KEY: &str = "meta";

const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("schema");
/// JSON `SessionMeta` under `META_KEY`.
const SESSION: TableDefinition<&str, &[u8]> = TableDefinition::new("session");
/// Connection id -> JSON `ActiveConnection`.
const ACTIVE: TableDefinition<&str, &[u8]> = TableDefinition::new("active");
/// (closed_at, connection id) -> JSON `ClosedConnection`.
const CLOSED: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("closed");
/// (group name, group key) -> (upload, download).
const TOTALS: TableDefinition<(&str, &str), (u64, u64)> = TableDefinition::new("totals");
/// JSON `TopologyKey` -> (upload, download).
const TOPOLOGY: TableDefinition<&str, (u64, u64)> = TableDefinition::new("topology");

pub struct RedbTrafficStore {
    db: Database,
}

impl RedbTrafficStore {
    /// Opens the database at `path`, replacing it when it is corrupted or from another schema.
    pub fn open(path: &Path) -> TrafficResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        if let Some(db) = Self::init(path)? {
            return Ok(Self { db });
        }
        fs::remove_file(path).map_err(storage)?;
        Self::init(path)?
            .map(|db| Self { db })
            .ok_or_else(|| TrafficError::Storage("fresh database was rejected".into()))
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
            txn.open_table(TOTALS)?;
            txn.open_table(TOPOLOGY)?;
        }
        txn.commit()?;
        Ok(Some(db))
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

    fn flush(&self, batch: &FlushBatch) -> TrafficResult<()> {
        let txn = self.db.begin_write().map_err(storage)?;
        {
            if batch.reset {
                // The active set is replaced below on every flush.
                txn.open_table(CLOSED)
                    .map_err(storage)?
                    .retain(|_, _| false)
                    .map_err(storage)?;
                txn.open_table(TOTALS)
                    .map_err(storage)?
                    .retain(|_, _| false)
                    .map_err(storage)?;
                txn.open_table(TOPOLOGY)
                    .map_err(storage)?
                    .retain(|_, _| false)
                    .map_err(storage)?;
            }

            txn.open_table(SESSION)
                .map_err(storage)?
                .insert(META_KEY, encode(&batch.meta)?.as_slice())
                .map_err(storage)?;

            let mut active = txn.open_table(ACTIVE).map_err(storage)?;
            active.retain(|_, _| false).map_err(storage)?;
            for conn in &batch.active {
                active
                    .insert(conn.id.as_str(), encode(conn)?.as_slice())
                    .map_err(storage)?;
            }

            let mut totals = txn.open_table(TOTALS).map_err(storage)?;
            for (group, key, delta) in &batch.totals {
                let slot = (group.name(), key.as_str());
                let current = totals
                    .get(slot)
                    .map_err(storage)?
                    .map(|v| bytes_of(v.value()));
                let next = current.unwrap_or_default().saturating_add(*delta);
                totals
                    .insert(slot, (next.upload, next.download))
                    .map_err(storage)?;
            }

            let mut topology = txn.open_table(TOPOLOGY).map_err(storage)?;
            for (key, delta) in &batch.topology {
                let key = encode_key(key)?;
                let current = topology
                    .get(key.as_str())
                    .map_err(storage)?
                    .map(|v| bytes_of(v.value()));
                let next = current.unwrap_or_default().saturating_add(*delta);
                topology
                    .insert(key.as_str(), (next.upload, next.download))
                    .map_err(storage)?;
            }

            // TODO(traffic): cap retained closed connections (and total keys) per session by count or age; a profile session can last months.
            let mut closed = txn.open_table(CLOSED).map_err(storage)?;
            for conn in &batch.closed {
                closed
                    .insert(
                        (closed_key(conn.closed_at), conn.id.as_str()),
                        encode(conn)?.as_slice(),
                    )
                    .map_err(storage)?;
            }
        }
        txn.commit().map_err(storage)
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

    fn totals(&self, group: GroupBy) -> TrafficResult<Vec<(String, Bytes)>> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(TOTALS).map_err(storage)?;
        let name = group.name();

        let mut out = Vec::new();
        for entry in table.range::<(&str, &str)>((name, "")..).map_err(storage)? {
            let (key, value) = entry.map_err(storage)?;
            let (group_name, key) = key.value();
            if group_name != name {
                break;
            }
            out.push((key.to_owned(), bytes_of(value.value())));
        }
        Ok(out)
    }

    fn topology(&self) -> TrafficResult<Vec<(TopologyKey, Bytes)>> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(TOPOLOGY).map_err(storage)?;

        let mut out = Vec::new();
        for entry in table.iter().map_err(storage)? {
            let (key, value) = entry.map_err(storage)?;
            out.push((decode(key.value().as_bytes())?, bytes_of(value.value())));
        }
        Ok(out)
    }
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

fn encode_key(key: &TopologyKey) -> TrafficResult<String> {
    serde_json::to_string(key).map_err(storage)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> TrafficResult<T> {
    serde_json::from_slice(bytes).map_err(storage)
}

fn bytes_of((upload, download): (u64, u64)) -> Bytes {
    Bytes { upload, download }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    fn bytes(upload: u64, download: u64) -> Bytes {
        Bytes { upload, download }
    }

    fn meta(profile: &str) -> SessionMeta {
        SessionMeta {
            profile: Some(profile.into()),
            started_at: 1_000,
            last_sample_at: Some(2_000),
            core_bytes: bytes(5, 6),
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
            bytes: bytes(3, 4),
            dimensions: dims("curl"),
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

    fn topology_key(exit: &str) -> TopologyKey {
        TopologyKey::from_dimensions(&Dimensions {
            chains: vec![exit.into(), "Proxy".into()],
            ..dims("curl")
        })
    }

    fn batch(profile: &str) -> FlushBatch {
        FlushBatch {
            reset: false,
            meta: meta(profile),
            active: vec![active("a")],
            totals: vec![
                (GroupBy::Process, "curl".into(), bytes(10, 20)),
                (GroupBy::Target, "example.com".into(), bytes(10, 20)),
            ],
            topology: vec![(topology_key("Node-A"), bytes(10, 20))],
            closed: Vec::new(),
        }
    }

    fn db_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("traffic").join("traffic.redb")
    }

    fn open(dir: &TempDir) -> RedbTrafficStore {
        RedbTrafficStore::open(&db_path(dir)).unwrap()
    }

    #[test]
    fn flush_then_reopen_loads_the_session() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        assert_eq!(store.load().unwrap(), None);

        let batch = batch("p1");
        store.flush(&batch).unwrap();
        drop(store);

        let (loaded, active) = open(&dir).load().unwrap().unwrap();
        assert_eq!(loaded, batch.meta);
        assert_eq!(active, batch.active);
    }

    #[test]
    fn flush_replaces_the_active_set() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch("p1")).unwrap();
        store
            .flush(&FlushBatch {
                active: vec![active("b"), active("c")],
                ..batch("p1")
            })
            .unwrap();

        let (_, mut active) = store.load().unwrap().unwrap();
        active.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            active.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["b", "c"]
        );
    }

    #[test]
    fn totals_and_topology_accumulate_across_flushes() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch("p1")).unwrap();
        store
            .flush(&FlushBatch {
                totals: vec![
                    (GroupBy::Process, "curl".into(), bytes(1, 2)),
                    (GroupBy::Process, "firefox".into(), bytes(7, 8)),
                ],
                topology: vec![
                    (topology_key("Node-A"), bytes(1, 2)),
                    (topology_key("Node-B"), bytes(5, 5)),
                ],
                ..batch("p1")
            })
            .unwrap();

        let mut process = store.totals(GroupBy::Process).unwrap();
        process.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            process,
            [
                ("curl".to_owned(), bytes(11, 22)),
                ("firefox".to_owned(), bytes(7, 8))
            ]
        );
        // Other groups are untouched and not mixed in.
        assert_eq!(
            store.totals(GroupBy::Target).unwrap(),
            [("example.com".to_owned(), bytes(10, 20))]
        );
        assert_eq!(store.totals(GroupBy::Exit).unwrap(), []);

        let mut topology = store.topology().unwrap();
        topology.sort_by(|a, b| a.0.exit.cmp(&b.0.exit));
        assert_eq!(
            topology,
            [
                (topology_key("Node-A"), bytes(11, 22)),
                (topology_key("Node-B"), bytes(5, 5))
            ]
        );
    }

    #[test]
    fn totals_saturate() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let huge = FlushBatch {
            totals: vec![(GroupBy::Process, "curl".into(), bytes(u64::MAX, 0))],
            ..batch("p1")
        };
        store.flush(&huge).unwrap();
        store.flush(&huge).unwrap();

        assert_eq!(
            store.totals(GroupBy::Process).unwrap(),
            [("curl".to_owned(), bytes(u64::MAX, 0))]
        );
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
                ..batch("p1")
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
                ..batch("p1")
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
                ..batch("p1")
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
                ..batch("p1")
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
                ..batch("p1")
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
    fn reset_flush_replaces_the_previous_session() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store
            .flush(&FlushBatch {
                closed: vec![closed("old", 10)],
                ..batch("p1")
            })
            .unwrap();

        let next = FlushBatch {
            reset: true,
            meta: meta("p2"),
            active: vec![active("survivor")],
            totals: vec![(GroupBy::Process, "firefox".into(), bytes(7, 8))],
            topology: vec![(topology_key("Node-B"), bytes(7, 8))],
            closed: vec![closed("new", 20)],
        };
        store.flush(&next).unwrap();

        let check = |store: &RedbTrafficStore| {
            // The batch's own baselines survive the wipe it carries.
            assert_eq!(
                store.load().unwrap(),
                Some((next.meta.clone(), next.active.clone()))
            );
            let closed = store.closed_connections(None, 10).unwrap();
            assert_eq!(closed.connections, next.closed);
            assert_eq!(
                store.totals(GroupBy::Process).unwrap(),
                [("firefox".to_owned(), bytes(7, 8))]
            );
            assert!(store.totals(GroupBy::Target).unwrap().is_empty());
            assert_eq!(
                store.topology().unwrap(),
                [(topology_key("Node-B"), bytes(7, 8))]
            );
        };
        check(&store);

        // It is committed, not only visible through this handle.
        drop(store);
        check(&open(&dir));
    }

    #[test]
    fn schema_mismatch_wipes_the_file() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch("p1")).unwrap();
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
        assert!(store.totals(GroupBy::Process).unwrap().is_empty());
        // The fresh database is usable.
        store.flush(&batch("p2")).unwrap();
        assert_eq!(
            store.load().unwrap().unwrap().0.profile.as_deref(),
            Some("p2")
        );
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
        store.flush(&batch("p1")).unwrap();
    }

    #[test]
    fn a_database_that_is_already_open_is_not_deleted() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        store.flush(&batch("p1")).unwrap();

        assert!(RedbTrafficStore::open(&db_path(&dir)).is_err());
        drop(store);

        assert!(open(&dir).load().unwrap().is_some());
    }
}
