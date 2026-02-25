//! The service for managing the runtime config of Clash
//!
//! A ClashRuntimeConfig should follow the following process:
//!
//!```mermaid
//! flowchart TD
//! A[Profile Provider] --> B[ChainProcessing]
//! B --> C[FilterProcessing]
//! C --> D[ClashRuntimeConfigService.upsert]
//! D --> E[ClashConfigService.applyOverrides]
//! E --> F[Diff]
//! F --> G[Upsert / Patch]
//!```
//!
//!```mermaid
//! flowchart TD
//! S[Patch] --> J{Patch Kind?}
//! %% Case 1: Switch Profile (including Chain/Filter changes) → Full restart required
//! J -->|Switch Profile (including Chain/Filter changes)| A1[Introduce Profile as the original config]
//! A1 --> B1[ChainProcessing]
//! B1 --> C1[FilterProcessing]
//! C1 --> D1[ClashConfigService.applyOverrides]
//! D1 --> E1[Diff]
//! E1 --> F1[Upsert]

//! %% Case 2: Only modify ClashConfigService → Only modify the currently active config
//! J -->|Only modify ClashConfigService| K[Read the currently active config (Active Config)]
//! K --> D2[ClashConfigService.applyOverrides (Only the currently active config)]
//! D2 --> E2[Diff]
//! E2 --> F2[Upsert]
//!```

const SERVICE_NAME: &str = "ClashRuntimeConfigService";

use super::{PatchRuntimeConfig, snapshot};
use crate::{
    config::{
        ClashConfig, ClashConfigService, ClashRuntimeState, NyanpasuAppConfig, Profile,
        ProfileContentGuard, Profiles,
        nyanpasu::{ClashCore, NyanpasuAppConfigService},
        profile::ProfilesService,
    },
    core::state_v2::{Context, SimpleStateManager, StateChangedSubscriber, StateCoordinator},
    enhance::{self, EnhanceResult, PartialProfileItem},
};
use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
pub struct ClashInfo {
    /// clash core port
    pub proxy_mixed_port: u16,
    /// same as `external-controller`
    pub external_controller_server: SocketAddr,
    /// clash secret
    pub secret: Option<String>,
}

#[derive(Clone)]
pub struct ClashRuntimeConfigService {
    clash_config_service: Arc<ClashConfigService>,
    profiles_service: Arc<ProfilesService>,
    nyanpasu_config_service: Arc<NyanpasuAppConfigService>,
    runtime: Arc<RwLock<SimpleStateManager<ClashRuntimeState>>>,
}

#[async_trait::async_trait]
impl StateChangedSubscriber<Profiles> for ClashRuntimeConfigService {
    fn name(&self) -> &str {
        SERVICE_NAME
    }

    async fn migrate(
        &self,
        prev_state: Option<Profiles>,
        new_state: Profiles,
    ) -> Result<(), anyhow::Error> {
        todo!()
    }

    async fn rollback(
        &self,
        prev_state: Option<Profiles>,
        new_state: Profiles,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl StateChangedSubscriber<ClashConfig> for ClashRuntimeConfigService {
    fn name(&self) -> &str {
        SERVICE_NAME
    }

    async fn migrate(
        &self,
        prev_state: Option<ClashConfig>,
        new_state: ClashConfig,
    ) -> Result<(), anyhow::Error> {
        todo!()
    }

    async fn rollback(
        &self,
        prev_state: Option<ClashConfig>,
        new_state: ClashConfig,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl StateChangedSubscriber<NyanpasuAppConfig> for ClashRuntimeConfigService {
    fn name(&self) -> &str {
        SERVICE_NAME
    }

    async fn migrate(
        &self,
        prev_state: Option<NyanpasuAppConfig>,
        new_state: NyanpasuAppConfig,
    ) -> Result<(), anyhow::Error> {
        todo!()
    }

    async fn rollback(
        &self,
        prev_state: Option<NyanpasuAppConfig>,
        new_state: NyanpasuAppConfig,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
/// The payload for patching the runtime config
pub enum PatchPayload {
    /// The specific patch config, might be handled by the service itself
    Specific(PatchRuntimeConfig),
    /// The untyped patch config, might include some keys were triggered by the chain
    Untyped(Mapping),
}

impl ClashRuntimeConfigService {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(RwLock::new(
                SimpleStateManager::new(StateCoordinator::new()),
            )),
        }
    }

    /// Generate the runtime config based on the selected profile and global chain
    // TODO: Support patch generation from a snapshot node
    pub async fn generate_runtime_config<'r, 'c: 'r>(
        nyanpasu_config: &NyanpasuAppConfig,
        clash_config: &ClashConfig,
        // The valid fields from the profile
        valid_fields: &BTreeSet<String>,
        selected_profile: &[PartialProfileItem<'r, 'c>],
        global_chain: &[ProfileContentGuard<'c>],
    ) -> Result<ClashRuntimeState, anyhow::Error> {
        let opts = enhance::EnhanceOptions {
            clash_core: nyanpasu_config.core.clone(),
            enable_tun: clash_config.enable_tun_mode,
            enable_builtin_enhanced: nyanpasu_config.enable_builtin_enhanced,
            enable_clash_fields: clash_config.enable_clash_fields,
        };
        let valid_fields = Vec::from_iter(valid_fields.iter().cloned());

        let EnhanceResult {
            config,
            exists_keys,
            postprocessing_output,
            snapshots,
        } = enhance::process(
            opts,
            valid_fields.as_slice(),
            selected_profile,
            global_chain,
            &clash_config.overrides,
        )
        .await;

        Ok(ClashRuntimeState {
            config,
            exists_keys,
            postprocessing_output,
            snapshots: Some(snapshots),
        })
    }

    pub async fn patch_runtime_config(&self, patch: PatchPayload) -> Result<(), anyhow::Error> {
        let mut runtime = self.runtime.write().await;
        let mut state = match runtime.current_state() {
            Some(state) => state.clone(),
            None => anyhow::bail!("no runtime state found"),
        };
        match &patch {
            PatchPayload::Specific(patch) => {
                // TODO: handle specific patch
                let patch = serde_yaml::to_value(patch)?
                    .as_mapping()
                    .cloned()
                    .unwrap_or_default();
                crate::utils::yaml::apply_overrides(&mut state.config, &patch);
            }
            PatchPayload::Untyped(mapping) => {
                crate::utils::yaml::apply_overrides(&mut state.config, mapping);
            }
        };
        runtime
            .upsert_state_with_context(state)
            .await
            .with_context(|| format!("failed to upsert patch {patch:?}"))?;
        Ok(())
    }

    pub async fn upsert(&self, state: ClashRuntimeState) -> Result<(), anyhow::Error> {
        let mut runtime = self.runtime.write().await;
        runtime
            .upsert_state_with_context(state.clone())
            .await
            .with_context(|| format!("failed to upsert state {state:?}"))?;
        Ok(())
    }

    pub async fn current_state(&self) -> Option<ClashRuntimeState> {
        match Context::get::<ClashRuntimeState>() {
            Some(state) => Some(state.clone()),
            None => self.runtime.read().await.current_state(),
        }
    }

    /// Get the client info from the runtime config
    pub async fn get_client_info(&self) -> Option<ClashInfo> {
        let config = self.current_state().await?;
        let external_controller_server = config.get_external_controller_server()?;
        let proxy_mixed_port = config.get_proxy_mixed_port()?;
        let secret = config.get_secret();

        Some(ClashInfo {
            proxy_mixed_port,
            external_controller_server,
            secret,
        })
    }
}
