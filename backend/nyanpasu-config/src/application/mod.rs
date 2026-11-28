use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use url::Url;

mod clash_core;
mod core_logs;
mod i18n;
mod logging;
mod network_automation;
mod permission;
mod preferences;
mod public_ip_probe;
mod remote_access;
mod settings_view;
mod system_settings;
mod traffic;
mod update;
mod widget;
mod window_close;
pub use clash_core::*;
pub use core_logs::*;
pub use i18n::*;
pub use logging::*;
pub use network_automation::*;
pub use permission::*;
pub use preferences::*;
pub use public_ip_probe::*;
pub use remote_access::*;
pub use settings_view::*;
pub use system_settings::*;
pub use traffic::*;
pub use update::*;
pub use widget::*;
pub use window_close::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

/// The Material color scheme variant the application theme is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeVariant {
    #[default]
    TonalSpot,
    Expressive,
    Fidelity,
    Content,
    Neutral,
    Monochrome,
    Vibrant,
    Rainbow,
    FruitSalad,
}

/// The Material color specification the application theme follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
pub enum ThemeSpecVersion {
    #[default]
    #[serde(rename = "2025")]
    V2025,
    #[serde(rename = "2021")]
    V2021,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Beta,
    Nightly,
}

impl ReleaseChannel {
    pub fn resolve(self, preference: Option<Self>) -> Self {
        if self == Self::Nightly {
            Self::Nightly
        } else {
            preference.unwrap_or(self)
        }
    }
}

/// The desktop application's choice of program for opening a profile file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileFileOpener {
    #[default]
    SystemDefault,
    VsCode,
    Zed,
    NotepadPlusPlus,
    Custom {
        path: std::path::PathBuf,
    },
}

/// `application.yaml` schema.
#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
#[patch(attribute(serde(default)))]
pub struct NyanpasuAppConfig {
    /// app listening port for app singleton
    pub app_singleton_port: u16,

    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: LoggingLevel,

    /// Core log disk settings, applied as soon as they are committed.
    #[serde(default)]
    pub core_logs: CoreLogSettings,

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

    /// Service mode, stored in the application domain.
    /// RPC writes use only `set_service_mode`: this setting can request privileged
    /// host state and must never be writable over HTTP. Core domain patches remain
    /// supported.
    pub enable_service_mode: bool,

    /// Device-local program selection; RPC writes require the desktop-only command.
    #[serde(default)]
    pub profile_file_opener: ProfileFileOpener,

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

    /// Material color scheme variant the theme is built from.
    #[serde(default)]
    pub theme_variant: ThemeVariant,

    /// Material contrast level, from -1.0 to 1.0.
    #[serde(default)]
    pub theme_contrast_level: f64,

    /// Material color specification the theme follows.
    #[serde(default)]
    pub theme_spec_version: ThemeSpecVersion,

    /// clash core path
    #[patch(attribute(serde(alias = "clash_core")))]
    pub core: ClashCore,

    /// Global shortcut bindings, stored in the application domain.
    /// RPC writes use only `set_hotkeys`: these host-bound bindings must never be
    /// writable over HTTP. Core domain patches remain supported.
    /// Format: `{function},{key}`.
    pub hotkeys: Vec<String>,

    /// 默认的延迟测试连接
    pub default_latency_test: String,

    /// Latency test timeout in milliseconds.
    #[serde(default = "default_latency_timeout_ms")]
    pub default_latency_timeout_ms: u64,

    /// 是否使用内部的脚本支持，默认为真
    pub enable_builtin_enhanced: bool,

    /// proxy 页面布局 列数
    pub proxy_layout_column: i32,

    /// 最多保留的日志文件数
    pub max_log_files: usize,

    /// 单个日志文件的大小上限，单位：MiB
    #[serde(default = "default_max_log_file_size")]
    pub max_log_file_size: u64,

    /// Check update when app launch
    pub enable_auto_check_update: bool,

    /// Download application updates automatically after they are found
    #[serde(default)]
    pub enable_auto_download_update: bool,

    /// None in older configurations means the channel of the installed build.
    #[serde(default)]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<ReleaseChannel>>)))]
    pub release_channel: Option<ReleaseChannel>,

    /// Enabled application update package download sources, in priority order.
    #[serde(default = "default_update_sources")]
    pub update_sources: Vec<UpdateSource>,

    /// 是否启用代理托盘选择
    #[patch(attribute(serde(alias = "clash_tray_selector")))]
    pub tray_selector_mode: ProxiesSelectorMode,

    /// 是否窗口置顶
    pub always_on_top: bool,

    /// 托盘菜单模式：系统原生菜单还是 WebView 菜单
    /// 平台相关默认值：Windows 为 `webview`，其他平台 `native`
    pub tray_menu_mode: TrayMenuMode,

    /// 窗口关闭时的行为：销毁还是隐藏，可按窗口种类单独设定
    #[patch(nesting)]
    #[serde(default)]
    pub window_close: WindowCloseSettings,

    /// 是否启用网络统计信息浮窗
    pub network_statistic_widget: NetworkStatisticWidgetConfig,

    /// Which logo the network statistic widget shows
    #[serde(default)]
    pub network_statistic_widget_logo: NetworkStatisticWidgetLogo,

    /// How long recorded traffic is kept
    #[serde(default)]
    pub traffic_retention: TrafficRetention,

    /// Who may reach the application over HTTP. Written only by the dedicated
    /// remote-access command; ordinary preference patches reject it.
    #[serde(default)]
    pub remote_access: RemoteAccessConfig,

    /// Allow public IP probes to locate LAN/local traffic sources.
    #[serde(default)]
    pub enable_local_ip_probe: bool,

    /// STUN servers the public IP probe asks, built-in and user-added, in
    /// the order it asks them.
    #[serde(
        default = "default_stun_servers",
        deserialize_with = "deserialize_stun_servers"
    )]
    #[specta(type = Vec<StunServer>)]
    pub stun_servers: Vec<StunServer>,

    /// HTTP lookup services asked when no STUN server answers, in order. A
    /// provider is listed only after the user accepted its privacy notice.
    #[serde(default)]
    pub ip_lookup_providers: Vec<IpLookupProvider>,

    /// Rules that switch system proxy, TUN and transparent proxy when the
    /// machine arrives on a network. Replaced as a whole by a patch.
    #[serde(default)]
    pub network_automation: NetworkAutomation,

    /// PAC URL for automatic proxy configuration
    /// This field is used to set PAC proxy without exposing it to the frontend UI
    #[serde(skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<Url>>)))]
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

    /// Enable colored tray icons on macOS.
    ///
    /// When enabled, uses colored icons instead of template icons to show proxy
    /// status; when disabled, uses system template icons that adapt to light and
    /// dark mode. The field exists on every platform so one schema describes
    /// every install and a portable projection round-trips unchanged; only the
    /// macOS tray reads it.
    #[serde(default)]
    pub enable_macos_colored_icons: bool,
}

/// The latency test timeout a fresh or upgraded install starts with.
pub const DEFAULT_LATENCY_TIMEOUT_MS: u64 = 5000;

fn default_latency_timeout_ms() -> u64 {
    DEFAULT_LATENCY_TIMEOUT_MS
}

/// A latency test waits between one and thirty seconds: Mihomo parses a single-node
/// timeout as int16 milliseconds and the API adapter bounds every call.
pub fn validate_latency_timeout(timeout_ms: u64) -> Result<(), &'static str> {
    if (1000..=30000).contains(&timeout_ms) {
        Ok(())
    } else {
        Err("the latency test timeout must be between 1 and 30 seconds")
    }
}

/// A Material contrast level is finite and within the builder's `-1..=1` range.
pub fn validate_theme_contrast_level(level: f64) -> Result<(), &'static str> {
    if level.is_finite() && (-1.0..=1.0).contains(&level) {
        Ok(())
    } else {
        Err("the theme contrast level must be a finite value between -1 and 1")
    }
}

fn default_max_log_file_size() -> u64 {
    10
}

impl Default for NyanpasuAppConfig {
    fn default() -> Self {
        Self {
            app_singleton_port: 0,
            app_log_level: LoggingLevel::default(),
            core_logs: CoreLogSettings::default(),
            language: default_i18n_language(),
            theme_mode: ThemeMode::System,
            traffic_graph: true,
            enable_memory_usage: true,
            lighten_animation_effects: false,
            enable_service_mode: false,
            profile_file_opener: ProfileFileOpener::default(),
            enable_auto_launch: false,
            enable_silent_start: false,
            enable_system_proxy: false,
            enable_proxy_guard: false,
            system_proxy_bypass: String::new(),
            proxy_guard_interval: 30,
            theme_color: CssColor::from_rgba8(24, 103, 192, 255),
            theme_variant: ThemeVariant::default(),
            theme_contrast_level: 0.0,
            theme_spec_version: ThemeSpecVersion::default(),
            core: ClashCore::default(),
            hotkeys: Vec::new(),
            default_latency_test: "http://www.gstatic.com/generate_204".into(),
            default_latency_timeout_ms: default_latency_timeout_ms(),
            enable_builtin_enhanced: true,
            proxy_layout_column: 0,
            max_log_files: 7,
            max_log_file_size: default_max_log_file_size(),
            enable_auto_check_update: true,
            enable_auto_download_update: false,
            release_channel: None,
            update_sources: default_update_sources(),
            tray_selector_mode: ProxiesSelectorMode::default(),
            always_on_top: false,
            tray_menu_mode: TrayMenuMode::default(),
            window_close: WindowCloseSettings::default(),
            network_statistic_widget: NetworkStatisticWidgetConfig::default(),
            network_statistic_widget_logo: NetworkStatisticWidgetLogo::default(),
            traffic_retention: TrafficRetention::default(),
            remote_access: RemoteAccessConfig::default(),
            enable_local_ip_probe: false,
            stun_servers: default_stun_servers(),
            ip_lookup_providers: Vec::new(),
            network_automation: NetworkAutomation::default(),
            pac_url: None,
            enable_tray_text: false,
            enable_tray_traffic: false,
            use_legacy_ui: false,
            enable_macos_colored_icons: false,
        }
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Status;

    #[test]
    fn profile_file_opener_defaults_for_older_configs() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("profile_file_opener");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.profile_file_opener, ProfileFileOpener::SystemDefault);
    }

    /// The macOS icon preference is part of every platform's schema, so a file
    /// written before it existed still loads and a projection round-trips.
    #[test]
    fn the_macos_icon_preference_is_unconditional_and_defaults_when_missing() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        assert!(value.get("enable_macos_colored_icons").is_some());
        value
            .as_object_mut()
            .unwrap()
            .remove("enable_macos_colored_icons");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert!(!config.enable_macos_colored_icons);
    }

    #[test]
    fn latency_timeout_defaults_when_missing_and_is_bounded() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("default_latency_timeout_ms");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(
            config.default_latency_timeout_ms,
            DEFAULT_LATENCY_TIMEOUT_MS
        );

        assert!(validate_latency_timeout(1000).is_ok());
        assert!(validate_latency_timeout(30000).is_ok());
        assert!(validate_latency_timeout(999).is_err());
        assert!(validate_latency_timeout(30001).is_err());
    }

    #[test]
    fn theme_preferences_default_when_missing() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        for field in [
            "theme_variant",
            "theme_contrast_level",
            "theme_spec_version",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.theme_variant, ThemeVariant::TonalSpot);
        assert_eq!(config.theme_contrast_level, 0.0);
        assert_eq!(config.theme_spec_version, ThemeSpecVersion::V2025);
    }

    #[test]
    fn theme_variant_wire_values_round_trip() {
        for (text, variant) in [
            ("tonal_spot", ThemeVariant::TonalSpot),
            ("expressive", ThemeVariant::Expressive),
            ("fidelity", ThemeVariant::Fidelity),
            ("content", ThemeVariant::Content),
            ("neutral", ThemeVariant::Neutral),
            ("monochrome", ThemeVariant::Monochrome),
            ("vibrant", ThemeVariant::Vibrant),
            ("rainbow", ThemeVariant::Rainbow),
            ("fruit_salad", ThemeVariant::FruitSalad),
        ] {
            let encoded = serde_json::to_string(&variant).unwrap();
            assert_eq!(encoded, format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<ThemeVariant>(&encoded).unwrap(),
                variant
            );

            let patch: NyanpasuAppConfigPatch =
                serde_json::from_str(&format!(r#"{{"theme_variant":"{text}"}}"#)).unwrap();
            assert_eq!(patch.theme_variant, Some(variant));
        }
        assert!(
            serde_json::from_str::<NyanpasuAppConfigPatch>(r#"{"theme_variant":"unknown"}"#)
                .is_err()
        );
    }

    #[test]
    fn theme_spec_version_wire_values_round_trip() {
        for (text, version) in [
            ("2025", ThemeSpecVersion::V2025),
            ("2021", ThemeSpecVersion::V2021),
        ] {
            let encoded = serde_json::to_string(&version).unwrap();
            assert_eq!(encoded, format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<ThemeSpecVersion>(&encoded).unwrap(),
                version
            );

            let patch: NyanpasuAppConfigPatch =
                serde_json::from_str(&format!(r#"{{"theme_spec_version":"{text}"}}"#)).unwrap();
            assert_eq!(patch.theme_spec_version, Some(version));
        }
        assert!(
            serde_json::from_str::<NyanpasuAppConfigPatch>(r#"{"theme_spec_version":"2023"}"#)
                .is_err()
        );
    }

    #[test]
    fn theme_contrast_level_is_finite_and_bounded() {
        assert!(validate_theme_contrast_level(-1.0).is_ok());
        assert!(validate_theme_contrast_level(0.0).is_ok());
        assert!(validate_theme_contrast_level(1.0).is_ok());
        assert!(validate_theme_contrast_level(-1.01).is_err());
        assert!(validate_theme_contrast_level(1.01).is_err());
        assert!(validate_theme_contrast_level(f64::NAN).is_err());
        assert!(validate_theme_contrast_level(f64::INFINITY).is_err());
        assert!(validate_theme_contrast_level(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn network_automation_defaults_when_missing_and_is_replaced_by_a_patch() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("network_automation");
        let mut config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.network_automation, NetworkAutomation::default());

        let patch = serde_json::from_str(
            r#"{"network_automation":{"enabled":true,"default_actions":{"tun":"disable"}}}"#,
        )
        .unwrap();
        config.apply(patch);
        assert!(config.network_automation.enabled);
        assert_eq!(
            config.network_automation.default_actions,
            MechanismActions {
                tun: MechanismAction::Disable,
                ..Default::default()
            }
        );
    }

    #[test]
    fn local_ip_probe_requires_opt_in_and_can_be_patched() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("enable_local_ip_probe");
        let mut config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert!(!config.enable_local_ip_probe);
        let patch = serde_json::from_str(r#"{"enable_local_ip_probe":true}"#).unwrap();
        config.apply(patch);
        assert!(config.enable_local_ip_probe);
    }

    #[test]
    fn existing_configurations_get_the_default_probe_sources() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        for field in ["stun_servers", "ip_lookup_providers"] {
            object.remove(field);
        }
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();

        assert_eq!(config.stun_servers, default_stun_servers());
        assert!(config.ip_lookup_providers.is_empty());
    }

    #[test]
    fn legacy_stun_servers_migrate_on_deserialize_preserving_user_order_and_toggles() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        let stale_caps = StunServerCapabilities::default();
        let custom_caps = StunServerCapabilities {
            udp: StunFamilies {
                ipv4: true,
                ipv6: false,
            },
            tcp: StunFamilies {
                ipv4: false,
                ipv6: true,
            },
        };
        value["stun_servers"] = serde_json::json!([
            {
                "host": "stun.miwifi.com",
                "port": 3478,
                "enabled": true,
                "capabilities": stale_caps,
                "builtin": true
            },
            {
                "host": "stun.custom.example",
                "port": 3479,
                "enabled": true,
                "capabilities": custom_caps,
                "builtin": false
            },
            {
                "host": "stun.nextcloud.com",
                "port": 3478,
                "enabled": false,
                "capabilities": stale_caps,
                "builtin": true
            },
            {
                "host": "stun.cloudflare.com",
                "port": 3478,
                "enabled": false,
                "capabilities": stale_caps,
                "builtin": true
            },
            {
                "host": "stun.l.google.com",
                "port": 19302,
                "enabled": true,
                "capabilities": stale_caps,
                "builtin": true
            },
            {
                "host": "stun.chat.bilibili.com",
                "port": 3478,
                "enabled": false,
                "capabilities": stale_caps,
                "builtin": true
            }
        ]);

        let yaml = yaml_serde::to_string(&value).unwrap();
        let config: NyanpasuAppConfig = yaml_serde::from_str(&yaml).unwrap();

        let hosts: Vec<&str> = config
            .stun_servers
            .iter()
            .map(|server| server.host.as_str())
            .collect();
        assert_eq!(
            hosts,
            [
                "stun.miwifi.com",
                "stun.custom.example",
                "stun.nextcloud.com",
                "stun.cloudflare.com",
                "stun.l.google.com",
                "stun.chat.bilibili.com",
                "stun.hitv.com",
            ]
        );
        assert!(!config.stun_servers[2].enabled);
        assert!(!config.stun_servers[3].enabled);
        assert!(!config.stun_servers[5].enabled);
        assert!(config.stun_servers[6].enabled);

        let nextcloud_default = default_stun_servers()
            .into_iter()
            .find(|server| server.host == "stun.nextcloud.com")
            .unwrap();
        assert_eq!(
            config.stun_servers[2].capabilities,
            nextcloud_default.capabilities
        );
        assert_eq!(config.stun_servers[1].capabilities, custom_caps);
        assert!(!config.stun_servers[1].builtin);
        assert_eq!(
            validate_public_ip_probe_sources(&config.stun_servers, &config.ip_lookup_providers),
            Ok(())
        );
    }

    #[test]
    fn stun_server_patch_preserves_explicit_list_without_reinjecting_builtins() {
        let patch: NyanpasuAppConfigPatch = serde_json::from_str(r#"{"stun_servers":[]}"#).unwrap();
        assert_eq!(patch.stun_servers, Some(Vec::new()));
    }

    #[test]
    fn update_sources_default_for_existing_configurations_and_keep_patch_order() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("update_sources");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.update_sources, default_update_sources());
        let patch: NyanpasuAppConfigPatch = serde_json::from_str(
            r#"{"update_sources":["ghfast","sourceforge","github","nyanpasu"]}"#,
        )
        .unwrap();
        assert_eq!(
            patch.update_sources,
            Some(vec![
                UpdateSource::Ghfast,
                UpdateSource::Sourceforge,
                UpdateSource::Github,
                UpdateSource::Nyanpasu
            ])
        );
        assert!(
            serde_json::from_str::<NyanpasuAppConfigPatch>(r#"{"update_sources":["unknown"]}"#)
                .is_err()
        );
    }

    #[test]
    fn release_channel_defaults_for_existing_configurations() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("release_channel");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.release_channel, None);
        let patch: NyanpasuAppConfigPatch =
            serde_json::from_str(r#"{"release_channel":"beta"}"#).unwrap();
        assert_eq!(patch.release_channel, Some(Some(ReleaseChannel::Beta)));
    }

    #[test]
    fn widget_logo_defaults_for_existing_configurations() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("network_statistic_widget_logo");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(
            config.network_statistic_widget_logo,
            NetworkStatisticWidgetLogo::default()
        );
    }

    #[test]
    fn max_log_file_size_defaults_for_existing_configurations() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("max_log_file_size");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.max_log_file_size, 10);
    }

    #[test]
    fn auto_download_update_defaults_off_for_defaults_and_existing_configurations() {
        assert!(!NyanpasuAppConfig::default().enable_auto_download_update);

        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("enable_auto_download_update");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert!(!config.enable_auto_download_update);
    }

    #[test]
    fn auto_download_update_is_patchable() {
        let patch: NyanpasuAppConfigPatch =
            serde_json::from_str(r#"{"enable_auto_download_update":true}"#).unwrap();
        assert_eq!(patch.enable_auto_download_update, Some(true));

        let mut config = NyanpasuAppConfig::default();
        config.apply(patch);
        assert!(config.enable_auto_download_update);
    }

    #[test]
    fn traffic_retention_defaults_for_existing_configurations() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("traffic_retention");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.traffic_retention, TrafficRetention::SevenDays);
    }

    #[test]
    fn traffic_retention_wire_format_and_patch() {
        for (text, retention) in [
            ("1d", TrafficRetention::OneDay),
            ("7d", TrafficRetention::SevenDays),
            ("30d", TrafficRetention::ThirtyDays),
            ("90d", TrafficRetention::NinetyDays),
            ("forever", TrafficRetention::Forever),
        ] {
            assert_eq!(
                serde_json::to_string(&retention).unwrap(),
                format!("\"{text}\"")
            );
            let patch: NyanpasuAppConfigPatch =
                serde_json::from_str(&format!(r#"{{"traffic_retention":"{text}"}}"#)).unwrap();
            assert_eq!(patch.traffic_retention, Some(retention));
        }
        assert!(
            serde_json::from_str::<NyanpasuAppConfigPatch>(r#"{"traffic_retention":"2d"}"#)
                .is_err()
        );

        let mut config = NyanpasuAppConfig::default();
        config.apply(NyanpasuAppConfigPatch {
            traffic_retention: Some(TrafficRetention::Forever),
            ..NyanpasuAppConfig::new_empty_patch()
        });
        assert_eq!(config.traffic_retention, TrafficRetention::Forever);
    }

    #[test]
    fn traffic_retention_durations() {
        let day = std::time::Duration::from_secs(24 * 60 * 60);
        assert_eq!(TrafficRetention::OneDay.duration(), Some(day));
        assert_eq!(TrafficRetention::SevenDays.duration(), Some(7 * day));
        assert_eq!(TrafficRetention::ThirtyDays.duration(), Some(30 * day));
        assert_eq!(TrafficRetention::NinetyDays.duration(), Some(90 * day));
        assert_eq!(TrafficRetention::Forever.duration(), None);
    }

    /// The legacy field aliases carried over from the former derive-builder
    /// partial must still decode onto the generated `NyanpasuAppConfigPatch`.
    #[test]
    fn patch_honours_legacy_aliases() {
        let patch: NyanpasuAppConfigPatch = yaml_serde::from_str(
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

        let dumped = yaml_serde::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("traffic_graph: false"), "got:\n{dumped}");
        assert!(
            !dumped.contains("app_singleton_port"),
            "absent fields must be skipped, got:\n{dumped}"
        );
    }

    /// Tray menu settings decode from their snake_case wire form onto the patch,
    /// and the mode keeps its platform-dependent default.
    #[test]
    fn tray_menu_settings_wire_format() {
        let patch: NyanpasuAppConfigPatch =
            yaml_serde::from_str("tray_menu_mode: native\nwindow_close:\n  tray_menu: hide\n")
                .expect("tray menu patch must deserialize");

        assert_eq!(patch.tray_menu_mode, Some(TrayMenuMode::Native));
        assert_eq!(
            patch.window_close.tray_menu,
            Some(WindowCloseOverride::Hide)
        );

        let expected_mode = if cfg!(windows) {
            TrayMenuMode::Webview
        } else {
            TrayMenuMode::Native
        };
        assert_eq!(TrayMenuMode::default(), expected_mode);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.tray_menu_mode = Some(TrayMenuMode::Webview);
        let dumped = yaml_serde::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("tray_menu_mode: webview"), "got:\n{dumped}");
    }

    /// `pac_url` (struct-level `skip_serializing_none` + `double_option`):
    /// absent decodes to keep, explicit `null` to clear, `Some(None)` serializes
    /// as `null` while an absent field is skipped.
    #[test]
    fn pac_url_double_option_wire_semantics() {
        let keep: NyanpasuAppConfigPatch =
            yaml_serde::from_str("traffic_graph: true\n").expect("patch must deserialize");
        assert_eq!(keep.pac_url, None, "absent decodes to outer None (keep)");

        let clear: NyanpasuAppConfigPatch =
            yaml_serde::from_str("pac_url: null\n").expect("patch must deserialize");
        assert_eq!(
            clear.pac_url,
            Some(None),
            "null decodes to Some(None) (clear)"
        );

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.pac_url = Some(None);
        let dumped = yaml_serde::to_string(&patch).expect("serialize patch");
        assert!(
            dumped.contains("pac_url: null"),
            "Some(None) -> null, got:\n{dumped}"
        );
        assert!(
            !dumped.contains("theme_mode"),
            "absent skipped, got:\n{dumped}"
        );
    }
}
