use derive_builder::Builder;

use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

mod clash_core;
mod logging;
mod widget;
mod i18n;
pub use clash_core::*;
pub use logging::*;
pub use widget::*;
pub use i18n::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxiesSelectorMode {
    Hidden,
    #[default]
    Normal,
    Submenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

/// ### `verge.yaml` schema
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, specta::Type))]
// #[builder_update(patch_fn = "apply")]
// #[serde(flatten)]
//     #[builder(field(
//         ty = "ProfileSharedBuilder",
//         build = "self.shared.build(&PROFILE_TYPE).map_err(|e| LocalProfileBuilderError::from(e.to_string()))?"
//     ))]
//     #[builder_field_attr(serde(flatten))]
//     #[builder_update(nested)]
pub struct NyanpasuAppConfig {
    /// app listening port for app singleton
    pub app_singleton_port: u16,

    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: LoggingLevel,

    // i18n
    #[builder(default = "default_i18n_language()")]
    pub language: I18nLanguage,

    /// `light` or `dark` or `system`
    pub theme_mode: ThemeMode,

    /// enable traffic graph
    #[builder(default = "true")]
    pub traffic_graph: bool,

    /// show memory info (only for Clash Meta)
    #[builder(default = "true")]
    pub enable_memory_usage: bool,

    /// global ui framer motion effects
    pub lighten_animation_effects: bool,

    /// service mode
    pub enable_service_mode: bool,

    /// can the app auto startup
    pub enable_auto_launch: bool,

    /// not show the window on launch
    pub enable_silent_start: bool,

    /// set system proxy
    pub enable_system_proxy: bool,

    /// enable proxy guard
    pub enable_proxy_guard: bool,

    /// set system proxy bypass
    pub system_proxy_bypass: String,

    /// proxy guard interval
    #[builder_field_attr(serde(alias = "proxy_guard_duration"))]
    #[builder(default = "30")]
    pub proxy_guard_interval: u64,

    /// theme setting
    #[specta(type = String)]
    #[builder_field_attr(specta(type = String))]
    pub theme_color: CssColor,

    /// clash core path
    #[builder_field_attr(serde(alias = "clash_core"))]
    pub core: ClashCore,

    /// hotkey map
    /// format: {func},{key}
    pub hotkeys: Vec<String>,

    /// 默认的延迟测试连接
    pub default_latency_test: String,

    /// 是否使用内部的脚本支持，默认为真
    pub enable_builtin_enhanced: bool,

    /// proxy 页面布局 列数
    pub proxy_layout_column: i32,

    /// 日记轮转时间，单位：天
    #[builder(default = "7")]
    pub max_log_files: usize,

    /// Check update when app launch
    #[builder(default = "true")]
    pub enable_auto_check_update: bool,

    /// 是否启用代理托盘选择
    #[builder_field_attr(serde(alias = "clash_tray_selector"))]
    pub tray_selector_mode: ProxiesSelectorMode,

    /// 是否窗口置顶
    pub always_on_top: bool,

    /// 是否启用网络统计信息浮窗
    #[builder_field_attr(serde(skip_serializing_if = "Option::is_none"))]
    pub network_statistic_widget: NetworkStatisticWidgetConfig,

    /// PAC URL for automatic proxy configuration
    /// This field is used to set PAC proxy without exposing it to the frontend UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pac_url: Option<Url>,

    /// enable tray text display on Linux systems
    /// When enabled, shows proxy and TUN mode status as text next to the tray icon
    /// When disabled, only shows status via icon changes (prevents text display issues on Wayland)
    pub enable_tray_text: bool,

    /// enable traffic information display in system tray
    /// When enabled, shows upload/download speeds in the tray tooltip (macOS/Windows) or title (Linux)
    pub enable_tray_traffic: bool,

    /// Use legacy UI (original UI at "/" route)
    /// When true, opens legacy window; when false, opens new main window
    #[builder(default = "true")]
    pub use_legacy_ui: bool,

    /// enable colored tray icons on macOS
    /// When enabled, uses colored icons instead of template icons to show proxy status
    /// When disabled, uses system template icons that adapt to light/dark mode
    #[cfg(target_os = "macos")]
    pub enable_macos_colored_icons: bool,
}

impl Default for NyanpasuAppConfig {
    fn default() -> Self {
        Self {
            app_singleton_port: todo!(),
            app_log_level: todo!(),
            language: todo!(),
            theme_mode: todo!(),
            traffic_graph: todo!(),
            enable_memory_usage: todo!(),
            lighten_animation_effects: todo!(),
            enable_service_mode: todo!(),
            enable_auto_launch: todo!(),
            enable_silent_start: todo!(),
            enable_system_proxy: todo!(),
            enable_proxy_guard: todo!(),
            system_proxy_bypass: todo!(),
            proxy_guard_interval: todo!(),
            theme_color: todo!(),
            core: todo!(),
            hotkeys: todo!(),
            default_latency_test: todo!(),
            enable_builtin_enhanced: todo!(),
            proxy_layout_column: todo!(),
            max_log_files: todo!(),
            enable_auto_check_update: todo!(),
            tray_selector_mode: todo!(),
            always_on_top: todo!(),
            network_statistic_widget: todo!(),
            pac_url: todo!(),
            enable_tray_text: todo!(),
            enable_tray_traffic: todo!(),
            use_legacy_ui: todo!(),
        }
    }
}

