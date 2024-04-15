use redb::TableDefinition;

use crate::utils::dirs;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

/// storage is a wrapper or called a facade for the rocksdb
/// Maybe provide a facade for a kv storage is a good idea?
pub struct Storage {
    instance: redb::Database,
}

impl Storage {
    pub fn global() -> &'static Self {
        static STORAGE: OnceLock<Arc<Storage>> = OnceLock::new();

        STORAGE.get_or_init(|| {
            let path = dirs::storage_path().unwrap().to_str().unwrap().to_string();
            let path = PathBuf::from(&path);
            let instance: redb::Database = if path.exists() && !path.is_dir() {
                redb::Database::open(&path).unwrap()
            } else {
                if path.exists() && path.is_dir() {
                    fs::remove_dir_all(&path).unwrap();
                }
                let db = redb::Database::create(&path).unwrap();
                const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("clash-nyanpasu");
                // Create table
                let write_txn = db.begin_write().unwrap();
                write_txn.open_table(TABLE).unwrap();
                write_txn.commit().unwrap();
                db
            };
            Arc::new(Storage { instance })
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }
}
