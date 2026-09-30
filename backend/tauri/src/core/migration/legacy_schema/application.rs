use super::verge::{self as legacy_app, IVerge};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;

pub(super) fn application_from_legacy(legacy: &IVerge) -> anyhow::Result<NyanpasuAppConfig> {
    let mut next = NyanpasuAppConfig::default();

    if let Some(value) = legacy.app_singleton_port {
        next.app_singleton_port = value;
    }
    if let Some(value) = &legacy.app_log_level {
        next.app_log_level = super::yaml_convert(value)?;
    }
    if let Some(value) = &legacy.language
        && let Ok(value) = super::yaml_convert(value)
    {
        next.language = value;
    }
    if let Some(value) = &legacy.theme_mode
        && let Ok(value) = super::yaml_convert(value)
    {
        next.theme_mode = value;
    }
    if let Some(value) = legacy.traffic_graph {
        next.traffic_graph = value;
    }
    if let Some(value) = legacy.enable_memory_usage {
        next.enable_memory_usage = value;
    }
    if let Some(value) = legacy.lighten_animation_effects {
        next.lighten_animation_effects = value;
    }
    if let Some(value) = legacy.enable_service_mode {
        next.enable_service_mode = value;
    }
    if let Some(value) = legacy.enable_auto_launch {
        next.enable_auto_launch = value;
    }
    if let Some(value) = legacy.enable_silent_start {
        next.enable_silent_start = value;
    }
    if let Some(value) = legacy.enable_system_proxy {
        next.enable_system_proxy = value;
    }
    if let Some(value) = legacy.enable_proxy_guard {
        next.enable_proxy_guard = value;
    }
    if let Some(value) = &legacy.system_proxy_bypass {
        next.system_proxy_bypass = value.clone();
    }
    if let Some(value) = legacy.proxy_guard_interval {
        next.proxy_guard_interval = value;
    }
    if let Some(value) = &legacy.theme_color
        && let Ok(value) = super::yaml_convert(value)
    {
        next.theme_color = value;
    }
    if let Some(value) = &legacy.clash_core
        && let Ok(value) = super::yaml_convert(value)
    {
        next.core = value;
    }
    if let Some(value) = &legacy.hotkeys {
        next.hotkeys = value.clone();
    }
    if let Some(value) = &legacy.default_latency_test {
        next.default_latency_test = value.clone();
    }
    if let Some(value) = legacy.enable_builtin_enhanced {
        next.enable_builtin_enhanced = value;
    }
    if let Some(value) = legacy.proxy_layout_column {
        next.proxy_layout_column = value;
    }
    if let Some(value) = legacy.max_log_files {
        next.max_log_files = value;
    }
    if let Some(value) = legacy.enable_auto_check_update {
        next.enable_auto_check_update = value;
    }
    if let Some(value) = &legacy.clash_tray_selector
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_selector_mode = value;
    }
    if let Some(value) = legacy.always_on_top {
        next.always_on_top = value;
    }
    if let Some(value) = legacy.network_statistic_widget {
        next.network_statistic_widget = network_widget_from_legacy(value);
    }
    if let Some(value) = &legacy.pac_url
        && let Ok(value) = super::yaml_convert(value)
    {
        next.pac_url = Some(value);
    }
    if let Some(value) = legacy.enable_tray_text {
        next.enable_tray_text = value;
    }
    if let Some(value) = legacy.window_type {
        next.use_legacy_ui = matches!(value, legacy_app::WindowType::Main);
    }
    if let Some(value) = &legacy.tray_menu_mode
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_menu_mode = value;
    }
    if let Some(value) = &legacy.tray_menu_close_behavior
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_menu_close_behavior = value;
    }

    Ok(next)
}

fn network_widget_from_legacy(
    value: legacy_app::LegacyNetworkStatisticWidgetConfig,
) -> AppNetworkStatisticWidgetConfig {
    match value {
        legacy_app::LegacyNetworkStatisticWidgetConfig::Disabled => {
            AppNetworkStatisticWidgetConfig::Disabled
        }
        legacy_app::LegacyNetworkStatisticWidgetConfig::Large => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large)
        }
        legacy_app::LegacyNetworkStatisticWidgetConfig::Small => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
        }
    }
}
