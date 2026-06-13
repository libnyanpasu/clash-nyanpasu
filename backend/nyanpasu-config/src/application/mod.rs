use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
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

/// Whether the tray menu uses the system-native menu or the WebView menu.
///
/// Platform-dependent default: `Webview` on Windows, `Native` elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuMode {
    Native,
    Webview,
}

impl Default for TrayMenuMode {
    fn default() -> Self {
        if cfg!(windows) {
            TrayMenuMode::Webview
        } else {
            TrayMenuMode::Native
        }
    }
}

/// What happens to the WebView tray menu window when it loses focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuCloseBehavior {
    #[default]
    Hide,
    Close,
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
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
pub struct NyanpasuAppConfig {
    /// app listening port for app singleton
    pub app_singleton_port: u16,

    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: LoggingLevel,

    // i18n
    pub language: I18nLanguage,

    /// `light` or `dark` or `system`
    pub theme_mode: ThemeMode,

    /// enable traffic graph
    pub traffic_graph: bool,

    /// show memory info (only for Clash Meta)
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
    #[patch(attribute(serde(alias = "proxy_guard_duration")))]
    pub proxy_guard_interval: u64,

    /// theme setting
    #[specta(type = String)]
    #[patch(attribute(specta(type = String)))]
    pub theme_color: CssColor,

    /// clash core path
    #[patch(attribute(serde(alias = "clash_core")))]
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
    pub max_log_files: usize,

    /// Check update when app launch
    pub enable_auto_check_update: bool,

    /// 是否启用代理托盘选择
    #[patch(attribute(serde(alias = "clash_tray_selector")))]
    pub tray_selector_mode: ProxiesSelectorMode,

    /// 是否窗口置顶
    pub always_on_top: bool,

    /// 托盘菜单模式：系统原生菜单还是 WebView 菜单
    /// 平台相关默认值：Windows 为 `webview`，其他平台 `native`
    pub tray_menu_mode: TrayMenuMode,

    /// WebView 托盘菜单窗口失焦时的行为：隐藏还是销毁
    pub tray_menu_close_behavior: TrayMenuCloseBehavior,

    /// 是否启用网络统计信息浮窗
    pub network_statistic_widget: NetworkStatisticWidgetConfig,

    /// PAC URL for automatic proxy configuration
    /// This field is used to set PAC proxy without exposing it to the frontend UI
    #[serde(skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
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
            tray_menu_mode: todo!(),
            tray_menu_close_behavior: todo!(),
            network_statistic_widget: todo!(),
            pac_url: todo!(),
            enable_tray_text: todo!(),
            enable_tray_traffic: todo!(),
            use_legacy_ui: todo!(),
        }
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Status;

    /// The legacy field aliases carried over from the former derive-builder
    /// partial must still decode onto the generated `NyanpasuAppConfigPatch`.
    #[test]
    fn patch_honours_legacy_aliases() {
        let patch: NyanpasuAppConfigPatch = serde_yaml_ng::from_str(
            "proxy_guard_duration: 45\nclash_core: mihomo\nclash_tray_selector: hidden\n",
        )
        .expect("aliased patch must deserialize");

        assert_eq!(patch.proxy_guard_interval, Some(45));
        assert_eq!(patch.core, Some(ClashCore::Mihomo));
        assert_eq!(patch.tray_selector_mode, Some(ProxiesSelectorMode::Hidden));
        // Untouched fields stay absent.
        assert_eq!(patch.app_singleton_port, None);
        assert!(!patch.is_empty());
    }

    /// An absent field must serialize away (skip_serializing_none on the patch),
    /// so a partial patch round-trips to only the fields it carries.
    #[test]
    fn patch_skips_none_on_serialize() {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.traffic_graph = Some(false);

        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("traffic_graph: false"), "got:\n{dumped}");
        assert!(
            !dumped.contains("app_singleton_port"),
            "absent fields must be skipped, got:\n{dumped}"
        );
    }

    /// Tray menu settings decode from their snake_case wire form onto the patch,
    /// and the enums keep their platform-dependent / `Hide` defaults.
    #[test]
    fn tray_menu_settings_wire_format() {
        let patch: NyanpasuAppConfigPatch =
            serde_yaml_ng::from_str("tray_menu_mode: native\ntray_menu_close_behavior: close\n")
                .expect("tray menu patch must deserialize");

        assert_eq!(patch.tray_menu_mode, Some(TrayMenuMode::Native));
        assert_eq!(
            patch.tray_menu_close_behavior,
            Some(TrayMenuCloseBehavior::Close)
        );

        assert_eq!(TrayMenuCloseBehavior::default(), TrayMenuCloseBehavior::Hide);
        let expected_mode = if cfg!(windows) {
            TrayMenuMode::Webview
        } else {
            TrayMenuMode::Native
        };
        assert_eq!(TrayMenuMode::default(), expected_mode);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.tray_menu_mode = Some(TrayMenuMode::Webview);
        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("tray_menu_mode: webview"), "got:\n{dumped}");
    }

    /// `pac_url` (struct-level `skip_serializing_none` + `double_option`):
    /// absent decodes to keep, explicit `null` to clear, `Some(None)` serializes
    /// as `null` while an absent field is skipped.
    #[test]
    fn pac_url_double_option_wire_semantics() {
        let keep: NyanpasuAppConfigPatch =
            serde_yaml_ng::from_str("traffic_graph: true\n").expect("patch must deserialize");
        assert_eq!(keep.pac_url, None, "absent decodes to outer None (keep)");

        let clear: NyanpasuAppConfigPatch =
            serde_yaml_ng::from_str("pac_url: null\n").expect("patch must deserialize");
        assert_eq!(clear.pac_url, Some(None), "null decodes to Some(None) (clear)");

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.pac_url = Some(None);
        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("pac_url: null"), "Some(None) -> null, got:\n{dumped}");
        assert!(!dumped.contains("theme_mode"), "absent skipped, got:\n{dumped}");
    }
}

