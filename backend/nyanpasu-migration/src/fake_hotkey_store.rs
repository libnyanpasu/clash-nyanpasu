//! Per-test hotkey stores whose marker files support migration's existence checks.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use nyanpasu_contracts::BoxedCause;

use crate::modules::storage::{HotkeyStore, HotkeyStoreOpener};

#[derive(Debug, Default)]
pub(crate) struct FakeHotkeyStoreOpener {
    stores: Mutex<BTreeMap<PathBuf, Arc<FakeHotkeyStore>>>,
}

impl HotkeyStoreOpener for FakeHotkeyStoreOpener {
    fn open(&self, path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
        let mut stores = self.stores.lock().expect("test store map lock poisoned");
        let store = match stores.get(path) {
            Some(store) => store.clone(),
            None => {
                fs::File::create(path)?;
                let store = Arc::new(FakeHotkeyStore::default());
                stores.insert(path.to_owned(), store.clone());
                store
            }
        };
        Ok(Box::new(store))
    }
}

// Handles share only this test store's hotkey value, including an explicit empty list.
#[derive(Debug, Default)]
struct FakeHotkeyStore {
    hotkeys: Mutex<Option<Vec<String>>>,
}

impl HotkeyStore for Arc<FakeHotkeyStore> {
    fn read(&self) -> Result<Option<Vec<String>>, BoxedCause> {
        Ok(self
            .hotkeys
            .lock()
            .expect("test hotkey lock poisoned")
            .clone())
    }

    fn write(&self, hotkeys: &[String]) -> Result<(), BoxedCause> {
        *self.hotkeys.lock().expect("test hotkey lock poisoned") = Some(hotkeys.to_vec());
        Ok(())
    }

    fn remove(&self) -> Result<(), BoxedCause> {
        *self.hotkeys.lock().expect("test hotkey lock poisoned") = None;
        Ok(())
    }
}
