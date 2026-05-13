use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyChangeBreakMode {
    Off,
    /// 仅中断当前使用的代理组的连接
    ProxyGroup,
    /// 中断所有连接
    #[default]
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Builder)]
pub struct BreakConnectionStrategy {
    /// 切换代理时中断连接
    pub on_proxy_change: ProxyChangeBreakMode,

    /// 切换配置时中断连接
    pub on_profile_change: bool,

    /// 切换模式时中断连接
    pub on_mode_change: bool,
}

impl Default for BreakConnectionStrategy {
    fn default() -> Self {
        Self {
            on_proxy_change: ProxyChangeBreakMode::default(),
            on_profile_change: true,
            on_mode_change: true,
        }
    }
}
