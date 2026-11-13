//! The application settings a `read` caller may see.
//!
//! [`NyanpasuAppConfig`] holds values that can carry credentials: the PAC URL and the latency test
//! URL are free-form URLs a user may write a token into, in the userinfo, the query, the path or
//! the host name, so no syntactic check can tell a safe one apart. [`ApplicationSettings`] is the
//! projection for callers that only hold `read`. It keeps every ordinary preference and the public
//! desired state of the system settings, and reduces the two URL fields to flags:
//! `pac_configured` and `latency_test_configured`.
//!
//! A flag is not a placeholder: a form that needs the real URL reads the full config
//! (`sensitive_read`), and a flag is never a valid thing to write back.

use serde::Serialize;
use specta::Type;

use super::{
    ClashCore, CoreLogSettings, I18nLanguage, IpLookupProvider, LoggingLevel,
    NetworkStatisticWidgetConfig, NetworkStatisticWidgetLogo, NyanpasuAppConfig,
    ProxiesSelectorMode, ReleaseChannel, RemoteAccessConfig, StunServer, ThemeMode,
    TrafficRetention, TrayMenuMode, UpdateSource, WindowCloseSettings,
};

/// The secret-free view of [`NyanpasuAppConfig`].
///
/// The conversion destructures the config without `..`, so a field added to the config does not
/// compile until it is classified here.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ApplicationSettings {
    pub app_singleton_port: u16,
    pub app_log_level: LoggingLevel,
    pub core_logs: CoreLogSettings,
    pub language: I18nLanguage,
    pub theme_mode: ThemeMode,
    pub traffic_graph: bool,
    pub enable_memory_usage: bool,
    pub lighten_animation_effects: bool,
    pub enable_service_mode: bool,
    pub enable_auto_launch: bool,
    pub enable_silent_start: bool,
    pub enable_system_proxy: bool,
    pub enable_proxy_guard: bool,
    pub system_proxy_bypass: String,
    pub proxy_guard_interval: u64,
    #[specta(type = String)]
    pub theme_color: csscolorparser::Color,
    pub core: ClashCore,
    pub hotkeys: Vec<String>,
    /// Whether a default latency test URL is set. The URL itself is only in the full config; a
    /// probe that omits its URL uses it on the backend.
    pub latency_test_configured: bool,
    pub default_latency_timeout_ms: u64,
    pub enable_builtin_enhanced: bool,
    pub proxy_layout_column: i32,
    pub max_log_files: usize,
    pub max_log_file_size: u64,
    pub enable_auto_check_update: bool,
    pub enable_auto_download_update: bool,
    pub release_channel: Option<ReleaseChannel>,
    pub update_sources: Vec<UpdateSource>,
    pub tray_selector_mode: ProxiesSelectorMode,
    pub always_on_top: bool,
    pub tray_menu_mode: TrayMenuMode,
    pub window_close: WindowCloseSettings,
    pub network_statistic_widget: NetworkStatisticWidgetConfig,
    pub network_statistic_widget_logo: NetworkStatisticWidgetLogo,
    pub traffic_retention: TrafficRetention,
    pub remote_access: RemoteAccessConfig,
    pub enable_local_ip_probe: bool,
    pub stun_servers: Vec<StunServer>,
    pub ip_lookup_providers: Vec<IpLookupProvider>,
    /// Whether a PAC URL is set. The URL itself is only in the full config.
    pub pac_configured: bool,
    pub enable_tray_text: bool,
    pub enable_tray_traffic: bool,
    pub use_legacy_ui: bool,
    #[cfg(target_os = "macos")]
    #[specta(skip)]
    pub enable_macos_colored_icons: bool,
}

impl From<&NyanpasuAppConfig> for ApplicationSettings {
    fn from(config: &NyanpasuAppConfig) -> Self {
        let NyanpasuAppConfig {
            app_singleton_port,
            app_log_level,
            core_logs,
            language,
            theme_mode,
            traffic_graph,
            enable_memory_usage,
            lighten_animation_effects,
            enable_service_mode,
            enable_auto_launch,
            enable_silent_start,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            theme_color,
            core,
            hotkeys,
            default_latency_test,
            default_latency_timeout_ms,
            enable_builtin_enhanced,
            proxy_layout_column,
            max_log_files,
            max_log_file_size,
            enable_auto_check_update,
            enable_auto_download_update,
            release_channel,
            update_sources,
            tray_selector_mode,
            always_on_top,
            tray_menu_mode,
            window_close,
            network_statistic_widget,
            network_statistic_widget_logo,
            traffic_retention,
            remote_access,
            enable_local_ip_probe,
            stun_servers,
            ip_lookup_providers,
            pac_url,
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
        } = config;

        Self {
            app_singleton_port: *app_singleton_port,
            app_log_level: app_log_level.clone(),
            core_logs: core_logs.clone(),
            language: language.clone(),
            theme_mode: *theme_mode,
            traffic_graph: *traffic_graph,
            enable_memory_usage: *enable_memory_usage,
            lighten_animation_effects: *lighten_animation_effects,
            enable_service_mode: *enable_service_mode,
            enable_auto_launch: *enable_auto_launch,
            enable_silent_start: *enable_silent_start,
            enable_system_proxy: *enable_system_proxy,
            enable_proxy_guard: *enable_proxy_guard,
            system_proxy_bypass: system_proxy_bypass.clone(),
            proxy_guard_interval: *proxy_guard_interval,
            theme_color: theme_color.clone(),
            core: core.clone(),
            hotkeys: hotkeys.clone(),
            latency_test_configured: !default_latency_test.is_empty(),
            default_latency_timeout_ms: *default_latency_timeout_ms,
            enable_builtin_enhanced: *enable_builtin_enhanced,
            proxy_layout_column: *proxy_layout_column,
            max_log_files: *max_log_files,
            max_log_file_size: *max_log_file_size,
            enable_auto_check_update: *enable_auto_check_update,
            enable_auto_download_update: *enable_auto_download_update,
            release_channel: *release_channel,
            update_sources: update_sources.clone(),
            tray_selector_mode: *tray_selector_mode,
            always_on_top: *always_on_top,
            tray_menu_mode: *tray_menu_mode,
            window_close: window_close.clone(),
            network_statistic_widget: network_statistic_widget.clone(),
            network_statistic_widget_logo: *network_statistic_widget_logo,
            traffic_retention: traffic_retention.clone(),
            remote_access: remote_access.clone(),
            enable_local_ip_probe: *enable_local_ip_probe,
            stun_servers: stun_servers.clone(),
            ip_lookup_providers: ip_lookup_providers.clone(),
            pac_configured: pac_url.is_some(),
            enable_tray_text: *enable_tray_text,
            enable_tray_traffic: *enable_tray_traffic,
            use_legacy_ui: *use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons: *enable_macos_colored_icons,
        }
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    fn serialized(config: &NyanpasuAppConfig) -> String {
        serde_json::to_string(&ApplicationSettings::from(config)).unwrap()
    }

    #[test]
    fn pac_url_is_reduced_to_a_flag() {
        let mut config = NyanpasuAppConfig::default();
        config.pac_url = Some(
            Url::parse("https://user:pac-sentinel@pac.example/proxy.pac?token=pac-sentinel")
                .unwrap(),
        );

        let json = serialized(&config);
        assert!(!json.contains("pac-sentinel"), "{json}");
        assert!(!json.contains("pac.example"), "{json}");
        let view = ApplicationSettings::from(&config);
        assert!(view.pac_configured);
        assert!(!ApplicationSettings::from(&NyanpasuAppConfig::default()).pac_configured);
    }

    #[test]
    fn latency_url_is_reduced_to_a_flag() {
        for private in [
            "https://user:latency-sentinel@probe.example/ping",
            "https://probe.example/ping?key=latency-sentinel",
            "https://probe.example/ping#latency-sentinel",
            "https://probe.example/latency-sentinel/generate_204",
            "https://latency-sentinel.probe.example/generate_204",
            "http://www.gstatic.com/generate_204",
        ] {
            let mut config = NyanpasuAppConfig::default();
            config.default_latency_test = private.to_owned();
            let json = serialized(&config);
            assert!(!json.contains("sentinel"), "{private}: {json}");
            assert!(!json.contains("probe.example"), "{private}: {json}");
            assert!(!json.contains("gstatic"), "{private}: {json}");
            assert!(ApplicationSettings::from(&config).latency_test_configured);
        }

        let mut config = NyanpasuAppConfig::default();
        config.default_latency_test.clear();
        assert!(!ApplicationSettings::from(&config).latency_test_configured);
    }
}
