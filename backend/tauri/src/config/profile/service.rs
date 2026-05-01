use std::sync::Arc;

use crate::core::state_v2::Context;
use camino::Utf8PathBuf;
use tokio::sync::RwLock;

use crate::{
    core::state_v2::{SimpleStateManager, SimpleStateManagerSetup, StateSnapshot},
    utils::help,
};

use super::profiles::Profiles;

pub struct ProfilesService {
    manager: Arc<RwLock<SimpleStateManager<Profiles>>>,
    snapshot: StateSnapshot<Profiles>,
    /// the path of the profiles file
    profiles_path: Utf8PathBuf,
    /// the directory of the profile items
    profile_items_dir: Utf8PathBuf,
}

impl ProfilesService {
    pub async fn new(
        profiles_path: Utf8PathBuf,
        profile_items_dir: Utf8PathBuf,
    ) -> Result<Self, anyhow::Error> {
        let profiles = help::read_yaml(&profiles_path)
            .await
            .inspect_err(|e| {
                tracing::error!(
                    "failed to read the profiles file: {e:?} - use the default profiles"
                );
            })
            .unwrap_or_else(|_| Profiles::default());

        let manager = SimpleStateManagerSetup::builder()
            .initial_state(profiles)
            .assemble()
            .initialize()
            .await?;
        let snapshot = manager.snapshot_handle();

        Ok(Self {
            manager: Arc::new(RwLock::new(manager)),
            snapshot,
            profiles_path,
            profile_items_dir,
        })
    }

    /// MVCC snapshot read: lock-free read of last committed state.
    pub fn snapshot(&self) -> Option<Arc<Profiles>> {
        self.snapshot.load()
    }

    async fn write_file(&self, profiles: Profiles) -> Result<(), anyhow::Error> {
        help::save_yaml(
            &self.profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )
        .await
        .inspect_err(|e| {
            tracing::error!("failed to save the profiles file: {e:?}");
        })?;
        Ok(())
    }

    pub async fn current_state(&self) -> Option<Arc<Profiles>> {
        match Context::get::<Profiles>() {
            Some(profiles) => Some(Arc::new(profiles)),
            None => self.manager.read().await.current_state(),
        }
    }

    pub async fn upsert(&self, profiles: Profiles) -> Result<(), anyhow::Error> {
        let mut manager = self.manager.write().await;
        manager.upsert_state_with_context(profiles.clone()).await?;
        self.write_file(profiles).await?;
        Ok(())
    }
}
