//! The ordinary application preferences, as one request contract.
//!
//! [`NyanpasuAppConfigPatch`] is the internal patch of the whole config. Some of its fields have
//! a different privilege class (hotkeys, service mode, remote access) and are written only by
//! their own commands. The general preferences command therefore accepts this narrower patch:
//! its fields, and so its schema, are exactly the permitted ones.

use csscolorparser::Color as CssColor;
use serde::{Deserialize, Deserializer, Serialize};
use snafu::Snafu;
use specta::Type;
use url::Url;

use super::{
    ClashCore, CoreLogSettings, I18nLanguage, IpLookupProvider, LoggingLevel,
    NetworkStatisticWidgetConfig, NetworkStatisticWidgetLogo, NyanpasuAppConfigPatch,
    ProxiesSelectorMode, ReleaseChannel, StunServer, ThemeMode, TrafficRetention, TrayMenuMode,
    UpdateSource, WindowCloseSettingsPatch,
};

/// A patch of the ordinary preferences.
///
/// An omitted field stays unchanged, and so does a field sent as `null`: for an ordinary
/// preference `null` is a no-op, not a clear. Only the two clearable preferences,
/// `release_channel` and `pac_url`, treat an explicit `null` as "clear the value"
/// (a double option), and `window_close` is a nested patch that names only the window kinds it
/// changes.
///
/// It is decoded only through the full patch ([`TryFrom`]), so unrelated unknown keys are ignored
/// as before and a privileged field is refused instead of silently dropped.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default)]
pub struct ApplicationPreferencesPatch {
    pub app_singleton_port: Option<u16>,
    pub app_log_level: Option<LoggingLevel>,
    pub core_logs: Option<CoreLogSettings>,
    pub language: Option<I18nLanguage>,
    pub theme_mode: Option<ThemeMode>,
    pub traffic_graph: Option<bool>,
    pub enable_memory_usage: Option<bool>,
    pub lighten_animation_effects: Option<bool>,
    pub enable_auto_launch: Option<bool>,
    pub enable_silent_start: Option<bool>,
    pub enable_system_proxy: Option<bool>,
    pub enable_proxy_guard: Option<bool>,
    pub system_proxy_bypass: Option<String>,
    pub proxy_guard_interval: Option<u64>,
    #[specta(type = Option<String>)]
    pub theme_color: Option<CssColor>,
    pub core: Option<ClashCore>,
    pub default_latency_test: Option<String>,
    pub default_latency_timeout_ms: Option<u64>,
    pub enable_builtin_enhanced: Option<bool>,
    pub proxy_layout_column: Option<i32>,
    pub max_log_files: Option<usize>,
    pub max_log_file_size: Option<u64>,
    pub enable_auto_check_update: Option<bool>,
    pub enable_auto_download_update: Option<bool>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<ReleaseChannel>>)]
    pub release_channel: Option<Option<ReleaseChannel>>,
    pub update_sources: Option<Vec<UpdateSource>>,
    pub tray_selector_mode: Option<ProxiesSelectorMode>,
    pub always_on_top: Option<bool>,
    pub tray_menu_mode: Option<TrayMenuMode>,
    pub window_close: WindowCloseSettingsPatch,
    pub network_statistic_widget: Option<NetworkStatisticWidgetConfig>,
    pub network_statistic_widget_logo: Option<NetworkStatisticWidgetLogo>,
    pub traffic_retention: Option<TrafficRetention>,
    pub enable_local_ip_probe: Option<bool>,
    pub stun_servers: Option<Vec<StunServer>>,
    pub ip_lookup_providers: Option<Vec<IpLookupProvider>>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<Url>>)]
    pub pac_url: Option<Option<Url>>,
    pub enable_tray_text: Option<bool>,
    pub enable_tray_traffic: Option<bool>,
    pub use_legacy_ui: Option<bool>,
    // Kept out of the TS bindings: a macOS-only field would make the generated bindings differ
    // per platform.
    #[cfg(target_os = "macos")]
    #[specta(skip)]
    pub enable_macos_colored_icons: Option<bool>,
}

/// A privileged field was present in a preferences patch.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("preferences cannot write {fields:?}"))]
pub struct PreferencesFieldsRejected {
    /// The serialized names of the refused fields, in declaration order.
    pub fields: Vec<&'static str>,
}

impl TryFrom<NyanpasuAppConfigPatch> for ApplicationPreferencesPatch {
    type Error = PreferencesFieldsRejected;

    /// Projects the permitted fields. Any present privileged field, whatever its value, refuses
    /// the whole patch.
    ///
    /// The destructure names every field of the full patch, so a field added to the config must
    /// be classified here as a preference or as a privileged setting before this compiles.
    fn try_from(patch: NyanpasuAppConfigPatch) -> Result<Self, Self::Error> {
        let NyanpasuAppConfigPatch {
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
        } = patch;

        let fields: Vec<&'static str> = [
            (hotkeys.is_some(), "hotkeys"),
            (enable_service_mode.is_some(), "enable_service_mode"),
            (remote_access.is_some(), "remote_access"),
        ]
        .into_iter()
        .filter_map(|(present, name)| present.then_some(name))
        .collect();
        if !fields.is_empty() {
            return Err(PreferencesFieldsRejected { fields });
        }

        Ok(Self {
            app_singleton_port,
            app_log_level,
            core_logs,
            language,
            theme_mode,
            traffic_graph,
            enable_memory_usage,
            lighten_animation_effects,
            enable_auto_launch,
            enable_silent_start,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            theme_color,
            core,
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
            enable_local_ip_probe,
            stun_servers,
            ip_lookup_providers,
            pac_url,
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
        })
    }
}

impl From<ApplicationPreferencesPatch> for NyanpasuAppConfigPatch {
    /// The full patch that writes exactly the preferences. The privileged fields stay unset.
    fn from(preferences: ApplicationPreferencesPatch) -> Self {
        let ApplicationPreferencesPatch {
            app_singleton_port,
            app_log_level,
            core_logs,
            language,
            theme_mode,
            traffic_graph,
            enable_memory_usage,
            lighten_animation_effects,
            enable_auto_launch,
            enable_silent_start,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            theme_color,
            core,
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
            enable_local_ip_probe,
            stun_servers,
            ip_lookup_providers,
            pac_url,
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
        } = preferences;

        Self {
            app_singleton_port,
            app_log_level,
            core_logs,
            language,
            theme_mode,
            traffic_graph,
            enable_memory_usage,
            lighten_animation_effects,
            enable_auto_launch,
            enable_silent_start,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            theme_color,
            core,
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
            enable_local_ip_probe,
            stun_servers,
            ip_lookup_providers,
            pac_url,
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
            // Privileged settings are not preferences and are never written from here.
            enable_service_mode: None,
            hotkeys: None,
            remote_access: None,
        }
    }
}

impl<'de> Deserialize<'de> for ApplicationPreferencesPatch {
    /// Decodes the full patch first, so omitted and `null` fields, aliases and unknown keys behave
    /// exactly as before, then refuses a present privileged field before building the contract.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        NyanpasuAppConfigPatch::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn decode(value: serde_json::Value) -> Result<ApplicationPreferencesPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    #[test]
    fn every_privileged_field_refuses_the_whole_patch_whatever_its_value() {
        for (field, values) in [
            (
                "hotkeys",
                vec![json!([]), json!(["open_or_close_dashboard,Ctrl+Q"])],
            ),
            ("enable_service_mode", vec![json!(false), json!(true)]),
            (
                "remote_access",
                vec![
                    json!({}),
                    json!({"http": {"enabled": false}}),
                    json!({"http": {"enabled": true}}),
                ],
            ),
        ] {
            for value in values {
                // Mixed with an allowed field: nothing of the patch is accepted.
                let error = decode(json!({field: value, "proxy_layout_column": 9})).unwrap_err();
                assert!(
                    error.contains(&format!("[\"{field}\"]")),
                    "{field}: {error}"
                );
            }
        }
        let error = decode(json!({
            "remote_access": {}, "enable_service_mode": false, "hotkeys": []
        }))
        .unwrap_err();
        assert!(error.contains("[\"hotkeys\", \"enable_service_mode\", \"remote_access\"]"));
    }

    #[test]
    fn omitted_and_null_fields_keep_their_meaning() {
        let patch = decode(json!({
            "hotkeys": null, "enable_service_mode": null, "remote_access": null,
            "proxy_layout_column": 7,
            "release_channel": null,
            "pac_url": null,
            "theme_color": null,
            "window_close": {"global": "hide"},
        }))
        .unwrap();
        let full = NyanpasuAppConfigPatch::from(patch);
        assert_eq!(full.proxy_layout_column, Some(7));
        // A null privileged field writes nothing; an explicit null of a clearable preference clears it.
        assert!(full.hotkeys.is_none() && full.enable_service_mode.is_none());
        assert!(full.remote_access.is_none());
        // Ordinary null is a no-op ...
        assert!(full.theme_color.is_none());
        // ... while the two clearable preferences clear on an explicit null.
        assert_eq!(full.release_channel, Some(None));
        assert_eq!(full.pac_url, Some(None));
        // The nested patch carries only the window kinds that were named.
        assert!(full.window_close.global.is_some() && full.window_close.main.is_none());
        let untouched = decode(json!({})).unwrap();
        let full = NyanpasuAppConfigPatch::from(untouched);
        assert!(full.release_channel.is_none() && full.pac_url.is_none());
    }

    #[test]
    fn unrelated_unknown_keys_and_old_aliases_are_still_accepted() {
        let patch = decode(json!({
            "a_future_setting": true, "remoteAccess": {"http": {"enabled": true}},
            "clash_core": "mihomo", "proxy_guard_duration": 15,
        }))
        .unwrap();
        assert_eq!(patch.proxy_guard_interval, Some(15));
        assert!(patch.core.is_some());
        assert!(NyanpasuAppConfigPatch::from(patch).remote_access.is_none());
    }

    #[test]
    fn the_converted_patch_writes_only_preferences() {
        let patch = decode(json!({"language": "en", "window_close": {"global": "hide"}})).unwrap();
        let full = NyanpasuAppConfigPatch::from(patch);
        assert!(full.language.is_some());
        assert!(full.hotkeys.is_none() && full.enable_service_mode.is_none());
        assert!(full.remote_access.is_none());
    }

    #[test]
    fn the_schema_describes_only_the_permitted_fields() {
        let mut types = specta::Types::default();
        let definition = <ApplicationPreferencesPatch as Type>::definition(&mut types);
        let rendered = format!("{definition:?}{types:?}");
        for forbidden in ["hotkeys", "enable_service_mode", "remote_access"] {
            assert!(!rendered.contains(forbidden), "{forbidden} is advertised");
        }
        assert!(rendered.contains("proxy_layout_column"));
    }
}
