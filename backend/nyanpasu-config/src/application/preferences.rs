//! The ordinary application preferences, as one request contract.
//!
//! [`NyanpasuAppConfigPatch`] is the internal patch of the whole config. Its fields have three
//! privilege classes, and each class is written by its own command:
//!
//! - ordinary preferences (`settings`): [`ApplicationPreferencesPatch`], this module;
//! - system-affecting settings (`system`): [`super::SystemSettingsPatch`];
//! - dedicated settings with their own command: `hotkeys` (`set_hotkeys`), `enable_service_mode`
//!   (`set_service_mode`), `remote_access` (`set_remote_access`) and `core`
//!   (`change_clash_core`).
//!
//! Every request type accepts only its own fields, so its fields, and its schema, are exactly the
//! permitted ones.

use csscolorparser::Color as CssColor;
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::request::{FieldsRejected, reject_present};

use super::{
    CoreLogSettings, I18nLanguage, IpLookupProvider, LoggingLevel, NetworkStatisticWidgetConfig,
    NetworkStatisticWidgetLogo, NyanpasuAppConfigPatch, ProxiesSelectorMode, ReleaseChannel,
    StunServer, ThemeMode, TrafficRetention, TrayMenuMode, UpdateSource, WindowCloseSettingsPatch,
};

/// A patch of the ordinary preferences.
///
/// An omitted field stays unchanged, and so does a field sent as `null`: for an ordinary
/// preference `null` is a no-op, not a clear. Only `release_channel` treats an explicit `null`
/// as "clear the value" (a double option), and `window_close` is a nested patch that names only
/// the window kinds it changes.
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
    pub enable_silent_start: Option<bool>,
    #[specta(type = Option<String>)]
    pub theme_color: Option<CssColor>,
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
    pub enable_tray_text: Option<bool>,
    pub enable_tray_traffic: Option<bool>,
    pub use_legacy_ui: Option<bool>,
    // Kept out of the TS bindings: a macOS-only field would make the generated bindings differ
    // per platform.
    #[cfg(target_os = "macos")]
    #[specta(skip)]
    pub enable_macos_colored_icons: Option<bool>,
}

impl TryFrom<NyanpasuAppConfigPatch> for ApplicationPreferencesPatch {
    type Error = FieldsRejected;

    /// Projects the permitted fields. Any present field of another class, whatever its value,
    /// refuses the whole patch.
    ///
    /// The destructure names every field of the full patch, so a field added to the config must
    /// be classified here as a preference or as another class's field before this compiles.
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

        reject_present!(
            "preferences";
            hotkeys,
            enable_service_mode,
            remote_access,
            core,
            enable_auto_launch,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            pac_url,
        )?;

        Ok(Self {
            app_singleton_port,
            app_log_level,
            core_logs,
            language,
            theme_mode,
            traffic_graph,
            enable_memory_usage,
            lighten_animation_effects,
            enable_silent_start,
            theme_color,
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
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
        })
    }
}

impl From<ApplicationPreferencesPatch> for NyanpasuAppConfigPatch {
    /// The full patch that writes exactly the preferences. Every other field stays unset.
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
            enable_silent_start,
            theme_color,
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
            enable_silent_start,
            theme_color,
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
            enable_tray_text,
            enable_tray_traffic,
            use_legacy_ui,
            #[cfg(target_os = "macos")]
            enable_macos_colored_icons,
            // Other classes are never written from here.
            enable_service_mode: None,
            enable_auto_launch: None,
            enable_system_proxy: None,
            enable_proxy_guard: None,
            system_proxy_bypass: None,
            proxy_guard_interval: None,
            core: None,
            hotkeys: None,
            remote_access: None,
            pac_url: None,
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
    use crate::request::testing::assert_refused;

    fn decode(value: serde_json::Value) -> Result<ApplicationPreferencesPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    /// Every field the preferences request refuses, with values that change nothing (false,
    /// empty, the default): a refusal never depends on the value.
    fn refused_fields() -> Vec<(&'static str, Vec<serde_json::Value>)> {
        vec![
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
            ("core", vec![json!("mihomo"), json!("clash-rs")]),
            ("enable_auto_launch", vec![json!(false), json!(true)]),
            ("enable_system_proxy", vec![json!(false), json!(true)]),
            ("enable_proxy_guard", vec![json!(false), json!(true)]),
            (
                "system_proxy_bypass",
                vec![json!(""), json!("localhost;127.*")],
            ),
            ("proxy_guard_interval", vec![json!(0), json!(10)]),
            (
                "pac_url",
                vec![json!("http://127.0.0.1/proxy.pac"), json!(null)],
            ),
        ]
    }

    #[test]
    fn every_other_class_field_refuses_the_whole_patch_whatever_its_value() {
        for (field, values) in refused_fields() {
            for value in values {
                assert_refused::<NyanpasuAppConfigPatch, ApplicationPreferencesPatch>(
                    "preferences",
                    field,
                    json!({ field: value }),
                );
                // Mixed with an allowed field: nothing of the patch is accepted.
                assert_refused::<NyanpasuAppConfigPatch, ApplicationPreferencesPatch>(
                    "preferences",
                    field,
                    json!({ field: value, "proxy_layout_column": 9 }),
                );
            }
        }
        let error = decode(json!({
            "remote_access": {}, "enable_service_mode": false, "hotkeys": [],
            "enable_system_proxy": false
        }))
        .unwrap_err();
        assert!(error.contains(
            "[\"hotkeys\", \"enable_service_mode\", \"remote_access\", \"enable_system_proxy\"]"
        ));
    }

    #[test]
    fn omitted_and_null_fields_keep_their_meaning() {
        let mut body = json!({
            "proxy_layout_column": 7,
            "release_channel": null,
            "theme_color": null,
            "window_close": {"global": "hide"},
        });
        // A null field of another class writes nothing, so it is accepted (`pac_url` is the one
        // field whose null clears, so it is a write and stays refused: see the refusal test).
        for (field, _) in refused_fields() {
            if field != "pac_url" {
                body[field] = serde_json::Value::Null;
            }
        }
        let patch = decode(body).unwrap();
        let full = NyanpasuAppConfigPatch::from(patch);
        assert_eq!(full.proxy_layout_column, Some(7));
        assert!(full.hotkeys.is_none() && full.enable_service_mode.is_none());
        assert!(full.remote_access.is_none() && full.core.is_none());
        // Ordinary null is a no-op ...
        assert!(full.theme_color.is_none());
        // ... while the clearable preference clears on an explicit null.
        assert_eq!(full.release_channel, Some(None));
        // The nested patch carries only the window kinds that were named.
        assert!(full.window_close.global.is_some() && full.window_close.main.is_none());
        let untouched = decode(json!({})).unwrap();
        assert!(
            NyanpasuAppConfigPatch::from(untouched)
                .release_channel
                .is_none()
        );
    }

    #[test]
    fn unrelated_unknown_keys_and_old_aliases_are_still_accepted() {
        let patch = decode(json!({
            "a_future_setting": true, "remoteAccess": null,
            "proxy_layout_column": 3,
        }))
        .unwrap();
        assert_eq!(patch.proxy_layout_column, Some(3));
        assert!(NyanpasuAppConfigPatch::from(patch).remote_access.is_none());
        // An alias of another class is still refused under its new name.
        for body in [
            json!({"clash_core": "mihomo"}),
            json!({"proxy_guard_duration": 15}),
        ] {
            let error = decode(body).unwrap_err();
            assert!(
                error.contains("\"core\"") || error.contains("\"proxy_guard_interval\""),
                "{error}"
            );
        }
    }

    #[test]
    fn the_converted_patch_writes_only_preferences() {
        let patch = decode(json!({"language": "en", "window_close": {"global": "hide"}})).unwrap();
        let full = serde_json::to_value(NyanpasuAppConfigPatch::from(patch)).unwrap();
        let mut keys: Vec<_> = full.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["language", "window_close"]);
    }

    #[test]
    fn the_schema_describes_only_the_permitted_fields() {
        let mut types = specta::Types::default();
        let definition = <ApplicationPreferencesPatch as Type>::definition(&mut types);
        let rendered = format!("{definition:?}{types:?}");
        for (field, _) in refused_fields() {
            // Quoted: `core` must not match `core_logs`.
            assert!(
                !rendered.contains(&format!("{field:?}")),
                "{field} is advertised"
            );
        }
        assert!(rendered.contains("\"proxy_layout_column\""));
    }
}
