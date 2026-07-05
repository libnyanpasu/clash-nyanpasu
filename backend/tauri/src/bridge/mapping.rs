#![cfg_attr(not(test), allow(dead_code))]

/// Phase 0 ownership metadata only. These entries document legacy `IVerge`
/// ownership and must not be used as production conversion logic until the
/// typed bridge phase adds explicit compatibility tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFieldOwner {
    Application,
    Session,
    Clash,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IVergeFieldMapping {
    pub legacy: &'static str,
    pub owner: LegacyFieldOwner,
    pub target: &'static str,
}

pub const IVERGE_FIELD_MAPPING: &[IVergeFieldMapping] = &[
    IVergeFieldMapping {
        legacy: "app_singleton_port",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.app_singleton_port",
    },
    IVergeFieldMapping {
        legacy: "app_log_level",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.app_log_level",
    },
    IVergeFieldMapping {
        legacy: "language",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.language",
    },
    IVergeFieldMapping {
        legacy: "theme_mode",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.theme_mode",
    },
    IVergeFieldMapping {
        legacy: "traffic_graph",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.traffic_graph",
    },
    IVergeFieldMapping {
        legacy: "enable_memory_usage",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_memory_usage",
    },
    IVergeFieldMapping {
        legacy: "lighten_animation_effects",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.lighten_animation_effects",
    },
    IVergeFieldMapping {
        legacy: "enable_tun_mode",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.enable_tun_mode",
    },
    IVergeFieldMapping {
        legacy: "enable_service_mode",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_service_mode",
    },
    IVergeFieldMapping {
        legacy: "enable_auto_launch",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_auto_launch",
    },
    IVergeFieldMapping {
        legacy: "enable_silent_start",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_silent_start",
    },
    IVergeFieldMapping {
        legacy: "enable_system_proxy",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_system_proxy",
    },
    IVergeFieldMapping {
        legacy: "enable_proxy_guard",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_proxy_guard",
    },
    IVergeFieldMapping {
        legacy: "system_proxy_bypass",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.system_proxy_bypass",
    },
    IVergeFieldMapping {
        legacy: "proxy_guard_interval",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.proxy_guard_interval",
    },
    IVergeFieldMapping {
        legacy: "theme_color",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.theme_color",
    },
    IVergeFieldMapping {
        legacy: "web_ui_list",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.web_ui_list",
    },
    IVergeFieldMapping {
        legacy: "clash_core",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.core",
    },
    IVergeFieldMapping {
        legacy: "hotkeys",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.hotkeys",
    },
    IVergeFieldMapping {
        legacy: "auto_close_connection",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.break_connection (deprecated backfill)",
    },
    IVergeFieldMapping {
        legacy: "break_when_proxy_change",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.break_connection",
    },
    IVergeFieldMapping {
        legacy: "break_when_profile_change",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.break_connection",
    },
    IVergeFieldMapping {
        legacy: "break_when_mode_change",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.break_connection",
    },
    IVergeFieldMapping {
        legacy: "default_latency_test",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.default_latency_test",
    },
    IVergeFieldMapping {
        legacy: "enable_clash_fields",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.enable_clash_fields",
    },
    IVergeFieldMapping {
        legacy: "enable_builtin_enhanced",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_builtin_enhanced",
    },
    IVergeFieldMapping {
        legacy: "proxy_layout_column",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.proxy_layout_column",
    },
    IVergeFieldMapping {
        legacy: "auto_log_clean",
        owner: LegacyFieldOwner::Discard,
        target: "deprecated: superseded by max_log_files",
    },
    IVergeFieldMapping {
        legacy: "max_log_files",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.max_log_files",
    },
    IVergeFieldMapping {
        legacy: "window_size_position",
        owner: LegacyFieldOwner::Session,
        target: "PersistentState.window_state",
    },
    IVergeFieldMapping {
        legacy: "window_size_state",
        owner: LegacyFieldOwner::Session,
        target: "PersistentState.window_state",
    },
    IVergeFieldMapping {
        legacy: "enable_random_port",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.mixed_port",
    },
    IVergeFieldMapping {
        legacy: "verge_mixed_port",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.mixed_port",
    },
    IVergeFieldMapping {
        legacy: "enable_auto_check_update",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_auto_check_update",
    },
    IVergeFieldMapping {
        legacy: "clash_strategy",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig external_controller and port strategies",
    },
    IVergeFieldMapping {
        legacy: "clash_tray_selector",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.tray_selector_mode",
    },
    IVergeFieldMapping {
        legacy: "always_on_top",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.always_on_top",
    },
    IVergeFieldMapping {
        legacy: "tun_stack",
        owner: LegacyFieldOwner::Clash,
        target: "nyanpasu_config::clash::config::ClashConfig.tun_stack",
    },
    IVergeFieldMapping {
        legacy: "network_statistic_widget",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.network_statistic_widget",
    },
    IVergeFieldMapping {
        legacy: "pac_url",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.pac_url",
    },
    IVergeFieldMapping {
        legacy: "enable_tray_text",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.enable_tray_text",
    },
    IVergeFieldMapping {
        legacy: "window_type",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.use_legacy_ui",
    },
    IVergeFieldMapping {
        legacy: "tray_menu_mode",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.tray_menu_mode",
    },
    IVergeFieldMapping {
        legacy: "tray_menu_close_behavior",
        owner: LegacyFieldOwner::Application,
        target: "NyanpasuAppConfig.tray_menu_close_behavior",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const EXPECTED_IVERGE_FIELDS: &[&str] = &[
        "app_singleton_port",
        "app_log_level",
        "language",
        "theme_mode",
        "traffic_graph",
        "enable_memory_usage",
        "lighten_animation_effects",
        "enable_tun_mode",
        "enable_service_mode",
        "enable_auto_launch",
        "enable_silent_start",
        "enable_system_proxy",
        "enable_proxy_guard",
        "system_proxy_bypass",
        "proxy_guard_interval",
        "theme_color",
        "web_ui_list",
        "clash_core",
        "hotkeys",
        "auto_close_connection",
        "break_when_proxy_change",
        "break_when_profile_change",
        "break_when_mode_change",
        "default_latency_test",
        "enable_clash_fields",
        "enable_builtin_enhanced",
        "proxy_layout_column",
        "auto_log_clean",
        "max_log_files",
        "window_size_position",
        "window_size_state",
        "enable_random_port",
        "verge_mixed_port",
        "enable_auto_check_update",
        "clash_strategy",
        "clash_tray_selector",
        "always_on_top",
        "tun_stack",
        "network_statistic_widget",
        "pac_url",
        "enable_tray_text",
        "window_type",
        "tray_menu_mode",
        "tray_menu_close_behavior",
    ];

    #[test]
    fn iverge_mapping_declares_every_legacy_field() {
        let declared = IVERGE_FIELD_MAPPING
            .iter()
            .map(|entry| entry.legacy)
            .collect::<BTreeSet<_>>();

        let expected = EXPECTED_IVERGE_FIELDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(declared, expected);
    }

    #[test]
    fn iverge_mapping_has_no_duplicate_fields() {
        let declared = IVERGE_FIELD_MAPPING
            .iter()
            .map(|entry| entry.legacy)
            .collect::<BTreeSet<_>>();

        assert_eq!(declared.len(), IVERGE_FIELD_MAPPING.len());
    }

    #[test]
    fn iverge_mapping_keeps_all_targets_explicit() {
        for entry in IVERGE_FIELD_MAPPING {
            assert!(!entry.legacy.is_empty());
            assert!(!entry.target.is_empty());
        }
    }
}
