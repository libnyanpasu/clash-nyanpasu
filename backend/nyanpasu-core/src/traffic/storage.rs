use nyanpasu_paths::PathResolver;
use nyanpasu_traffic::{RedbTrafficStore, TrafficStore};
use std::sync::Arc;

/// Recording is optional, so a store that cannot be opened disables it rather
/// than failing the launch. The history holds browsing targets, hence the
/// owner-only directory.
pub fn open_traffic_store(paths: &PathResolver) -> Option<Arc<dyn TrafficStore>> {
    let dir = paths.app_data_dir().join("traffic").into_std_path_buf();
    let open = || -> anyhow::Result<RedbTrafficStore> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)?;
        Ok(RedbTrafficStore::open(&dir.join("traffic.redb"))?)
    };
    match open() {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::error!(%error, "failed to open the traffic store; recording is disabled");
            None
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
