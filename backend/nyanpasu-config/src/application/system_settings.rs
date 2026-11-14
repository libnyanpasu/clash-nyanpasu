//! The application settings that change how the host system behaves, as one request contract.
//!
//! They start with the OS integration the application manages for the user (autostart, the
//! system proxy, its guard and bypass list, the PAC URL). Ordinary preferences are
//! [`super::ApplicationPreferencesPatch`]; hotkeys, service mode, remote access and core
//! selection have dedicated commands.

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use url::Url;

use crate::request::{FieldsRejected, reject_present};

use super::NyanpasuAppConfigPatch;

/// A patch of the system-affecting application settings.
///
/// An omitted field stays unchanged, and so does a field sent as `null`; only `pac_url` treats
/// an explicit `null` as "clear the value" (a double option). Unrelated unknown keys and old
/// aliases behave as before, and a field of another class refuses the whole patch instead of being
/// silently dropped.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default)]
pub struct SystemSettingsPatch {
    pub enable_auto_launch: Option<bool>,
    pub enable_system_proxy: Option<bool>,
    pub enable_proxy_guard: Option<bool>,
    pub system_proxy_bypass: Option<String>,
    pub proxy_guard_interval: Option<u64>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<Url>>)]
    pub pac_url: Option<Option<Url>>,
}

impl TryFrom<NyanpasuAppConfigPatch> for SystemSettingsPatch {
    type Error = FieldsRejected;

    /// Projects the system-affecting fields. Any present field of another class, whatever its
    /// value, refuses the whole patch.
    ///
    /// The destructure names every field of the full patch, so a field added to the config must
    /// be classified here before this compiles.
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
            "system settings";
            hotkeys,
            enable_service_mode,
            remote_access,
            core,
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
            enable_macos_colored_icons;
            nested: window_close,
        )?;

        Ok(Self {
            enable_auto_launch,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            pac_url,
        })
    }
}

impl From<SystemSettingsPatch> for NyanpasuAppConfigPatch {
    /// The full patch that writes exactly the system-affecting settings. Every other field stays
    /// unset.
    fn from(system: SystemSettingsPatch) -> Self {
        let SystemSettingsPatch {
            enable_auto_launch,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            pac_url,
        } = system;

        Self {
            enable_auto_launch,
            enable_system_proxy,
            enable_proxy_guard,
            system_proxy_bypass,
            proxy_guard_interval,
            pac_url,
            ..Self::default()
        }
    }
}

impl<'de> Deserialize<'de> for SystemSettingsPatch {
    /// Decodes the full patch first, so omitted and `null` fields, aliases and unknown keys behave
    /// exactly as before, then refuses a present field of another class before building the
    /// contract.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        NyanpasuAppConfigPatch::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::{application::NyanpasuAppConfig, request::testing::assert_refused};

    fn decode(value: Value) -> Result<SystemSettingsPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    /// The serialized default config: every value is a valid spelling and a same-value write.
    fn default_config() -> Value {
        serde_json::to_value(NyanpasuAppConfig::default()).unwrap()
    }

    /// Every field the system request refuses, with the serialized default and, where it
    /// differs, a valid value that changes it. The refusal helper proves each body is a valid
    /// unrestricted patch before it asserts the narrow request refuses it.
    fn refused_fields() -> Vec<(&'static str, Value)> {
        let config = default_config();
        let mut fields: Vec<_> = REFUSED
            .into_iter()
            .map(|field| (field, config[field].clone()))
            .collect();
        fields.extend([
            ("hotkeys", json!([])),
            ("enable_service_mode", json!(false)),
            ("enable_service_mode", json!(true)),
            ("remote_access", json!({})),
            ("remote_access", json!({"http": {"enabled": false}})),
            ("core", json!("mihomo")),
            ("language", json!("en")),
            ("theme_mode", json!("light")),
            ("traffic_graph", json!(false)),
            ("enable_memory_usage", json!(false)),
            ("default_latency_timeout_ms", json!(0)),
            ("max_log_files", json!(0)),
            ("update_sources", json!([])),
            ("stun_servers", json!([])),
            ("release_channel", json!(null)),
            ("window_close", json!({"global": "hide"})),
        ]);
        fields
    }

    const REFUSED: [&str; 35] = [
        "hotkeys",
        "enable_service_mode",
        "remote_access",
        "core",
        "app_singleton_port",
        "app_log_level",
        "core_logs",
        "language",
        "theme_mode",
        "traffic_graph",
        "enable_memory_usage",
        "lighten_animation_effects",
        "enable_silent_start",
        "theme_color",
        "default_latency_test",
        "default_latency_timeout_ms",
        "enable_builtin_enhanced",
        "proxy_layout_column",
        "max_log_files",
        "max_log_file_size",
        "enable_auto_check_update",
        "enable_auto_download_update",
        "release_channel",
        "update_sources",
        "tray_selector_mode",
        "always_on_top",
        "tray_menu_mode",
        "window_close",
        "traffic_retention",
        "enable_local_ip_probe",
        "stun_servers",
        "ip_lookup_providers",
        "enable_tray_text",
        "enable_tray_traffic",
        "use_legacy_ui",
    ];

    #[test]
    fn every_other_class_field_refuses_the_whole_patch_whatever_its_value() {
        for (field, value) in refused_fields() {
            assert_refused::<NyanpasuAppConfigPatch, SystemSettingsPatch>(
                "system settings",
                field,
                json!({ field: value }),
            );
            // Mixed with an allowed field: nothing of the patch is accepted.
            assert_refused::<NyanpasuAppConfigPatch, SystemSettingsPatch>(
                "system settings",
                field,
                json!({ field: value, "enable_system_proxy": true }),
            );
        }
        let error = decode(json!({
            "hotkeys": [], "enable_service_mode": false, "remote_access": {}, "core": "mihomo",
            "language": "en"
        }))
        .unwrap_err();
        assert!(error.contains(
            "[\"hotkeys\", \"enable_service_mode\", \"remote_access\", \"core\", \"language\"]"
        ));
    }

    #[test]
    fn omitted_and_null_fields_keep_their_meaning() {
        let mut body = json!({"enable_system_proxy": false, "pac_url": null});
        // A null field of another class writes nothing, so it is accepted.
        for (field, _) in refused_fields() {
            // A nested patch is an object, never null.
            if field != "release_channel" && field != "window_close" {
                body[field] = Value::Null;
            }
        }
        let patch = decode(body).unwrap();
        let full = NyanpasuAppConfigPatch::from(patch);
        assert_eq!(full.enable_system_proxy, Some(false));
        // The clearable setting clears on an explicit null ...
        assert_eq!(full.pac_url, Some(None));
        // ... every other null is a no-op ...
        assert!(full.enable_auto_launch.is_none() && full.system_proxy_bypass.is_none());
        // ... and an omitted field stays unchanged.
        let untouched = NyanpasuAppConfigPatch::from(decode(json!({})).unwrap());
        assert!(untouched.pac_url.is_none());
        // `release_channel` clears on null, so it is a write of another class and is refused.
        let error = decode(json!({"release_channel": null})).unwrap_err();
        assert!(error.contains("\"release_channel\""), "{error}");
    }

    #[test]
    fn unrelated_unknown_keys_and_old_aliases_are_still_accepted() {
        let patch = decode(json!({
            "a_future_setting": true, "remoteAccess": null,
            "proxy_guard_duration": 15,
        }))
        .unwrap();
        assert_eq!(patch.proxy_guard_interval, Some(15));
        // The `clash_core` alias names a field of another class and stays refused under it.
        let error = decode(json!({"clash_core": "mihomo"})).unwrap_err();
        assert!(error.contains("\"core\""), "{error}");
    }

    #[test]
    fn the_converted_patch_writes_only_system_settings() {
        let patch = decode(json!({
            "enable_auto_launch": false, "enable_system_proxy": false, "enable_proxy_guard": false,
            "system_proxy_bypass": "", "proxy_guard_interval": 0,
            "pac_url": "http://127.0.0.1/proxy.pac",
        }))
        .unwrap();
        let full = serde_json::to_value(NyanpasuAppConfigPatch::from(patch)).unwrap();
        let mut full = full.as_object().unwrap().clone();
        // The nested window patch serializes as an empty object: it names no leaf.
        assert_eq!(full.remove("window_close"), Some(json!({})));
        let mut keys: Vec<_> = full.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "enable_auto_launch",
                "enable_proxy_guard",
                "enable_system_proxy",
                "pac_url",
                "proxy_guard_interval",
                "system_proxy_bypass"
            ]
        );
    }

    #[test]
    fn the_schema_describes_only_the_permitted_fields() {
        let mut types = specta::Types::default();
        let definition = <SystemSettingsPatch as Type>::definition(&mut types);
        let rendered = format!("{definition:?}{types:?}");
        for (field, _) in refused_fields() {
            assert!(
                !rendered.contains(&format!("{field:?}")),
                "{field} is advertised"
            );
        }
        assert!(rendered.contains("\"enable_system_proxy\"") && rendered.contains("\"pac_url\""));
    }
}
