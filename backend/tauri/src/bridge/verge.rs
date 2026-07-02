use crate::{
    config::{Config, IVerge, nyanpasu as legacy_app},
    state::mirror::VergeLegacyBridge,
};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;

pub struct LegacyVergeBridge;

impl VergeLegacyBridge for LegacyVergeBridge {
    fn mirror(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
        // TODO(actor-migration): compatibility bridge for legacy Config::verge().
        // Reason: legacy readers still consume Config::verge() while typed actors are introduced.
        // Remove when get_verge_config and direct Config::verge() readers use typed facade data.
        let verge = Config::verge();
        let mut draft = verge.draft();
        mirror_application_fields(&mut draft, snap)?;
        drop(draft);
        verge.apply();
        Ok(())
    }

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        let legacy = Config::verge().data().clone();
        application_from_legacy(&legacy)
    }
}

fn application_from_legacy(legacy: &IVerge) -> anyhow::Result<NyanpasuAppConfig> {
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

fn mirror_application_fields(draft: &mut IVerge, snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
    draft.app_singleton_port = Some(snap.app_singleton_port);
    draft.app_log_level = Some(super::yaml_convert(&snap.app_log_level)?);
    draft.language = Some(super::yaml_convert(&snap.language)?);
    draft.theme_mode = Some(super::yaml_convert(&snap.theme_mode)?);
    draft.traffic_graph = Some(snap.traffic_graph);
    draft.enable_memory_usage = Some(snap.enable_memory_usage);
    draft.lighten_animation_effects = Some(snap.lighten_animation_effects);
    draft.enable_service_mode = Some(snap.enable_service_mode);
    draft.enable_auto_launch = Some(snap.enable_auto_launch);
    draft.enable_silent_start = Some(snap.enable_silent_start);
    draft.enable_system_proxy = Some(snap.enable_system_proxy);
    draft.enable_proxy_guard = Some(snap.enable_proxy_guard);
    draft.system_proxy_bypass = Some(snap.system_proxy_bypass.clone());
    draft.proxy_guard_interval = Some(snap.proxy_guard_interval);
    draft.theme_color = Some(super::yaml_convert(&snap.theme_color)?);
    draft.clash_core = Some(super::yaml_convert(&snap.core)?);
    draft.hotkeys = Some(snap.hotkeys.clone());
    draft.default_latency_test = Some(snap.default_latency_test.clone());
    draft.enable_builtin_enhanced = Some(snap.enable_builtin_enhanced);
    draft.proxy_layout_column = Some(snap.proxy_layout_column);
    draft.max_log_files = Some(snap.max_log_files);
    draft.enable_auto_check_update = Some(snap.enable_auto_check_update);
    draft.clash_tray_selector = Some(super::yaml_convert(&snap.tray_selector_mode)?);
    draft.always_on_top = Some(snap.always_on_top);
    draft.network_statistic_widget = Some(network_widget_to_legacy(snap.network_statistic_widget));
    draft.pac_url = snap.pac_url.as_ref().map(ToString::to_string);
    draft.enable_tray_text = Some(snap.enable_tray_text);
    draft.window_type = snap.use_legacy_ui.then_some(legacy_app::WindowType::Main);
    draft.tray_menu_mode = Some(super::yaml_convert(&snap.tray_menu_mode)?);
    draft.tray_menu_close_behavior = Some(super::yaml_convert(&snap.tray_menu_close_behavior)?);
    Ok(())
}

fn network_widget_from_legacy(
    value: legacy_app::NetworkStatisticWidgetConfig,
) -> AppNetworkStatisticWidgetConfig {
    match value {
        legacy_app::NetworkStatisticWidgetConfig::Disabled => {
            AppNetworkStatisticWidgetConfig::Disabled
        }
        legacy_app::NetworkStatisticWidgetConfig::Large => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large)
        }
        legacy_app::NetworkStatisticWidgetConfig::Small => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
        }
    }
}

fn network_widget_to_legacy(
    value: AppNetworkStatisticWidgetConfig,
) -> legacy_app::NetworkStatisticWidgetConfig {
    match value {
        AppNetworkStatisticWidgetConfig::Disabled => {
            legacy_app::NetworkStatisticWidgetConfig::Disabled
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large) => {
            legacy_app::NetworkStatisticWidgetConfig::Large
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small) => {
            legacy_app::NetworkStatisticWidgetConfig::Small
        }
    }
}
