use crate::{log_err, utils::dirs};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::{ResultExt as _, Snafu};
use specta::Type;
use std::{fs, ops::Deref, result::Result as StdResult, sync::Arc};
use tauri::Manager;
use tauri_specta::Event;

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
    tx: tokio::sync::broadcast::Sender<(String, Option<Vec<u8>>)>,
}

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
}

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<Option<T>>;
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()>;
    fn remove_item(&self, key: impl AsRef<str>) -> Result<()>;
    /// Returns all key-value pairs as raw JSON strings (for debug use).
    fn get_all(&self) -> Result<Vec<(String, String)>>;
    /// Removes all entries from the storage (for debug use).
    #[allow(dead_code)]
    fn clear(&self) -> Result<()>;
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
            tx: tokio::sync::broadcast::channel(16).0,
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }

    fn notify_subscribers(&self, key: impl AsRef<str>, value: Option<&[u8]>) {
        let key = key.as_ref().to_string();
        let value = value.map(|v| v.to_vec());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((key, value));
        });
    }

    fn get_rx(&self) -> tokio::sync::broadcast::Receiver<(String, Option<Vec<u8>>)> {
        self.tx.subscribe()
    }
}

impl WebStorage for StorageInner {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<Option<T>> {
        let key = key.as_ref();
        let db = self.get_instance();
        let read_txn = db.begin_read().context(BeginTransactionSnafu)?;
        let table = read_txn
            .open_table(NYANPASU_TABLE)
            .context(OpenTableSnafu)?;
        let result = table.get(key.as_bytes()).context(ReadItemSnafu { key })?;
        match result {
            Some(value) => {
                let value = value.value();
                let value = serde_json::from_slice(value).context(DecodeValueSnafu { key })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()> {
        let key_str = key.as_ref();
        let key = key_str.as_bytes();
        let value = serde_json::to_vec(value).context(EncodeValueSnafu { key: key_str })?;
        let db = self.get_instance();
        let write_txn = db.begin_write().context(BeginTransactionSnafu)?;
        {
            let mut table = write_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            table
                .insert(key, &*value)
                .context(WriteItemSnafu { key: key_str })?;
        }
        write_txn.commit().context(CommitTransactionSnafu)?;
        self.notify_subscribers(key_str, Some(&value));
        Ok(())
    }

    fn remove_item(&self, key: impl AsRef<str>) -> Result<()> {
        let key_str = key.as_ref();
        let key = key_str.as_bytes();
        let db = self.get_instance();
        let write_txn = db.begin_write().context(BeginTransactionSnafu)?;
        {
            let mut table = write_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            table
                .remove(key)
                .context(RemoveItemSnafu { key: key_str })?;
        }
        write_txn.commit().context(CommitTransactionSnafu)?;
        self.notify_subscribers(key_str, None);
        Ok(())
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

    fn clear(&self) -> Result<()> {
        let db = self.get_instance();
        // Collect all keys in a read transaction first
        let keys: Vec<Vec<u8>> = {
            let read_txn = db.begin_read().context(BeginTransactionSnafu)?;
            let table = read_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            let mut keys = Vec::new();
            for entry in table.iter().context(ListItemsSnafu)? {
                let (key, _) = entry.context(ListItemsSnafu)?;
                keys.push(key.value().to_vec());
            }
            keys
        };
        // Remove all in a write transaction
        let write_txn = db.begin_write().context(BeginTransactionSnafu)?;
        {
            let mut table = write_txn
                .open_table(NYANPASU_TABLE)
                .context(OpenTableSnafu)?;
            for key in &keys {
                table.remove(key.as_slice()).context(RemoveItemSnafu {
                    key: String::from_utf8_lossy(key),
                })?;
            }
        }
        write_txn.commit().context(CommitTransactionSnafu)?;
        Ok(())
    }
}

pub fn register_web_storage_listener(app_handle: &tauri::AppHandle) {
    let storage = app_handle.state::<Storage>();
    let rx = storage.get_rx();
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async {
            let mut rx = rx;

            while let Ok((key, value)) = rx.recv().await {
                let value = value.map(|v| String::from_utf8_lossy(&v).to_string());
                let event = StorageValueChangedEvent { key, value };
                log_err!(
                    event.emit(&app_handle),
                    "failed to emit storage_value_changed event"
                );
            }
        });
    });
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> anyhow::Result<()> {
    let storage_path =
        anyhow::Context::context(dirs::storage_path(), "failed to get storage path")?;
    let storage = Storage::try_new(&storage_path)?;
    app.manage(storage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
