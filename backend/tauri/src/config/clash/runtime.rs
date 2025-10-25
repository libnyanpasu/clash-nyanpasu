use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{
    core::state_v2::{SimpleStateManager, StateChangedSubscriber, StateCoordinator},
    enhance::PostProcessingOutput,
};

const SERVICE_NAME: &str = "ClashRuntimeConfigService";

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
pub struct ClashInfo {
    /// clash core port
    pub port: u16,
    /// same as `external-controller`
    pub server: String,
    /// clash secret
    pub secret: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct PatchRuntimeConfig {
    #[serde(default)]
    pub allow_lan: Option<bool>,
    #[serde(default)]
    pub ipv6: Option<bool>,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct ClashRuntimeConfig {
    pub config: Option<Mapping>,
    // 记录在配置中（包括merge和script生成的）出现过的keys
    // 这些keys不一定都生效
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}

#[derive(Clone)]
pub struct ClashRuntimeConfigService {
    runtime: Arc<SimpleStateManager<ClashRuntimeConfig>>,
}

impl ClashRuntimeConfigService {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(SimpleStateManager::new(StateCoordinator::new())),
        }
    }

    pub async fn patch(&self, patch: ClashRuntimeConfig) -> Result<(), anyhow::Error> {
        // tracing::debug!("patching runtime config: {:?}", patch);
        // if let Some(config) = self.config.as_mut() {
        //     let patch_config: PatchRuntimeConfig =
        //         serde_yaml::from_value(serde_yaml::Value::Mapping(patch.clone()))
        //             .unwrap_or_default();

        //     [
        //         (
        //             "allow-lan",
        //             patch_config.allow_lan.map(serde_yaml::Value::Bool),
        //         ),
        //         ("ipv6", patch_config.ipv6.map(serde_yaml::Value::Bool)),
        //         (
        //             "log-level",
        //             patch_config.log_level.map(serde_yaml::Value::String),
        //         ),
        //         ("mode", patch_config.mode.map(serde_yaml::Value::String)),
        //     ]
        //     .into_iter()
        //     .filter_map(|(key, value)| value.map(|v| (key.into(), v)))
        //     .for_each(|(k, v)| {
        //         config.insert(k, v);
        //     });
        // }
        Ok(())
    }
}
