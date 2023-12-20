use crate::utils::dirs;
use rocksdb::MultiThreaded;
use std::sync::{Arc, OnceLock};

/// storage is a wrapper or called a facade for the rocksdb
/// Maybe provide a facade for a kv storage is a good idea?
pub struct Storage {
    instance: rocksdb::OptimisticTransactionDB<MultiThreaded>,
    path: String,
}

impl Storage {
    pub fn global() -> &'static Self {
        static STORAGE: OnceLock<Arc<Storage>> = OnceLock::new();

        STORAGE.get_or_init(|| {
            let path = dirs::storage_path().unwrap().to_str().unwrap().to_string();
            let instance =
                rocksdb::OptimisticTransactionDB::<MultiThreaded>::open_default(&path).unwrap();
            Arc::new(Storage { instance, path })
        })
    }

    pub fn get_instance(&self) -> &rocksdb::OptimisticTransactionDB<MultiThreaded> {
        &self.instance
    }

    pub fn destroy(&self) -> Result<(), rocksdb::Error> {
        rocksdb::DB::destroy(&rocksdb::Options::default(), &self.path)
    }
}
