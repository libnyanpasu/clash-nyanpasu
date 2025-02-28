mod indexer;
mod manager;

const LOGGING_NS: &str = "logging";
const LOGGING_DB_PREFIX: &str = "logging";

use anyhow::Context;
use camino::Utf8PathBuf;
pub use indexer::*;

use manager::IndexerManager;
use tauri::Manager;

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> anyhow::Result<()> {
    let app_handle = app.app_handle().clone();
    // FIXME: this is a background setup, so be careful use this state in ipc. If use state<T> when it is not ready, it will cause panic.
    nyanpasu_utils::runtime::spawn(async move {
        let logging_dir = crate::utils::dirs::app_logs_dir()
            .context("failed to get app logs dir")
            .unwrap();
        let logging_dir = Utf8PathBuf::from_path_buf(logging_dir).unwrap();
        let manager = IndexerManager::try_new(logging_dir)
            .await
            .context("failed to create indexer manager")
            .unwrap();
        app_handle.manage(manager);
    });

    Ok(())
}
