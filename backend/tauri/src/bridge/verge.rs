use std::sync::Arc;

use crate::{
    config::{Config, Draft, IVerge, nyanpasu as legacy_app},
    state::mirror::{PreparedLegacyMirror, VergeLegacyBridge},
};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;

#[derive(Clone)]
pub struct LegacyVergeBridge {
    legacy_store: Arc<dyn LegacyVergeStore>,
}

pub(crate) trait LegacyVergeStore: Send + Sync {
    fn snapshot(&self) -> anyhow::Result<IVerge>;
    fn prepare_application(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;
}

pub(crate) struct ConfigLegacyVergeStore {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl ConfigLegacyVergeStore {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

// TODO(actor-migration): compatibility adapter for the legacy Config::verge() store.
// Reason: legacy side-effect writers and readers still use the process-wide Draft<IVerge>.
// Remove when: all IVerge fields and side effects are owned by injected typed services.
impl LegacyVergeStore for ConfigLegacyVergeStore {
    fn snapshot(&self) -> anyhow::Result<IVerge> {
        let _guard = self.legacy_lock.lock();
        Ok(Config::verge().data().clone())
    }

    fn prepare_application(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        let store = Config::verge();
        let mut projected = {
            let _guard = self.legacy_lock.lock();
            store.data().clone()
        };
        apply_app_config_to_legacy_verge(&mut projected, snap)?;
        Ok(Box::new(PreparedVergeMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            store,
            projected,
        }))
    }
}

impl LegacyVergeBridge {
    pub(crate) fn with_store(legacy_store: Arc<dyn LegacyVergeStore>) -> Self {
        Self { legacy_store }
    }
}

struct PreparedVergeMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    store: Draft<IVerge>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedVergeMirror {
    fn apply(self: Box<Self>) {
        let Self {
            legacy_lock,
            store,
            projected,
        } = *self;
        let _guard = legacy_lock.lock();
        store.apply_update(|target| apply_prepared_app_projection(target, &projected));
    }
}

impl VergeLegacyBridge for LegacyVergeBridge {
    fn prepare(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        self.legacy_store.prepare_application(snap)
    }

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        application_from_legacy(&self.legacy_store.snapshot()?)
    }
}

pub(crate) fn application_from_legacy(legacy: &IVerge) -> anyhow::Result<NyanpasuAppConfig> {
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

fn apply_prepared_app_projection(target: &mut IVerge, projected: &IVerge) {
    target.app_singleton_port = projected.app_singleton_port;
    target.app_log_level = projected.app_log_level.clone();
    target.language = projected.language.clone();
    target.theme_mode = projected.theme_mode.clone();
    target.traffic_graph = projected.traffic_graph;
    target.enable_memory_usage = projected.enable_memory_usage;
    target.lighten_animation_effects = projected.lighten_animation_effects;
    target.enable_service_mode = projected.enable_service_mode;
    target.enable_auto_launch = projected.enable_auto_launch;
    target.enable_silent_start = projected.enable_silent_start;
    target.enable_system_proxy = projected.enable_system_proxy;
    target.enable_proxy_guard = projected.enable_proxy_guard;
    target.system_proxy_bypass = projected.system_proxy_bypass.clone();
    target.proxy_guard_interval = projected.proxy_guard_interval;
    target.theme_color = projected.theme_color.clone();
    target.clash_core = projected.clash_core;
    target.hotkeys = projected.hotkeys.clone();
    target.default_latency_test = projected.default_latency_test.clone();
    target.enable_builtin_enhanced = projected.enable_builtin_enhanced;
    target.proxy_layout_column = projected.proxy_layout_column;
    target.max_log_files = projected.max_log_files;
    target.enable_auto_check_update = projected.enable_auto_check_update;
    target.clash_tray_selector = projected.clash_tray_selector;
    target.always_on_top = projected.always_on_top;
    target.network_statistic_widget = projected.network_statistic_widget;
    target.pac_url = projected.pac_url.clone();
    target.enable_tray_text = projected.enable_tray_text;
    target.window_type = projected.window_type;
    target.tray_menu_mode = projected.tray_menu_mode;
    target.tray_menu_close_behavior = projected.tray_menu_close_behavior;
}

pub(crate) fn apply_app_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &NyanpasuAppConfig,
) -> anyhow::Result<()> {
    draft.app_singleton_port = Some(snap.app_singleton_port);
    draft.app_log_level = Some(super::yaml_convert(&snap.app_log_level)?);
    draft.language = Some(super::yaml_convert(snap.language)?);
    draft.theme_mode = Some(super::yaml_convert(snap.theme_mode)?);
    draft.traffic_graph = Some(snap.traffic_graph);
    draft.enable_memory_usage = Some(snap.enable_memory_usage);
    draft.lighten_animation_effects = Some(snap.lighten_animation_effects);
    draft.enable_service_mode = Some(snap.enable_service_mode);
    draft.enable_auto_launch = Some(snap.enable_auto_launch);
    draft.enable_silent_start = Some(snap.enable_silent_start);
    draft.enable_system_proxy = Some(snap.enable_system_proxy);
    draft.enable_proxy_guard = Some(snap.enable_proxy_guard);
    draft.system_proxy_bypass = if snap.system_proxy_bypass.is_empty() {
        None
    } else {
        Some(snap.system_proxy_bypass.clone())
    };
    draft.proxy_guard_interval = Some(snap.proxy_guard_interval);
    draft.theme_color = Some(super::yaml_convert(&snap.theme_color)?);
    draft.clash_core = Some(super::yaml_convert(snap.core)?);
    draft.hotkeys = Some(snap.hotkeys.clone());
    draft.default_latency_test = Some(snap.default_latency_test.clone());
    draft.enable_builtin_enhanced = Some(snap.enable_builtin_enhanced);
    draft.proxy_layout_column = Some(snap.proxy_layout_column);
    draft.max_log_files = Some(snap.max_log_files);
    draft.enable_auto_check_update = Some(snap.enable_auto_check_update);
    draft.clash_tray_selector = Some(super::yaml_convert(snap.tray_selector_mode)?);
    draft.always_on_top = Some(snap.always_on_top);
    draft.network_statistic_widget = Some(network_widget_to_legacy(snap.network_statistic_widget));
    draft.pac_url = snap.pac_url.as_ref().map(ToString::to_string);
    draft.enable_tray_text = Some(snap.enable_tray_text);
    draft.window_type = snap.use_legacy_ui.then_some(legacy_app::WindowType::Main);
    draft.tray_menu_mode = Some(super::yaml_convert(snap.tray_menu_mode)?);
    draft.tray_menu_close_behavior = Some(super::yaml_convert(snap.tray_menu_close_behavior)?);
    Ok(())
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

fn network_widget_to_legacy(
    value: AppNetworkStatisticWidgetConfig,
) -> legacy_app::LegacyNetworkStatisticWidgetConfig {
    match value {
        AppNetworkStatisticWidgetConfig::Disabled => {
            legacy_app::LegacyNetworkStatisticWidgetConfig::Disabled
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large) => {
            legacy_app::LegacyNetworkStatisticWidgetConfig::Large
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small) => {
            legacy_app::LegacyNetworkStatisticWidgetConfig::Small
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_app_config_to_legacy_verge_maps_empty_bypass_to_none() {
        let snap = NyanpasuAppConfig::default();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(draft.system_proxy_bypass, None);
    }
    #[test]
    fn apply_app_config_to_legacy_verge_preserves_custom_bypass() {
        let mut snap = NyanpasuAppConfig::default();
        snap.system_proxy_bypass = "localhost;127.*;<local>".to_string();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(
            draft.system_proxy_bypass.as_deref(),
            Some("localhost;127.*;<local>")
        );
    }
    #[test]
    fn apply_app_config_to_legacy_verge_preserves_whitespace_only_bypass() {
        let mut snap = NyanpasuAppConfig::default();
        snap.system_proxy_bypass = " \t\r\n".to_string();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(draft.system_proxy_bypass.as_deref(), Some(" \t\r\n"));
    }
}
