//! The Clash settings a `read` caller may see.
//!
//! [`ClashConfig`] holds the controller secret and the web UI templates, which are free-form URLs a
//! user may write a credential into. [`ClashSettings`] keeps every other field, so the dashboard
//! can show the TUN state, the ports and the break-connection policy without `sensitive_read`.
//! A form that edits the secret or the web UI list reads the full config.

use serde::Serialize;
use specta::Type;

use super::{
    ClashConfig, ClashControlChannel, TransparentProxyConfig,
    clash_strategy::{BreakConnectionStrategy, ExternalControllerStrategy, PortStrategy},
    overrides::{ClashOverridesSettings, TunOverrides},
    tun_stack::{MipsConfig, TunStack},
};

/// The secret-free view of [`ClashConfig`].
///
/// The conversion destructures the config without `..`, so a new field must be classified.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct ClashSettings {
    pub overrides: ClashOverridesSettings,
    pub enable_tun_mode: bool,
    pub tun: TunOverrides,
    pub enable_clash_fields: bool,
    pub expand_include_all: bool,
    pub external_controller: ExternalControllerStrategy,
    pub clash_control_channel: ClashControlChannel,
    pub clash_ipc_disable_http_controller: bool,
    pub mixed_port: PortStrategy,
    pub socks_port: Option<PortStrategy>,
    pub http_port: Option<PortStrategy>,
    pub redir_port: Option<PortStrategy>,
    pub tproxy_port: Option<PortStrategy>,
    pub transparent_proxy: TransparentProxyConfig,
    pub break_connection: BreakConnectionStrategy,
    pub tun_stack: TunStack,
    pub tun_mips: MipsConfig,
}

impl From<&ClashConfig> for ClashSettings {
    fn from(config: &ClashConfig) -> Self {
        let ClashConfig {
            overrides,
            enable_tun_mode,
            tun,
            // URL templates a user may embed a credential in: full config only.
            web_ui_list: _,
            enable_clash_fields,
            expand_include_all,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            break_connection,
            tun_stack,
            tun_mips,
        } = config;

        Self {
            overrides: overrides.into(),
            enable_tun_mode: *enable_tun_mode,
            tun: tun.clone(),
            enable_clash_fields: *enable_clash_fields,
            expand_include_all: *expand_include_all,
            external_controller: external_controller.clone(),
            clash_control_channel: *clash_control_channel,
            clash_ipc_disable_http_controller: *clash_ipc_disable_http_controller,
            mixed_port: mixed_port.clone(),
            socks_port: socks_port.clone(),
            http_port: http_port.clone(),
            redir_port: redir_port.clone(),
            tproxy_port: tproxy_port.clone(),
            transparent_proxy: transparent_proxy.clone(),
            break_connection: break_connection.clone(),
            tun_stack: *tun_stack,
            tun_mips: tun_mips.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_secret_and_web_ui_templates_are_not_serialized() {
        let mut config = ClashConfig::default();
        let secret = config.overrides.secret().to_owned();
        config.web_ui_list = vec!["https://ui.example/#/?secret=web-ui-sentinel".to_owned()];

        let json = serde_json::to_string(&ClashSettings::from(&config)).unwrap();
        assert!(!json.contains(&secret), "{json}");
        assert!(!json.contains("web-ui-sentinel"), "{json}");
        assert!(!json.contains("secret"), "{json}");
        assert!(!json.contains("web_ui_list"), "{json}");
    }

    #[test]
    fn ordinary_fields_survive() {
        let mut config = ClashConfig::default();
        config.enable_tun_mode = true;
        let value = serde_json::to_value(ClashSettings::from(&config)).unwrap();
        assert_eq!(value["enable_tun_mode"], true);
        assert_eq!(value["overrides"]["allow-lan"], false);
        assert_eq!(value["mixed_port"]["start_port"], 7890);
    }
}
