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
pub enum TunStack {
    System,
    #[default]
    Gvisor,
    Mixed,
}

impl AsRef<str> for TunStack {
    fn as_ref(&self) -> &str {
        match self {
            TunStack::System => "system",
            TunStack::Gvisor => "gvisor",
            TunStack::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum BreakWhenProxyChange {
    None,
    Chain,
    #[default]
    All,
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
// TODO: use new managedState and builder pattern instead
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

    /// clash tun mode
    pub enable_tun_mode: bool,

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

    /// web ui list
    pub web_ui_list: Vec<String>,

    /// clash core path
    pub clash_core: ClashCore,

    /// hotkey map
    /// format: {func},{key}
    pub hotkeys: Vec<String>,

    /// 切换代理时自动关闭连接 (已弃用)
    #[deprecated(note = "use `break_when_proxy_change` instead")]
    pub auto_close_connection: bool,

    /// 切换代理时中断连接
    /// None: 不中断
    /// Chain: 仅中断使用该代理链的连接
    /// All: 中断所有连接
    pub break_when_proxy_change: BreakWhenProxyChange,

    /// 切换配置时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    #[builder(default = "true")]
    pub break_when_profile_change: bool,

    /// 切换模式时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    #[builder(default = "true")]
    pub break_when_mode_change: bool,

    /// 默认的延迟测试连接
    pub default_latency_test: String,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: bool,

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

    /// 是否启用随机端口
    pub enable_random_port: bool,

    /// verge mixed port 用于覆盖 clash 的 mixed port
    #[builder(default = "7890")]
    pub verge_mixed_port: u16,

    /// Check update when app launch
    #[builder(default = "true")]
    pub enable_auto_check_update: bool,

    /// Clash 相关策略
    pub clash_strategy: ClashStrategy,

    /// 是否启用代理托盘选择
    pub clash_tray_selector: ProxiesSelectorMode,

    pub always_on_top: bool,

    /// Tun 堆栈选择
    /// TODO: 弃用此字段，转移到 clash config 里
    pub tun_stack: TunStack,

    /// 是否启用网络统计信息浮窗
    #[builder_field_attr(serde(skip_serializing_if = "Option::is_none"))]
    pub network_statistic_widget: NetworkStatisticWidgetConfig,

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
