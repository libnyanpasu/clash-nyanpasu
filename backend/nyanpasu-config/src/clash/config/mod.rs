pub mod clash_strategy;
pub mod overrides;
pub mod tun_stack;

use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;

use clash_strategy::*;
use overrides::*;
use tun_stack::*;

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

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: bool,

    /// 外部控制器端口策略
    pub external_controller: ExternalControllerStrategy,

    /// Mixed Proxy(Socks5, HTTP) Port Strategy
    pub mixed_port: PortStrategy,

    /// Socks5 Proxy Port
    pub socks_port: Option<PortStrategy>,

    /// HTTP Proxy Port
    pub http_port: Option<PortStrategy>,

    /// 断开连接策略
    pub break_connection: BreakConnectionStrategy,

    /// Tun 堆栈选择
    pub tun_stack: TunStack,
}
