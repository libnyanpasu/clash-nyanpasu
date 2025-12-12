use std::sync::Arc;

use crate::core::state_v2::Context;
use camino::Utf8PathBuf;
use tokio::sync::RwLock;

use crate::{
    core::state_v2::{SimpleStateManager, StateCoordinator},
    utils::help,
};

use super::profiles::Profiles;

pub struct ProfilesService {
    manager: Arc<RwLock<SimpleStateManager<Profiles>>>,
    /// the path of the profiles file
    profiles_path: Utf8PathBuf,
    /// the directory of the profile items
    profile_items_dir: Utf8PathBuf,
}

impl ProfilesService {
    pub fn new(profiles_path: Utf8PathBuf, profile_items_dir: Utf8PathBuf) -> Self {
        Self {
            manager: Arc::new(RwLock::new(
                SimpleStateManager::new(StateCoordinator::new()),
            )),
            profiles_path,
            profile_items_dir,
        }
    }

    pub async fn load(&self) -> Result<(), anyhow::Error> {
        let mut manager = self.manager.write().await;
        let profiles = help::read_yaml(&self.profiles_path)
            .await
            .inspect_err(|e| {
                tracing::error!(
                    "failed to read the profiles file: {e:?} - use the default profiles"
                );
            })
            .unwrap_or_else(|_| Profiles::default());

        manager.upsert(profiles).await?;

        Ok(())
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

    pub async fn current_state(&self) -> Option<Profiles> {
        match Context::get::<Profiles>() {
            Some(profiles) => Some(profiles),
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
