use std::{collections::BTreeSet, net::SocketAddr};

use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::enhance::PostProcessingOutput;

mod service;
mod snapshot;

pub use self::service::*;

#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub struct PatchRuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct ClashRuntimeState {
    /// Clash Runtime Config
    pub config: Mapping,
    // 记录在配置中（包括merge和script生成的）出现过的keys
    // 这些keys不一定都生效
    pub exists_keys: BTreeSet<String>,
    /// Postprocessing Output
    ///
    /// Include global and local(scoped) chain output
    ///
    /// And the advice from the postprocessing
    pub postprocessing_output: PostProcessingOutput,
    /// Config Snapshots
    pub snapshots: Option<snapshot::ConfigSnapshotState>,
}

impl Default for ClashRuntimeState {
    fn default() -> Self {
        Self {
            config: Mapping::new(),
            exists_keys: BTreeSet::new(),
            postprocessing_output: PostProcessingOutput::default(),
            snapshots: None,
        }
    }
}

impl ClashRuntimeState {
    pub fn get_proxy_mixed_port(&self) -> Option<u16> {
        self.config
            .get("mixed-port")
            .and_then(|value| value.as_u64().map(|v| v as u16))
    }

    pub fn get_external_controller_server(&self) -> Option<SocketAddr> {
        let addr_str = self
            .config
            .get("external-controller")
            .and_then(|value| value.as_str())?;
        let addr = addr_str
            .parse::<SocketAddr>()
            .inspect_err(|e| {
                tracing::error!(
                    addr = addr_str,
                    "failed to parse external controller server: {e:#?}"
                )
            })
            .ok()?;
        Some(addr)
    }

    pub fn get_secret(&self) -> Option<String> {
        self.config.get("secret").and_then(|value| match value {
            serde_yaml::Value::String(val_str) => Some(val_str.clone()),
            serde_yaml::Value::Number(val_num) => Some(val_num.to_string()),
            _ => None,
        })
    }
}
