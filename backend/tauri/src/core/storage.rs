use crate::utils::dirs;
use once_cell::sync::OnceCell;

/// storage is a wrapper or called a facade for the sled database

pub struct Storage {
    instance: sled::Db,
}

impl Storage {
    pub fn global() -> &'static Self {
        static STORAGE: OnceCell<Storage> = OnceCell::new();

        STORAGE.get_or_init(|| {
            let path = dirs::storage_path().unwrap();
            let instance = sled::open(path).expect("failed to open storage");
            Storage { instance }
        })
    }
}
