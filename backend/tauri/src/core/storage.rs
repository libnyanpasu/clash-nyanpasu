use crate::utils::dirs;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

/// storage is a wrapper or called a facade for the rocksdb
/// Maybe provide a facade for a kv storage is a good idea?
pub struct Storage {
    instance: redb::Database,
    path: String,
}

impl Storage {
    pub fn global() -> &'static Self {
        static STORAGE: OnceLock<Arc<Storage>> = OnceLock::new();

        STORAGE.get_or_init(|| {
            let path = dirs::storage_path().unwrap().to_str().unwrap().to_string();
            let instance: redb::Database = if PathBuf::from(&path).exists() {
                redb::Database::open(&path).unwrap()
            } else {
                redb::Database::create(&path).unwrap()
            };
            Arc::new(Storage { instance, path })
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }
}

// impl Drop for Storage {
//     fn drop(&mut self) {
//     }
// }
