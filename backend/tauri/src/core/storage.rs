use parking_lot::Mutex;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::{ResultExt as _, Snafu};
use specta::Type;
use std::{fs, ops::Deref, result::Result as StdResult, sync::Arc};
use tauri_specta::Event;
use tokio::sync::broadcast::{self, Receiver, Sender, error::RecvError};

/// What a storage operation failed with. Library causes stay in `source`
/// (skipped on the wire); they reach the user only through the copied detail.
/// A `key` is the storage key, which the web layer prefixes.
#[derive(Debug, Snafu, Serialize, Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StorageOperationError {
    #[snafu(display("failed to open the database at {path}"))]
    OpenDatabase {
        path: String,
        #[serde(skip)]
        source: redb::DatabaseError,
    },
    #[snafu(display("failed to start a storage transaction"))]
    BeginTransaction {
        #[serde(skip)]
        source: redb::TransactionError,
    },
    #[snafu(display("failed to open the storage table"))]
    OpenTable {
        #[serde(skip)]
        source: redb::TableError,
    },
    #[snafu(display("failed to read {key}"))]
    ReadItem {
        key: String,
        #[serde(skip)]
        source: redb::StorageError,
    },
    #[snafu(display("failed to write {key}"))]
    WriteItem {
        key: String,
        #[serde(skip)]
        source: redb::StorageError,
    },
    #[snafu(display("failed to remove {key}"))]
    RemoveItem {
        key: String,
        #[serde(skip)]
        source: redb::StorageError,
    },
    #[snafu(display("failed to list the stored items"))]
    ListItems {
        #[serde(skip)]
        source: redb::StorageError,
    },
    #[snafu(display("failed to commit the storage transaction"))]
    CommitTransaction {
        #[serde(skip)]
        source: redb::CommitError,
    },
    #[snafu(display("failed to decode the value of {key}"))]
    DecodeValue {
        key: String,
        #[serde(skip)]
        source: serde_json::Error,
    },
    #[snafu(display("failed to encode the value of {key}"))]
    EncodeValue {
        key: String,
        #[serde(skip)]
        source: serde_json::Error,
    },
}

pub const NYANPASU_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("clash-nyanpasu");

type Result<T> = StdResult<T, StorageOperationError>;

/// storage is a wrapper or called a facade for the rocksdb
/// Maybe provide a facade for a kv storage is a good idea?
#[derive(Clone)]
pub struct Storage {
    inner: Arc<StorageInner>,
}

impl Storage {
    pub fn try_new(path: &std::path::Path) -> Result<Self> {
        let inner = StorageInner::try_new(path)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Writes a consistent snapshot of the store into a new database at `dest`.
    ///
    /// Everything is read in one read transaction, so the snapshot is
    /// consistent even while writers are active, and it does not copy the
    /// file, which redb keeps locked on Windows.
    pub fn export_to(&self, dest: &std::path::Path) -> Result<()> {
        let read_txn = self
            .inner
            .instance
            .begin_read()
            .context(BeginTransactionSnafu)?;
        let source = read_txn
            .open_table(NYANPASU_TABLE)
            .context(OpenTableSnafu)?;

        let snapshot = StorageInner::create_and_init_database(dest)?;
        let write_txn = snapshot.begin_write().context(BeginTransactionSnafu)?;
        {
            let mut table = write_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            for entry in source.iter().context(ListItemsSnafu)? {
                let (key, value) = entry.context(ListItemsSnafu)?;
                table
                    .insert(key.value(), value.value())
                    .context(WriteItemSnafu {
                        key: String::from_utf8_lossy(key.value()),
                    })?;
            }
        }
        write_txn.commit().context(CommitTransactionSnafu)
    }
}

impl Deref for Storage {
    type Target = Arc<StorageInner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct StorageInner {
    instance: redb::Database,
    /// The revision of the last commit. Held from each commit until its change
    /// is published, so subscribers see changes in commit order, and across
    /// each read, so a value comes with the revision it was read at.
    revision: Mutex<StorageRevision>,
    tx: Sender<StorageChange>,
    #[cfg(test)]
    before_publish: Option<PublishHook>,
}

/// Runs between a commit and its publication.
#[cfg(test)]
type PublishHook = Box<dyn Fn(&str) + Send + Sync>;

/// Orders the commits of one open store, every key alike. It is kept in
/// memory and starts at 0 each time the store opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct StorageRevision(u64);

impl StorageRevision {
    fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("an open store never commits u64::MAX times"),
        )
    }
}

/// A stored value, or `None` if the key is absent, as of `revision`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct StorageItem<T> {
    pub value: Option<T>,
    pub revision: StorageRevision,
}

/// A committed change: the stored bytes, or `None` once the key is removed.
#[derive(Debug, Clone)]
pub struct StorageChange {
    pub key: String,
    pub value: Option<Vec<u8>>,
    pub revision: StorageRevision,
}

#[derive(Debug)]
pub enum StorageNotification {
    Changed(StorageChange),
    /// Changes were dropped before they were forwarded.
    Resync,
}

/// Event emitted to all windows when storage changes were dropped, so every
/// storage value must be read again.
/// Event name: `storage-resync-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageResyncEvent {}

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
    pub revision: StorageRevision,
}

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<StorageItem<T>>;
    /// Returns the revision the value was committed at.
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<StorageRevision>;
    /// Returns the revision the removal was committed at.
    fn remove_item(&self, key: impl AsRef<str>) -> Result<StorageRevision>;
    /// Returns all key-value pairs as raw JSON strings (for debug use).
    fn get_all(&self) -> Result<Vec<(String, String)>>;
}

impl StorageInner {
    fn create_and_init_database(path: &std::path::Path) -> Result<redb::Database> {
        let db = redb::Database::create(path).context(OpenDatabaseSnafu {
            path: path.display().to_string(),
        })?;
        // Create table
        let write_txn = db.begin_write().context(BeginTransactionSnafu)?;
        write_txn
            .open_table(NYANPASU_TABLE)
            .context(OpenTableSnafu)?;
        write_txn.commit().context(CommitTransactionSnafu)?;
        Ok(db)
    }

    pub fn try_new(path: &std::path::Path) -> Result<Self> {
        let metadata = fs::metadata(path).ok();
        let instance: redb::Database = if metadata.as_ref().is_some_and(|m| m.is_file()) {
            match redb::Database::open(path) {
                Ok(db) => db,
                // In redb v3 upgrading point, we only store the task history, and frontend persist state,
                // such as memorized router, which is NOT very valuable to make us keep two redb versions,
                // intended to support upgrade database formats.
                Err(redb::DatabaseError::UpgradeRequired(ver)) => {
                    tracing::error!("database upgrade required {ver:?}, removing...");
                    fs::remove_file(path).unwrap();
                    Self::create_and_init_database(path)?
                }
                Err(source) => {
                    return Err(source).context(OpenDatabaseSnafu {
                        path: path.display().to_string(),
                    });
                }
            }
        } else {
            // Remove previous rocksdb files
            if metadata.is_some_and(|m| m.is_dir()) {
                fs::remove_dir_all(path).unwrap();
            }
            Self::create_and_init_database(path)?
        };
        Ok(Self {
            instance,
            revision: Mutex::new(StorageRevision(0)),
            tx: broadcast::channel(16).0,
            #[cfg(test)]
            before_publish: None,
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }

    pub fn subscribe(&self) -> Receiver<StorageChange> {
        self.tx.subscribe()
    }

    /// Stores `value` under `key`, or removes the key for `None`, and
    /// publishes the change once it commits. A failed write publishes nothing.
    fn commit(&self, key: &str, value: Option<Vec<u8>>) -> Result<StorageRevision> {
        let mut revision = self.revision.lock();
        let write_txn = self.instance.begin_write().context(BeginTransactionSnafu)?;
        {
            let mut table = write_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            match &value {
                Some(value) => {
                    table
                        .insert(key.as_bytes(), value.as_slice())
                        .context(WriteItemSnafu { key })?;
                }
                None => {
                    table
                        .remove(key.as_bytes())
                        .context(RemoveItemSnafu { key })?;
                }
            }
        }
        write_txn.commit().context(CommitTransactionSnafu)?;
        *revision = revision.next();
        #[cfg(test)]
        if let Some(before_publish) = &self.before_publish {
            before_publish(key);
        }
        // Having no subscriber is not a failure of the write.
        let _ = self.tx.send(StorageChange {
            key: key.to_owned(),
            value,
            revision: *revision,
        });
        Ok(*revision)
    }
}

impl WebStorage for StorageInner {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<StorageItem<T>> {
        let key = key.as_ref();
        let revision = self.revision.lock();
        let db = self.get_instance();
        let read_txn = db.begin_read().context(BeginTransactionSnafu)?;
        let table = read_txn
            .open_table(NYANPASU_TABLE)
            .context(OpenTableSnafu)?;
        let value = match table.get(key.as_bytes()).context(ReadItemSnafu { key })? {
            Some(value) => {
                Some(serde_json::from_slice(value.value()).context(DecodeValueSnafu { key })?)
            }
            None => None,
        };
        Ok(StorageItem {
            value,
            revision: *revision,
        })
    }

    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<StorageRevision> {
        let key = key.as_ref();
        let value = serde_json::to_vec(value).context(EncodeValueSnafu { key })?;
        self.commit(key, Some(value))
    }

    fn remove_item(&self, key: impl AsRef<str>) -> Result<StorageRevision> {
        self.commit(key.as_ref(), None)
    }

    fn get_all(&self) -> Result<Vec<(String, String)>> {
        let db = self.get_instance();
        let read_txn = db.begin_read().context(BeginTransactionSnafu)?;
        let table = read_txn
            .open_table(NYANPASU_TABLE)
            .context(OpenTableSnafu)?;
        let mut result = Vec::new();
        for entry in table.iter().context(ListItemsSnafu)? {
            let (key, value) = entry.context(ListItemsSnafu)?;
            let key = String::from_utf8_lossy(key.value()).to_string();
            let value = String::from_utf8_lossy(value.value()).to_string();
            result.push((key, value));
        }
        Ok(result)
    }
}

/// Forwards changes in publication order. A lag becomes a resync and
/// forwarding continues; only a closed channel ends it.
pub async fn forward_storage_changes(
    mut rx: Receiver<StorageChange>,
    mut emit: impl FnMut(StorageNotification),
) {
    loop {
        match rx.recv().await {
            Ok(change) => emit(StorageNotification::Changed(change)),
            Err(RecvError::Lagged(_)) => emit(StorageNotification::Resync),
            Err(RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Barrier, thread};

    fn open(dir: &tempfile::TempDir) -> StorageInner {
        StorageInner::try_new(&dir.path().join("storage.redb")).expect("storage should open")
    }

    #[test]
    fn a_value_of_the_wrong_shape_names_the_key_it_failed_to_decode() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let storage =
            StorageInner::try_new(&dir.path().join("storage.redb")).expect("storage should open");
        storage.set_item("web:key", &"text").unwrap();

        let error = storage.get_item::<u32>("web:key").unwrap_err();

        assert!(
            matches!(&error, StorageOperationError::DecodeValue { key, .. } if key == "web:key"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_lag_becomes_a_resync_and_forwarding_continues_until_closed() {
        let (tx, rx) = broadcast::channel(16);
        for index in 0..17 {
            tx.send(StorageChange {
                key: format!("web:{index}"),
                value: None,
                revision: StorageRevision(index + 1),
            })
            .unwrap();
        }
        drop(tx);

        let mut notifications = Vec::new();
        forward_storage_changes(rx, |notification| notifications.push(notification)).await;

        // The oldest change was dropped; the rest still arrive, in order.
        assert!(matches!(notifications[0], StorageNotification::Resync));
        let keys: Vec<_> = notifications[1..]
            .iter()
            .map(|notification| match notification {
                StorageNotification::Changed(change) => change.key.clone(),
                StorageNotification::Resync => panic!("only the first receive lags"),
            })
            .collect();
        let expected: Vec<_> = (1..17).map(|index| format!("web:{index}")).collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn a_change_is_published_before_the_write_returns() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let mut rx = storage.subscribe();

        let written = storage.set_item("web:key", &"value").unwrap();
        let changed = rx.try_recv().unwrap();
        assert_eq!(changed.revision, written);
        assert_eq!(changed.key, "web:key");
        assert_eq!(changed.value.as_deref(), Some(br#""value""#.as_slice()));

        let removal = storage.remove_item("web:key").unwrap();
        let removed = rx.try_recv().unwrap();
        assert_eq!(removed.revision, removal);
        assert_eq!(removed.key, "web:key");
        assert_eq!(removed.value, None);
    }

    #[test]
    fn a_commit_holds_the_write_order_until_its_change_is_published() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = open(&dir);
        let committed = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        storage.before_publish = Some(Box::new({
            let committed = committed.clone();
            let resume = resume.clone();
            move |key| {
                if key == "web:a" {
                    committed.wait();
                    resume.wait();
                }
            }
        }));
        let mut rx = storage.subscribe();

        let (held, first, second) = thread::scope(|scope| {
            let first = scope.spawn(|| storage.set_item("web:a", &1).unwrap());
            // `web:a` has committed and waits to be published.
            committed.wait();
            let held = storage.revision.try_lock().is_none();
            resume.wait();
            let second = storage.set_item("web:b", &2).unwrap();
            (held, first.join().unwrap(), second)
        });

        assert!(
            held,
            "a commit must hold the write order until it is published"
        );
        let published: Vec<_> = [rx.try_recv().unwrap(), rx.try_recv().unwrap()]
            .into_iter()
            .map(|change| (change.key, change.revision))
            .collect();
        assert_eq!(
            published,
            [("web:a".to_owned(), first), ("web:b".to_owned(), second)]
        );
        assert!(first < second);
    }

    #[test]
    fn every_commit_advances_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let mut rx = storage.subscribe();

        let revisions = [
            storage.set_item("web:key", &1).unwrap(),
            storage.set_item("web:key", &1).unwrap(),
            storage.remove_item("web:key").unwrap(),
            storage.remove_item("web:key").unwrap(),
            storage.set_item("internal", &2).unwrap(),
        ];

        assert!(revisions.is_sorted_by(|a, b| a < b), "{revisions:?}");
        for revision in revisions {
            assert_eq!(rx.try_recv().unwrap().revision, revision);
        }
    }

    #[test]
    fn a_read_returns_the_revision_of_the_last_commit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);

        let empty = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(empty.value, None);
        assert_eq!(empty.revision, StorageRevision(0));

        let written = storage.set_item("web:key", &"kept").unwrap();
        let item = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(item.value.as_deref(), Some("kept"));
        assert_eq!(item.revision, written);

        let removal = storage.remove_item("web:key").unwrap();
        let removed = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(removed.value, None);
        assert_eq!(removed.revision, removal);
    }

    #[test]
    fn a_value_that_fails_to_encode_commits_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let written = storage.set_item("web:key", &"kept").unwrap();
        let mut rx = storage.subscribe();

        // JSON object keys must be strings.
        let unencodable = BTreeMap::from([((1_u8, 2_u8), 3_u8)]);
        let error = storage.set_item("web:key", &unencodable).unwrap_err();

        assert!(
            matches!(&error, StorageOperationError::EncodeValue { key, .. } if key == "web:key"),
            "{error:?}"
        );
        let item = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(item.value.as_deref(), Some("kept"));
        assert_eq!(item.revision, written);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn reopening_keeps_the_values_and_restarts_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        open(&dir).set_item("web:key", &"saved").unwrap();

        let item = open(&dir).get_item::<String>("web:key").unwrap();

        assert_eq!(item.value.as_deref(), Some("saved"));
        assert_eq!(item.revision, StorageRevision(0));
    }
}
