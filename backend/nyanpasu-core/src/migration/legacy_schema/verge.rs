use nyanpasu_macro::VergePatch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum ClashCore {
    #[serde(rename = "clash", alias = "clash-premium")]
    ClashPremium,
    #[serde(rename = "clash-rs")]
    ClashRs,
    #[serde(rename = "mihomo", alias = "clash-meta")]
    Mihomo,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs-alpha")]
    ClashRsAlpha,
    #[serde(rename = "meow")]
    Meow,
    #[serde(rename = "meow-alpha")]
    MeowAlpha,
}

impl Default for ClashCore {
    fn default() -> Self {
        match cfg!(feature = "default-meta") {
            false => Self::ClashPremium,
            true => Self::Mihomo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxiesSelectorMode {
    Hidden,
    #[default]
    Normal,
    Submenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TunStack {
    System,
    #[default]
    Gvisor,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BreakWhenProxyChange {
    #[default]
    None,
    Chain,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuMode {
    Native,
    Webview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TrayMenuCloseBehavior {
    #[default]
    Hide,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowType {
    #[default]
    Main,
}

/// ### `verge.yaml` schema
#[derive(Default, Debug, Clone, Deserialize, Serialize, VergePatch)]
#[verge(patch_fn = "patch_config")]
pub struct IVerge {
    /// app listening port for app singleton
    pub app_singleton_port: Option<u16>,

    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: Option<LoggingLevel>,

    // i18n
    pub language: Option<String>,

    /// `light` or `dark` or `system`
    pub theme_mode: Option<String>,

    /// enable traffic graph default is true
    pub traffic_graph: Option<bool>,

    /// show memory info (only for Clash Meta)
    pub enable_memory_usage: Option<bool>,

    /// global ui framer motion effects
    pub lighten_animation_effects: Option<bool>,

    /// clash tun mode
    pub enable_tun_mode: Option<bool>,

    /// windows service mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_service_mode: Option<bool>,

    /// can the app auto startup
    pub enable_auto_launch: Option<bool>,

    /// not show the window on launch
    pub enable_silent_start: Option<bool>,

    /// set system proxy
    pub enable_system_proxy: Option<bool>,

    /// enable proxy guard
    pub enable_proxy_guard: Option<bool>,

    /// set system proxy bypass
    pub system_proxy_bypass: Option<String>,

    /// proxy guard interval
    #[serde(alias = "proxy_guard_duration")]
    pub proxy_guard_interval: Option<u64>,

    /// theme setting
    pub theme_color: Option<String>,

    /// web ui list
    pub web_ui_list: Option<Vec<String>>,

    /// clash core path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_core: Option<ClashCore>,
    pub clash_control_channel: Option<nyanpasu_config::clash::config::ClashControlChannel>,
    pub clash_ipc_disable_http_controller: Option<bool>,

    /// hotkey map
    /// format: {func},{key}
    pub hotkeys: Option<Vec<String>>,

    /// 切换代理时自动关闭连接 (已弃用)
    #[deprecated(note = "use `break_when_proxy_change` instead")]
    pub auto_close_connection: Option<bool>,

    /// 切换代理时中断连接
    /// None: 不中断
    /// Chain: 仅中断使用该代理链的连接
    /// All: 中断所有连接
    pub break_when_proxy_change: Option<BreakWhenProxyChange>,

    /// 切换配置时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    pub break_when_profile_change: Option<bool>,

    /// 切换模式时中断连接
    /// true: 中断所有连接
    /// false: 不中断连接
    pub break_when_mode_change: Option<bool>,

    /// 默认的延迟测试连接
    pub default_latency_test: Option<String>,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: Option<bool>,

    /// 是否使用内部的脚本支持，默认为真
    pub enable_builtin_enhanced: Option<bool>,

    /// proxy 页面布局 列数
    pub proxy_layout_column: Option<i32>,

    /// 日志清理
    /// 分钟数； 0 为不清理
    #[deprecated(note = "use `max_log_files` instead")]
    pub auto_log_clean: Option<i64>,
    /// 日记轮转时间，单位：天
    pub max_log_files: Option<usize>,
    /// window size and position
    #[deprecated(note = "use `window_size_state` instead")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_position: Option<Vec<f64>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_state: Option<WindowState>,

    /// 是否启用随机端口
    pub enable_random_port: Option<bool>,

    /// verge mixed port 用于覆盖 clash 的 mixed port
    pub verge_mixed_port: Option<u16>,

    /// Check update when app launch
    pub enable_auto_check_update: Option<bool>,

    /// Clash 相关策略
    pub clash_strategy: Option<ClashStrategy>,

    /// 是否启用代理托盘选择
    pub clash_tray_selector: Option<ProxiesSelectorMode>,

    pub always_on_top: Option<bool>,

    /// Tun 堆栈选择
    /// TODO: 弃用此字段，转移到 clash config 里
    pub tun_stack: Option<TunStack>,

    /// 是否启用网络统计信息浮窗
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_statistic_widget: Option<LegacyNetworkStatisticWidgetConfig>,

    /// PAC URL for automatic proxy configuration
    /// This field is used to set PAC proxy without exposing it to the frontend UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pac_url: Option<String>,

    /// enable tray text display on Linux systems
    /// When enabled, shows proxy and TUN mode status as text next to the tray icon
    /// When disabled, only shows status via icon changes (prevents text display issues on Wayland)
    pub enable_tray_text: Option<bool>,

    /// Window type to use when opening the app window
    /// Main: opens new main window
    pub window_type: Option<WindowType>,

    /// Tray menu implementation mode
    /// Native: use the OS system tray menu (default on non-Windows)
    /// Webview: use a custom WebView window (default on Windows)
    pub tray_menu_mode: Option<TrayMenuMode>,

    /// Webview tray menu window dismiss behavior
    /// Hide: hide the window on close (fast re-open, higher memory usage)
    /// Close: destroy the window on close (slower re-open, lower memory usage)
    pub tray_menu_close_behavior: Option<TrayMenuCloseBehavior>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl IVerge {
    pub fn template() -> Self {
        Self {
            clash_core: Some(ClashCore::default()),
            clash_control_channel: Some(
                nyanpasu_config::clash::config::ClashControlChannel::default(),
            ),
            clash_ipc_disable_http_controller: Some(false),
            language: Some(
                nyanpasu_config::application::default_i18n_language()
                    .as_str()
                    .into(),
            ),
            app_log_level: Some(LoggingLevel::default()),
            theme_mode: Some("system".into()),
            traffic_graph: Some(true),
            enable_memory_usage: Some(true),
            enable_auto_launch: Some(false),
            enable_silent_start: Some(false),
            enable_system_proxy: Some(false),
            enable_random_port: Some(false),
            verge_mixed_port: Some(7890),
            enable_proxy_guard: Some(false),
            proxy_guard_interval: Some(30),
            // auto_close_connection: Some(true), // Deprecated, replaced by break_when_proxy_change
            break_when_proxy_change: Some(BreakWhenProxyChange::All),
            break_when_profile_change: Some(true),
            break_when_mode_change: Some(true),
            enable_builtin_enhanced: Some(true),
            enable_clash_fields: Some(true),
            lighten_animation_effects: Some(false),
            // auto_log_clean: Some(60 * 24 * 7), // 7 days 自动清理日记
            max_log_files: Some(7), // 7 days
            enable_auto_check_update: Some(true),
            clash_tray_selector: Some(ProxiesSelectorMode::default()),
            enable_service_mode: Some(false),
            always_on_top: Some(false),
            enable_tray_text: Some(false),
            window_type: Some(WindowType::Main),
            tray_menu_mode: Some(if cfg!(windows) {
                TrayMenuMode::Webview
            } else {
                TrayMenuMode::Native
            }),
            tray_menu_close_behavior: Some(TrayMenuCloseBehavior::default()),
            ..Self::default()
        }
    }

    /// Carries the deprecated `auto_close_connection` of a document written
    /// before `break_when_proxy_change` existed into the new field, as the
    /// legacy loader did. It must run before the document is merged onto
    /// [`Self::template`], whose `break_when_proxy_change` would shadow it.
    #[allow(deprecated)]
    pub fn migrate_auto_close_connection(&mut self) {
        if let (None, Some(enabled)) = (self.break_when_proxy_change, self.auto_close_connection) {
            self.break_when_proxy_change = Some(if enabled {
                BreakWhenProxyChange::All
            } else {
                BreakWhenProxyChange::None
            });
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum LoggingLevel {
    #[serde(rename = "silent", alias = "off")]
    Silent,
    #[serde(rename = "trace", alias = "tracing")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn", alias = "warning")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

impl Default for LoggingLevel {
    #[cfg(debug_assertions)]
    fn default() -> Self {
        Self::Trace
    }

    #[cfg(not(debug_assertions))]
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct ClashStrategy {
    pub external_controller_port_strategy: ExternalControllerPortStrategy,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalControllerPortStrategy {
    Fixed,
    Random,
    #[default]
    AllowFallback,
}

// Legacy flat widget setting; the typed `NetworkStatisticWidgetConfig` is
// tagged instead.
#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LegacyNetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Large,
    Small,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clash_core_default_preserves_default_meta_choice() {
        assert_eq!(
            ClashCore::default(),
            if cfg!(feature = "default-meta") {
                ClashCore::Mihomo
            } else {
                ClashCore::ClashPremium
            }
        );
    }
}
