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

use super::PatchRuntimeConfig;
use crate::{
    config::{
        ClashConfig, ClashConfigService, ClashRuntimeState, NyanpasuAppConfig, Profile,
        ProfileContentGuard, Profiles, nyanpasu::NyanpasuAppConfigService,
        profile::ProfilesService,
    },
    core::state_v2::{
        Context, StateChangedSubscriber, StateCoordinator, WeakPersistentStateManager, YamlFormat,
    },
    enhance::{self, EnhanceResult, PartialProfileItem},
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

type RuntimeManager = WeakPersistentStateManager<ClashRuntimeState>;

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
    runtime: Arc<RwLock<RuntimeManager>>,
}

// -- StateChangedSubscriber impls --

#[async_trait::async_trait]
impl StateChangedSubscriber<Profiles> for ClashRuntimeConfigService {
    fn name(&self) -> &str {
        SERVICE_NAME
    }

    async fn migrate(
        &self,
        _prev_state: Option<Profiles>,
        new_state: Profiles,
    ) -> Result<(), anyhow::Error> {
        let clash_config = self.resolve_clash_config()?;
        let nyanpasu_config = self.resolve_nyanpasu_config()?;
        let runtime = self
            .derive_runtime(&new_state, &clash_config, &nyanpasu_config)
            .await?;
        self.upsert(runtime).await
    }

    async fn rollback(
        &self,
        _prev_state: Option<Profiles>,
        _new_state: Profiles,
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
        _prev_state: Option<ClashConfig>,
        new_state: ClashConfig,
    ) -> Result<(), anyhow::Error> {
        let profiles = self.resolve_profiles()?;
        let nyanpasu_config = self.resolve_nyanpasu_config()?;
        let runtime = self
            .derive_runtime(&profiles, &new_state, &nyanpasu_config)
            .await?;
        self.upsert(runtime).await
    }

    async fn rollback(
        &self,
        _prev_state: Option<ClashConfig>,
        _new_state: ClashConfig,
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
        _prev_state: Option<NyanpasuAppConfig>,
        new_state: NyanpasuAppConfig,
    ) -> Result<(), anyhow::Error> {
        let profiles = self.resolve_profiles()?;
        let clash_config = self.resolve_clash_config()?;
        let runtime = self
            .derive_runtime(&profiles, &clash_config, &new_state)
            .await?;
        self.upsert(runtime).await
    }

    async fn rollback(
        &self,
        _prev_state: Option<NyanpasuAppConfig>,
        _new_state: NyanpasuAppConfig,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

// -- Patch types --

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
/// The payload for patching the runtime config
pub enum PatchPayload {
    /// The specific patch config, might be handled by the service itself
    Specific(PatchRuntimeConfig),
    /// The untyped patch config, might include some keys were triggered by the chain
    Untyped(Mapping),
}

// -- Private helpers --

/// Extract the scoped chain UIDs from a profile.
/// Only Local and Remote profiles have chains; Merge and Script do not.
fn get_profile_chain(profile: &Profile) -> &[String] {
    match profile {
        Profile::Local(p) => &p.chain,
        Profile::Remote(p) => &p.chain,
        Profile::Merge(_) | Profile::Script(_) => &[],
    }
}

/// Intermediate owned data for a loaded profile, used to bridge lifetimes
/// between file I/O (owned Mapping) and `PartialProfileItem` (borrowed references).
struct LoadedProfile<'c> {
    profile_id: String,
    config: Mapping,
    scoped_chain: Vec<ProfileContentGuard<'c>>,
}

impl ClashRuntimeConfigService {
    pub fn new(
        clash_config_service: Arc<ClashConfigService>,
        profiles_service: Arc<ProfilesService>,
        nyanpasu_config_service: Arc<NyanpasuAppConfigService>,
        lkg_path: Utf8PathBuf,
    ) -> Self {
        Self {
            clash_config_service,
            profiles_service,
            nyanpasu_config_service,
            runtime: Arc::new(RwLock::new(WeakPersistentStateManager::new(
                Some("# Clash Nyanpasu Runtime LKG - Do not edit manually".to_string()),
                lkg_path,
                StateCoordinator::new(),
                YamlFormat,
            ))),
        }
    }

    // -- Source state resolution (MVCC snapshot for fan-in deadlock avoidance) --

    fn resolve_profiles(&self) -> Result<Arc<Profiles>, anyhow::Error> {
        self.profiles_service
            .snapshot()
            .context("profiles state not available")
    }

    fn resolve_clash_config(&self) -> Result<Arc<ClashConfig>, anyhow::Error> {
        self.clash_config_service
            .snapshot()
            .context("clash config not available")
    }

    fn resolve_nyanpasu_config(&self) -> Result<Arc<NyanpasuAppConfig>, anyhow::Error> {
        self.nyanpasu_config_service
            .snapshot()
            .context("nyanpasu config not available")
    }

    // -- Runtime derivation --

    /// Derive the full runtime state from the three source configs.
    ///
    /// This loads profile content files from disk, runs the enhance pipeline,
    /// and returns the final `ClashRuntimeState`.
    async fn derive_runtime(
        &self,
        profiles: &Profiles,
        clash_config: &ClashConfig,
        nyanpasu_config: &NyanpasuAppConfig,
    ) -> Result<ClashRuntimeState, anyhow::Error> {
        if profiles.current.is_empty() {
            return Ok(ClashRuntimeState::default());
        }

        let valid_fields = profiles.valid.iter().cloned().collect::<BTreeSet<_>>();

        // Load all profile data (owned values)
        let mut loaded_profiles: Vec<LoadedProfile<'_>> =
            Vec::with_capacity(profiles.current.len());
        for uid in &profiles.current {
            let profile = profiles
                .get_item(uid)
                .with_context(|| format!("selected profile not found: {uid}"))?;

            if matches!(profile, Profile::Script(_)) {
                anyhow::bail!("script profile cannot be selected as runtime source: {uid}");
            }

            let content_guard = profile
                .load_content()
                .await
                .with_context(|| format!("failed to load content for profile: {uid}"))?;

            // Parse YAML mapping from profile content
            let config = {
                let mut value: serde_yaml::Value =
                    serde_yaml::from_slice(&content_guard.content)
                        .with_context(|| format!("failed to parse profile YAML: {uid}"))?;
                value
                    .apply_merge()
                    .with_context(|| format!("failed to apply YAML merge for: {uid}"))?;
                value
                    .as_mapping()
                    .cloned()
                    .with_context(|| format!("profile content is not a YAML mapping: {uid}"))?
            };

            // Load scoped chain
            let chain_uids = get_profile_chain(profile);
            let mut scoped_chain = Vec::with_capacity(chain_uids.len());
            for chain_uid in chain_uids {
                let chain_profile = profiles
                    .get_item(chain_uid)
                    .with_context(|| format!("scoped chain profile not found: {chain_uid}"))?;
                scoped_chain.push(chain_profile.load_content().await.with_context(|| {
                    format!("failed to load scoped chain content: {chain_uid}")
                })?);
            }

            loaded_profiles.push(LoadedProfile {
                profile_id: uid.clone(),
                config,
                scoped_chain,
            });
        }

        // Load global chain
        let mut global_chain = Vec::with_capacity(profiles.chain.len());
        for uid in &profiles.chain {
            let profile = profiles
                .get_item(uid)
                .with_context(|| format!("global chain profile not found: {uid}"))?;
            global_chain.push(
                profile
                    .load_content()
                    .await
                    .with_context(|| format!("failed to load global chain content: {uid}"))?,
            );
        }

        // Build borrowed PartialProfileItem references from owned data
        let partial_items: Vec<PartialProfileItem<'_, '_>> = loaded_profiles
            .iter()
            .map(|loaded| PartialProfileItem {
                profile_id: loaded.profile_id.clone(),
                profile_config: &loaded.config,
                scoped_chain: &loaded.scoped_chain,
            })
            .collect();

        Self::generate_runtime_config(
            nyanpasu_config,
            clash_config,
            &valid_fields,
            &partial_items,
            &global_chain,
        )
        .await
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

    /// Try to load the LastGoodKnown snapshot from disk (for bootstrap fallback).
    /// Returns None if the file doesn't exist or is corrupted.
    pub async fn try_load_snapshot(&self) -> Option<ClashRuntimeState> {
        self.runtime.read().await.try_load_snapshot().await
    }

    /// Upsert runtime state via WeakPersistentStateManager.
    /// State is committed in-memory; file persistence is advisory.
    pub async fn upsert(&self, state: ClashRuntimeState) -> Result<(), anyhow::Error> {
        let mut runtime = self.runtime.write().await;
        runtime
            .upsert_with_context(state)
            .await
            .map_err(|e| anyhow::anyhow!("failed to upsert runtime state: {e}"))?;
        Ok(())
    }

    /// Patch the current runtime config with a partial update.
    pub async fn patch_runtime_config(&self, patch: PatchPayload) -> Result<(), anyhow::Error> {
        let mut runtime = self.runtime.write().await;
        let mut state = (*runtime.current_state().context("no runtime state found")?).clone();
        match &patch {
            PatchPayload::Specific(p) => {
                let mapping = serde_yaml::to_value(p)?
                    .as_mapping()
                    .cloned()
                    .unwrap_or_default();
                crate::utils::yaml::apply_overrides(&mut state.config, &mapping);
            }
            PatchPayload::Untyped(mapping) => {
                crate::utils::yaml::apply_overrides(&mut state.config, mapping);
            }
        }
        runtime
            .upsert_with_context(state)
            .await
            .map_err(|e| anyhow::anyhow!("failed to upsert patched runtime state: {e}"))?;
        Ok(())
    }

    /// Get the current runtime state (Context-first for transactional consistency).
    pub async fn current_state(&self) -> Option<Arc<ClashRuntimeState>> {
        if let Some(state) = Context::get::<ClashRuntimeState>() {
            return Some(Arc::new(state));
        }
        self.runtime.read().await.current_state()
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

    /// Configure the runtime state coordinator (e.g. to register subscribers)
    /// without exposing the raw manager lock.
    pub async fn configure_state_coordinator(
        &self,
        f: impl FnOnce(&mut StateCoordinator<ClashRuntimeState>),
    ) {
        let mut runtime = self.runtime.write().await;
        f(runtime.state_coordinator_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhance::PostProcessingOutput;
    use tempfile::tempdir;

    fn make_test_runtime_state() -> ClashRuntimeState {
        let mut config = Mapping::new();
        config.insert("mixed-port".into(), 7890.into());
        config.insert("external-controller".into(), "127.0.0.1:9090".into());
        ClashRuntimeState {
            config,
            exists_keys: BTreeSet::from(["mixed-port".to_string()]),
            postprocessing_output: PostProcessingOutput::default(),
            snapshots: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_persists_to_lkg_file() {
        let temp = tempdir().unwrap();
        let lkg_path = Utf8PathBuf::from_path_buf(temp.path().join("runtime-lkg.yaml")).unwrap();

        let mut manager = WeakPersistentStateManager::new(
            None,
            lkg_path.clone(),
            StateCoordinator::<ClashRuntimeState>::new(),
            YamlFormat,
        );

        let state = make_test_runtime_state();
        manager.upsert(state).await.unwrap();

        assert!(manager.current_state().is_some());
        let current = manager.current_state().unwrap();
        assert_eq!(current.get_proxy_mixed_port(), Some(7890));
        assert!(lkg_path.exists());
    }

    #[tokio::test]
    async fn test_try_load_snapshot_returns_persisted_state() {
        let temp = tempdir().unwrap();
        let lkg_path = Utf8PathBuf::from_path_buf(temp.path().join("runtime-lkg.yaml")).unwrap();

        // Write state
        {
            let mut manager = WeakPersistentStateManager::new(
                None,
                lkg_path.clone(),
                StateCoordinator::<ClashRuntimeState>::new(),
                YamlFormat,
            );
            manager.upsert(make_test_runtime_state()).await.unwrap();
        }

        // Load snapshot with fresh manager
        let manager = WeakPersistentStateManager::<ClashRuntimeState>::new(
            None,
            lkg_path,
            StateCoordinator::new(),
            YamlFormat,
        );
        let snapshot = manager.try_load_snapshot().await;
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().get_proxy_mixed_port(), Some(7890));
    }

    #[tokio::test]
    async fn test_try_load_snapshot_returns_none_when_no_file() {
        let temp = tempdir().unwrap();
        let lkg_path = Utf8PathBuf::from_path_buf(temp.path().join("nonexistent.yaml")).unwrap();

        let manager = WeakPersistentStateManager::<ClashRuntimeState>::new(
            None,
            lkg_path,
            StateCoordinator::new(),
            YamlFormat,
        );
        assert!(manager.try_load_snapshot().await.is_none());
    }

    #[tokio::test]
    async fn test_upsert_with_unreachable_path_still_commits() {
        let mut manager = WeakPersistentStateManager::<ClashRuntimeState>::new(
            None,
            Utf8PathBuf::from("/__nonexistent__/lkg.yaml"),
            StateCoordinator::new(),
            YamlFormat,
        );

        let state = make_test_runtime_state();
        manager.upsert(state).await.unwrap();
        assert!(manager.current_state().is_some());
    }

    #[tokio::test]
    async fn test_default_runtime_state_for_empty_profiles() {
        let default = ClashRuntimeState::default();
        assert!(default.config.is_empty());
        assert!(default.exists_keys.is_empty());
        assert!(default.get_proxy_mixed_port().is_none());
    }
}
