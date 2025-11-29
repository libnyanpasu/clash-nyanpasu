use derive_builder::Builder;

use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use specta::Type;

mod partial;
mod service;
pub use partial::*;
pub use service::*;

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
#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type, Builder, BuilderUpdate)]
#[builder(default, derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
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
    pub app_log_level: partial::LoggingLevel,

    // i18n
    #[builder(default = "Self::default_language()")]
    pub language: String,

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
    pub theme_color: String,

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

    /// 日志清理
    /// 分钟数； 0 为不清理
    #[deprecated(note = "use `max_log_files` instead")]
    pub auto_log_clean: usize,

    /// 日记轮转时间，单位：天
    #[builder(default = "7")]
    pub max_log_files: usize,
    /// window size and position
    #[deprecated(note = "use `window_size_state` instead")]
    #[builder_field_attr(serde(skip_serializing_if = "Option::is_none"))]
    #[builder(setter(strip_option))]
    pub window_size_position: Vec<f64>,

    #[builder_field_attr(serde(skip_serializing_if = "Option::is_none"))]
    #[builder(setter(strip_option))]
    pub window_size_state: Option<WindowState>,

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
    pub pac_url: Option<String>,

    /// enable tray text display on Linux systems
    /// When enabled, shows proxy and TUN mode status as text next to the tray icon
    /// When disabled, only shows status via icon changes (prevents text display issues on Wayland)
    pub enable_tray_text: bool,

    /// enable traffic information display in system tray
    /// When enabled, shows upload/download speeds in the tray tooltip (macOS/Windows) or title (Linux)
    pub enable_tray_traffic: bool,

    /// enable colored tray icons on macOS
    /// When enabled, uses colored icons instead of template icons to show proxy status
    /// When disabled, uses system template icons that adapt to light/dark mode
    #[cfg(target_os = "macos")]
    pub enable_macos_colored_icons: bool,
}

impl NyanpasuAppConfigBuilder {
    fn default_language() -> String {
        let locale = crate::utils::help::get_system_locale();
        crate::utils::help::mapping_to_i18n_key(&locale).into()
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
    pub fullscreen: bool,
}
