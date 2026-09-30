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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ClashControlChannel {
    #[default]
    PreferIpc,
    HttpOnly,
}

/// Clash Related Config
#[derive(Debug, Clone, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(default, rename_all = "snake_case")))]
#[serde(rename_all = "snake_case")]
pub struct ClashConfig {
    /// Clash Overrides config, used to patch clash config directly
    pub overrides: ClashGuardOverrides,

    /// clash tun mode
    pub enable_tun_mode: bool,

    /// web ui list
    pub web_ui_list: Vec<String>,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认关闭
    pub enable_clash_fields: bool,

    /// 在 Nyanpasu 侧按 mihomo 语义展开代理组的 `include-all*`，默认为真；
    /// 关闭时原样交给核心处理
    #[serde(default = "default_expand_include_all")]
    pub expand_include_all: bool,

    /// 外部控制器端口策略
    #[patch(nesting)]
    pub external_controller: ExternalControllerStrategy,

    #[serde(default)]
    pub clash_control_channel: ClashControlChannel,
    #[serde(default)]
    pub clash_ipc_disable_http_controller: bool,

    /// Mixed Proxy(Socks5, HTTP) Port Strategy
    #[patch(nesting)]
    pub mixed_port: PortStrategy,

    /// Socks5 Proxy Port
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    pub socks_port: Option<PortStrategy>,

    /// HTTP Proxy Port
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    pub http_port: Option<PortStrategy>,

    /// 断开连接策略
    #[patch(nesting)]
    pub break_connection: BreakConnectionStrategy,

    /// Tun 堆栈选择
    pub tun_stack: TunStack,
}

fn default_expand_include_all() -> bool {
    true
}

/// The config of a fresh install.
impl Default for ClashConfig {
    fn default() -> Self {
        Self {
            overrides: ClashGuardOverrides::default(),
            enable_tun_mode: false,
            web_ui_list: Vec::new(),
            enable_clash_fields: false,
            expand_include_all: default_expand_include_all(),
            external_controller: ExternalControllerStrategy::default(),
            clash_control_channel: ClashControlChannel::default(),
            clash_ipc_disable_http_controller: false,
            mixed_port: PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: DEFAULT_MIXED_PORT,
            },
            socks_port: None,
            http_port: None,
            break_connection: BreakConnectionStrategy::default(),
            tun_stack: TunStack::default(),
        }
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Patch;

    #[test]
    fn default_binds_the_default_mixed_port_as_is() {
        let config = ClashConfig::default();
        assert_eq!(
            config.mixed_port,
            PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: DEFAULT_MIXED_PORT,
            }
        );
        assert_eq!(config.socks_port, None);
        assert_eq!(config.http_port, None);
        assert!(!config.enable_clash_fields);
    }

    #[test]
    fn old_clash_config_defaults_control_channel_without_application_fields() {
        let mut yaml = serde_yaml_ng::to_value(ClashConfig::default()).unwrap();
        let mapping = yaml.as_mapping_mut().unwrap();
        mapping.remove("clash_control_channel");
        mapping.remove("clash_ipc_disable_http_controller");
        let config: ClashConfig = serde_yaml_ng::from_value(yaml).unwrap();
        assert_eq!(config.clash_control_channel, ClashControlChannel::PreferIpc);
        assert!(!config.clash_ipc_disable_http_controller);
        let app =
            serde_yaml_ng::to_value(crate::application::NyanpasuAppConfig::default()).unwrap();
        assert!(app.get("clash_control_channel").is_none());
        assert!(app.get("clash_ipc_disable_http_controller").is_none());
    }

    #[test]
    fn include_all_expansion_defaults_on_for_new_and_old_configs() {
        assert!(ClashConfig::default().expand_include_all);
        let mut yaml = serde_yaml_ng::to_value(ClashConfig {
            expand_include_all: false,
            ..ClashConfig::default()
        })
        .unwrap();
        yaml.as_mapping_mut().unwrap().remove("expand_include_all");
        let config: ClashConfig = serde_yaml_ng::from_value(yaml).unwrap();
        assert!(config.expand_include_all);
    }

    /// The composite fields take nested patches: a sub-field replaces only
    /// itself, and a composite the patch does not carry is left alone.
    #[test]
    fn nested_patches_apply_only_the_given_sub_fields() {
        let mut cfg = ClashConfig {
            mixed_port: PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7890,
            },
            ..ClashConfig::default()
        };
        let controller = cfg.external_controller.clone();

        let patch: ClashConfigPatch = serde_json::from_value(serde_json::json!({
            "mixed_port": { "start_port": 7891 },
            "external_controller": { "port": { "kind": "random" } },
            "break_connection": { "on_mode_change": false },
        }))
        .expect("nested patch must deserialize");
        cfg.apply(patch);

        assert_eq!(
            cfg.mixed_port,
            PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7891,
            }
        );
        assert_eq!(cfg.external_controller.host, controller.host);
        assert_eq!(
            cfg.external_controller.port,
            PortStrategy {
                kind: PortStrategyKind::Random,
                start_port: controller.port.start_port,
            }
        );
        assert_eq!(
            cfg.break_connection,
            BreakConnectionStrategy {
                on_mode_change: false,
                ..BreakConnectionStrategy::default()
            }
        );

        let before = cfg.clone();
        let unrelated: ClashConfigPatch =
            serde_json::from_value(serde_json::json!({ "enable_tun_mode": true }))
                .expect("patch must deserialize");
        cfg.apply(unrelated);
        assert_eq!(cfg.mixed_port, before.mixed_port);
        assert_eq!(cfg.external_controller, before.external_controller);
        assert_eq!(cfg.break_connection, before.break_connection);
    }

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
