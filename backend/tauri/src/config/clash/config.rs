use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::net::SocketAddr;

use super::{
    overrides::ClashGuardOverrides,
    partial::{ClashStrategy, PickPortError, TunStack},
};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyChangeBreakMode {
    None,
    Chain,
    #[default]
    All,
}

/// Clash 默认混合端口
pub const DEFAULT_MIXED_PORT: u16 = 7890;
/// Clash 默认外部控制器端口
#[cfg(debug_assertions)]
pub const DEFAULT_EXTERNAL_CONTROLLER_PORT: u16 = 9872;
#[cfg(not(debug_assertions))]
pub const DEFAULT_EXTERNAL_CONTROLLER_PORT: u16 = 17650;

/// Clash Related Config
#[derive(Default, Debug, Clone, Deserialize, Serialize, Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, Type))]
#[builder_struct_attr(serde_with::skip_serializing_none)]
#[serde(rename_all = "snake_case")]
pub struct ClashConfig {
    /// Clash Overrides config, used to patch clash config directly
    pub overrides: ClashGuardOverrides,

    /// clash tun mode
    pub enable_tun_mode: bool,

    /// web ui list
    pub web_ui_list: Vec<String>,

    /// 切换代理时中断连接
    /// None: 不中断
    /// Chain: 仅中断使用该代理链的连接
    /// All: 中断所有连接
    pub break_when_proxy_change: ProxyChangeBreakMode,

    /// 切换配置时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    #[builder(default = "true")]
    pub break_when_profile_change: bool,

    /// 切换模式时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    #[builder(default = "true")]
    pub break_when_mode_change: bool,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: bool,

    /// Clash 相关策略
    pub strategy: ClashStrategy,

    /// Tun 堆栈选择
    pub tun_stack: TunStack,
}

#[derive(Debug, thiserror::Error)]
#[error("apply overrides error: {e:#?}, field: {field}")]
pub struct ApplyOverridesError {
    e: anyhow::Error,
    field: &'static str,
}

impl ApplyOverridesError {
    pub fn new<E: Into<anyhow::Error>>(e: E, field: &'static str) -> Self {
        Self { e: e.into(), field }
    }
}

impl ClashConfig {
    /// Apply overrides to the config
    /// # Arguments
    ///
    /// * `config` - The config to apply overrides to
    /// * `reuse_port` - Whether to reuse the cache port
    ///
    /// # Returns
    ///
    /// The config with overrides applied
    ///
    pub fn apply_overrides(
        &self,
        config: Mapping,
        reuse_port: bool,
    ) -> Result<Mapping, ApplyOverridesError> {
        let port = self
            .try_pick_mixed_port(reuse_port)
            .map_err(|e| ApplyOverridesError::new(e, "mixed-port"))?;
        let ctrl = self
            .try_pick_external_controller(reuse_port)
            .map_err(|e| ApplyOverridesError::new(e, "external-controller"))?;

        let mut config = self
            .overrides
            .apply_overrides(config)
            .map_err(|e| ApplyOverridesError::new(e, "overrides"))?;

        config.insert("mixed-port".into(), port.into());
        config.insert("external-controller".into(), ctrl.into());

        Ok(config)
    }

    pub fn try_pick_mixed_port(&self, reuse_port: bool) -> Result<u16, PickPortError> {
        let port = if reuse_port && let Some(port) = self.strategy.mixed_port.cached_port() {
            port
        } else {
            self.strategy.mixed_port.pick_and_try_port()?
        };

        Ok(port)
    }

    pub fn try_pick_external_controller(
        &self,
        reuse_port: bool,
    ) -> Result<SocketAddr, PickPortError> {
        let addr = if reuse_port
            && let Some(addr) = self
                .strategy
                .external_controller_port
                .try_pick_with_cached_port()
        {
            addr
        } else {
            self.strategy.external_controller_port.try_pick()?
        };

        Ok(addr)
    }

    // pub fn get_tun_device_ip(&self) -> String {
    //     let config = &self.0;

    //     let ip = config
    //         .get("dns")
    //         .and_then(|value| match value {
    //             Value::Mapping(val_map) => Some(val_map.get("fake-ip-range").and_then(
    //                 |fake_ip_range| match fake_ip_range {
    //                     Value::String(ip_range_val) => Some(ip_range_val.replace("1/16", "2")),
    //                     _ => None,
    //                 },
    //             )),
    //             _ => None,
    //         })
    //         // 默认IP
    //         .unwrap_or(Some("198.18.0.2".to_string()));

    //     ip.unwrap()
    // }
}
