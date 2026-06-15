pub mod clash_strategy;
pub mod overrides;
pub mod tun_stack;

use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

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
#[derive(Default, Debug, Clone, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(rename_all = "snake_case")))]
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
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub socks_port: Option<PortStrategy>,

    /// HTTP Proxy Port
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub http_port: Option<PortStrategy>,

    /// 断开连接策略
    pub break_connection: BreakConnectionStrategy,

    /// Tun 堆栈选择
    pub tun_stack: TunStack,
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Patch;

    /// `socks_port`/`http_port` are `Option<PortStrategy>` originals under the
    /// struct-level `skip_serializing_none` + field-level `double_option` combo:
    /// absent keeps, explicit `null` clears, and absent is skipped on serialize.
    #[test]
    fn optional_ports_clear_keep_and_sparse_serialize() {
        let seeded = || ClashConfig {
            socks_port: Some(PortStrategy::new_allow_fallback(1080)),
            ..ClashConfig::default()
        };

        // Absent → keep.
        let keep: ClashConfigPatch =
            serde_yaml_ng::from_str("enable_tun_mode: true\n").expect("patch must deserialize");
        assert_eq!(keep.socks_port, None, "absent decodes to outer None (keep)");
        let mut cfg = seeded();
        cfg.apply(keep);
        assert!(cfg.socks_port.is_some(), "absent must keep socks_port");

        // Explicit null → clear.
        let clear: ClashConfigPatch =
            serde_yaml_ng::from_str("socks_port: null\n").expect("patch must deserialize");
        assert_eq!(
            clear.socks_port,
            Some(None),
            "null decodes to Some(None) (clear)"
        );
        let mut cfg = seeded();
        cfg.apply(clear);
        assert_eq!(cfg.socks_port, None, "explicit null must clear socks_port");

        // Sparse serialize: absent http_port must not appear.
        let mut patch = ClashConfig::new_empty_patch();
        patch.socks_port = Some(None);
        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(
            dumped.contains("socks_port: null"),
            "Some(None) -> null, got:\n{dumped}"
        );
        assert!(
            !dumped.contains("http_port"),
            "absent skipped, got:\n{dumped}"
        );
    }
}
