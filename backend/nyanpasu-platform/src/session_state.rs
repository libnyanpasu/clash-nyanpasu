//! Filesystem-backed construction for the application session-state owner.

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::state::PersistentState;
use nyanpasu_core::state::{PersistentStateManager, PersistentStateManagerSetup};

/// Loads the persisted session state when present, or initializes a new
/// manager at the host-provided path. The application actor receives the
/// manager and owns all subsequent mutations.
pub async fn open_manager(
    config_path: Utf8PathBuf,
) -> anyhow::Result<PersistentStateManager<PersistentState>> {
    let should_load = config_path.exists();
    let setup = PersistentStateManagerSetup::<PersistentState>::builder()
        .config_path(config_path)
        .assemble();
    if should_load {
        setup
            .load()
            .await
            .context("failed to load session persistent state manager")
    } else {
        setup
            .from_state(PersistentState::default())
            .await
            .context("failed to initialize session persistent state manager")
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_config::state::window::{WindowLabel, WindowState};

    use super::*;

    #[tokio::test]
    async fn opener_creates_and_reloads_the_hosts_persisted_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("session.yaml")).unwrap();
        let mut manager = open_manager(path.clone()).await.unwrap();
        let label = WindowLabel("custom-host".into());
        let geometry = WindowState {
            x: 42,
            ..Default::default()
        };
        let mut next = manager.snapshot_handle().load().state.clone();
        next.window_state.insert(label.clone(), geometry.clone());
        manager.upsert(next).await.unwrap();
        drop(manager);
        let reloaded = open_manager(path).await.unwrap();
        assert_eq!(
            reloaded
                .snapshot_handle()
                .load()
                .state
                .window_state
                .get(&label),
            Some(&geometry)
        );
    }
}
