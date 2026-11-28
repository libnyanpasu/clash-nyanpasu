//! Which fields of each configuration document are portable.
//!
//! A field is `Portable` when it expresses the user's intent independently of
//! the machine, and `Device` when it describes one installation: ports, the
//! selected core, system integration, window behavior, secrets. A test fails
//! when a serialized field has no row, so a new field cannot ship without a
//! decision.

use serde::{Deserialize, Serialize};
use specta::Type;

/// The configuration documents that sync as portable projections.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Application,
    Clash,
    Profiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldScope {
    /// Leaves the device as part of the projection.
    Portable,
    /// Never leaves the device.
    Device,
}

use FieldScope::{Device, Portable};

/// Scope of every serialized top-level field of `application.yaml`.
pub const APPLICATION_SCOPES: &[(&str, FieldScope)] = &[
    ("app_singleton_port", Device),
    ("app_log_level", Device),
    ("core_logs", Device),
    ("language", Portable),
    ("theme_mode", Portable),
    ("traffic_graph", Portable),
    ("enable_memory_usage", Portable),
    ("lighten_animation_effects", Portable),
    ("enable_service_mode", Device),
    ("profile_file_opener", Device),
    ("enable_auto_launch", Device),
    ("enable_silent_start", Device),
    ("enable_system_proxy", Device),
    ("enable_proxy_guard", Device),
    ("system_proxy_bypass", Device),
    ("proxy_guard_interval", Device),
    ("theme_color", Portable),
    ("theme_variant", Portable),
    ("theme_contrast_level", Portable),
    ("theme_spec_version", Portable),
    ("core", Device),
    ("hotkeys", Device),
    ("default_latency_test", Portable),
    ("default_latency_timeout_ms", Portable),
    ("enable_builtin_enhanced", Portable),
    ("proxy_layout_column", Portable),
    ("max_log_files", Device),
    ("max_log_file_size", Device),
    ("enable_auto_check_update", Device),
    ("enable_auto_download_update", Device),
    ("release_channel", Device),
    ("update_sources", Portable),
    ("tray_selector_mode", Portable),
    ("always_on_top", Device),
    ("tray_menu_mode", Device),
    ("window_close", Device),
    ("network_statistic_widget", Device),
    ("network_statistic_widget_logo", Device),
    ("traffic_retention", Portable),
    // Who may reach this installation over the network is a property of the installation.
    ("remote_access", Device),
    ("enable_local_ip_probe", Portable),
    ("stun_servers", Portable),
    ("ip_lookup_providers", Portable),
    // Rules keyed by the networks this machine joins.
    ("network_automation", Device),
    ("pac_url", Device),
    ("enable_tray_text", Device),
    ("enable_tray_traffic", Portable),
    ("use_legacy_ui", Device),
    ("enable_macos_colored_icons", Device),
];

/// Scope of every serialized field of `clash-config.yaml`. `overrides` is
/// split per key; its rows are written `overrides.<key>`.
pub const CLASH_SCOPES: &[(&str, FieldScope)] = &[
    ("overrides.log-level", Portable),
    ("overrides.allow-lan", Portable),
    ("overrides.mode", Portable),
    // The controller secret of one running core.
    ("overrides.secret", Device),
    ("overrides.unified-delay", Portable),
    ("overrides.tcp-concurrent", Portable),
    ("overrides.ipv6", Portable),
    ("enable_tun_mode", Device),
    ("tun", Device),
    ("web_ui_list", Portable),
    ("enable_clash_fields", Portable),
    ("expand_include_all", Portable),
    ("external_controller", Device),
    ("clash_control_channel", Device),
    ("clash_ipc_disable_http_controller", Device),
    ("mixed_port", Device),
    ("socks_port", Device),
    ("http_port", Device),
    ("redir_port", Device),
    ("tproxy_port", Device),
    ("transparent_proxy", Device),
    ("break_connection", Portable),
    ("tun_stack", Device),
    ("tun_mips", Device),
    // `dns.listen` binds a port of this machine, and a nameserver URL may carry
    // a private resolver token; the hosts table goes with the control choice.
    ("dns_control", Device),
    ("dns", Device),
    ("hosts_control", Device),
    ("hosts", Device),
];

/// The scope of `field` in `table`, or `None` when this build does not know it.
#[must_use]
pub fn scope_of(table: &[(&str, FieldScope)], field: &str) -> Option<FieldScope> {
    table
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, scope)| *scope)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::{application::NyanpasuAppConfig, clash::config::ClashConfig};

    fn keys(value: Value) -> Vec<String> {
        let Value::Object(map) = value else {
            panic!("a configuration document serializes as a mapping");
        };
        map.into_iter()
            .flat_map(|(key, value)| match (key.as_str(), value) {
                ("overrides", Value::Object(overrides)) => overrides
                    .keys()
                    .map(|inner| format!("overrides.{inner}"))
                    .collect(),
                _ => vec![key],
            })
            .collect()
    }

    #[test]
    fn every_application_field_has_a_scope() {
        for key in keys(serde_json::to_value(NyanpasuAppConfig::default()).unwrap()) {
            assert!(
                scope_of(APPLICATION_SCOPES, &key).is_some(),
                "application field {key:?} has no sync scope"
            );
        }
    }

    #[test]
    fn every_clash_field_has_a_scope() {
        for key in keys(serde_json::to_value(ClashConfig::default()).unwrap()) {
            assert!(
                scope_of(CLASH_SCOPES, &key).is_some(),
                "clash field {key:?} has no sync scope"
            );
        }
    }

    #[test]
    fn tables_have_no_duplicate_rows() {
        for table in [APPLICATION_SCOPES, CLASH_SCOPES] {
            let mut names: Vec<_> = table.iter().map(|(name, _)| *name).collect();
            names.sort_unstable();
            let before = names.len();
            names.dedup();
            assert_eq!(names.len(), before);
        }
    }
}
