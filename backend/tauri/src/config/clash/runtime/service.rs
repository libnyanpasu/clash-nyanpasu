const SERVICE_NAME: &str = "ClashRuntimeConfigService";

use crate::{
    config::ClashRuntimeConfig,
    core::state_v2::{SimpleStateManager, StateCoordinator},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
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

    /// Get the client info from the runtime config
    pub fn get_client_info(&self) -> Option<ClashInfo> {
        let config = self.runtime.current_state()?;
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
