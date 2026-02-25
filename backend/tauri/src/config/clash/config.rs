use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use std::net::SocketAddr;

use crate::registry::{Label, PortRegistry};

use super::{
    overrides::ClashGuardOverrides,
    partial::{ClashStrategy, ClashStrategyBuilder, PickPortError, TunStack},
};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyChangeBreakMode {
    None,
    Chain,
    #[default]
    All,
}

/// Clash Default mixed-port
pub const DEFAULT_MIXED_PORT: u16 = 7890;
/// Clash Default external-controller port
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
    #[builder(field(
        ty = "ClashStrategyBuilder",
        build = "self.strategy.build().map_err(|e| ClashConfigBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
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
    pub const MIXED_PORT_LABEL: Label = Label::from("mixed-port");
    pub const EXTERNAL_CONTROLLER_LABEL: Label = Label::from("external-controller");

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
        port_registry: &PortRegistry,
    ) -> Result<Mapping, ApplyOverridesError> {
        let port = self
            .try_pick_mixed_port(port_registry)
            .map_err(|e| ApplyOverridesError::new(e, "mixed-port"))?;
        let ctrl = self
            .try_pick_external_controller(port_registry)
            .map_err(|e| ApplyOverridesError::new(e, "external-controller"))?
            .to_string();

        let mut config = self
            .overrides
            .apply_overrides(config)
            .map_err(|e| ApplyOverridesError::new(e, "overrides"))?;

        config.insert("mixed-port".into(), port.into());
        config.insert("external-controller".into(), ctrl.into());

        Ok(config)
    }

    pub fn try_pick_mixed_port(&self, port_registry: &PortRegistry) -> Result<u16, PickPortError> {
        let ports = port_registry.get_ports_by_label(Self::MIXED_PORT_LABEL);
        let port = if !ports.is_empty() {
            ports[0]
        } else {
            let port = self.strategy.mixed_port.pick_and_try_port()?;
            port_registry.replace(port, Self::MIXED_PORT_LABEL);
            port
        };

        Ok(port)
    }

    pub fn try_pick_external_controller(
        &self,
        port_registry: &PortRegistry,
    ) -> Result<SocketAddr, PickPortError> {
        let ports = port_registry.get_ports_by_label(Self::EXTERNAL_CONTROLLER_LABEL);
        let addr = if !ports.is_empty() {
            SocketAddr::from((self.strategy.external_controller.host, ports[0]))
        } else {
            let addr = self.strategy.external_controller.try_pick()?;
            port_registry.replace(addr.port(), Self::EXTERNAL_CONTROLLER_LABEL);
            addr
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
