use crate::{log_err, utils::dirs};
use anyhow::Context;
use redb::TableDefinition;
use serde::{de::DeserializeOwned, Serialize};
use std::{fs, ops::Deref, result::Result as StdResult, sync::Arc};
use tauri::{Emitter, Manager};

#[derive(Debug, thiserror::Error)]
pub enum StorageOperationError {
    #[error("internal redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("internal redb table error: {0}")]
    RedbTable(#[from] redb::TableError),
    #[error("internal redb storage error: {0}")]
    RedbStorage(#[from] redb::StorageError),
    #[error("failed to start transaction: {0}")]
    RedbTransaction(#[from] redb::TransactionError),
    #[error("failed to commit transaction: {0}")]
    RedbCommit(#[from] redb::CommitError),
    #[error("failed to serialize or deserialize data: {0}")]
    Serialize(#[from] serde_json::Error),
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

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<Option<T>>;
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()>;
    fn remove_item(&self, key: impl AsRef<str>) -> Result<()>;
}

impl StorageInner {
    pub fn try_new(path: &std::path::Path) -> Result<Self> {
        let instance: redb::Database = if path.exists() && !path.is_dir() {
            redb::Database::open(path).unwrap()
        } else {
            if path.exists() && path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            }
            let db = redb::Database::create(path).unwrap();
            // Create table
            let write_txn = db.begin_write().unwrap();
            write_txn.open_table(NYANPASU_TABLE).unwrap();
            write_txn.commit().unwrap();
            db
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
        let key = key.as_ref().as_bytes();
        let db = self.get_instance();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let result = table.get(key)?;
        match result {
            Some(value) => {
                let value = value.value();
                let value = serde_json::from_slice(value)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()> {
        let key_str = key.as_ref();
        let key = key_str.as_bytes();
        let value = serde_json::to_vec(value)?;
        let db = self.get_instance();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.insert(key, &*value)?;
        }
        write_txn.commit()?;
        self.notify_subscribers(key_str, Some(&value));
        Ok(())
    }

    fn remove_item(&self, key: impl AsRef<str>) -> Result<()> {
        let key_str = key.as_ref();
        let key = key_str.as_bytes();
        let db = self.get_instance();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.remove(key)?;
        }
        write_txn.commit()?;
        self.notify_subscribers(key_str, None);
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
                let payload = (key, value);
                log_err!(app_handle.emit_filter(
                    "storage_value_changed",
                    payload,
                    |t| matches!(t, tauri::EventTarget::WebviewWindow { label } if label == "main"),
                ), "failed to emit storage_value_changed event");
            }
        });
    });
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> anyhow::Result<()> {
    let storage_path = dirs::storage_path().context("failed to get storage path")?;
    let storage = Storage::try_new(&storage_path)?;
    app.manage(storage);
    Ok(())
}
