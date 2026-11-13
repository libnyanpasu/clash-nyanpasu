//! The Clash settings as request contracts, one per privilege class.
//!
//! [`ClashConfigPatch`] can write every field of [`super::ClashConfig`]. The control plane (the
//! controller, the listeners and the ports, TUN and transparent capture) changes what the host
//! exposes, so it is the `system` class; the remaining fields are ordinary settings. See
//! [`crate::request`] for the shared mechanism.

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use super::{
    ClashConfigPatch, ClashControlChannel, TransparentProxyConfigPatch,
    clash_strategy::{
        BreakConnectionStrategyPatch, ExternalControllerStrategyPatch, PortStrategy,
        PortStrategyPatch,
    },
    overrides::TunOverridesPatch,
    tun_stack::{MipsConfigPatch, TunStack},
};
use crate::request::{FieldsRejected, reject_present};

/// A patch of the ordinary Clash settings: how the application treats the core's profiles and
/// connections. It cannot change anything the core listens on or exposes.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default, rename_all = "snake_case")]
pub struct ClashSettingsPatch {
    pub expand_include_all: Option<bool>,
    pub break_connection: BreakConnectionStrategyPatch,
}

/// A patch of the Clash control plane: the controller, the listening ports, TUN, transparent
/// capture, the web UI templates and the field filter.
///
/// The whole `overrides` record is in no Clash request: replacing it also writes the proxy mode and
/// the ordinary overrides, which are other classes. Overrides are written through the per-class
/// override requests.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default, rename_all = "snake_case")]
pub struct ClashSystemPatch {
    pub enable_tun_mode: Option<bool>,
    pub tun: TunOverridesPatch,
    pub web_ui_list: Option<Vec<String>>,
    pub enable_clash_fields: Option<bool>,
    pub external_controller: ExternalControllerStrategyPatch,
    pub clash_control_channel: Option<ClashControlChannel>,
    pub clash_ipc_disable_http_controller: Option<bool>,
    pub mixed_port: PortStrategyPatch,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<PortStrategy>>)]
    pub socks_port: Option<Option<PortStrategy>>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<PortStrategy>>)]
    pub http_port: Option<Option<PortStrategy>>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<PortStrategy>>)]
    pub redir_port: Option<Option<PortStrategy>>,
    #[serde(with = "::serde_with::rust::double_option")]
    #[specta(type = Option<Option<PortStrategy>>)]
    pub tproxy_port: Option<Option<PortStrategy>>,
    pub transparent_proxy: TransparentProxyConfigPatch,
    pub tun_stack: Option<TunStack>,
    pub tun_mips: MipsConfigPatch,
}

impl TryFrom<ClashConfigPatch> for ClashSettingsPatch {
    type Error = FieldsRejected;

    /// Projects the ordinary settings. Any present control-plane field or whole override record,
    /// whatever its value, refuses the whole patch.
    ///
    /// The destructure names every field of the full patch, so a field added to the config must
    /// be classified here and in [`ClashSystemPatch`] before this compiles.
    fn try_from(patch: ClashConfigPatch) -> Result<Self, Self::Error> {
        let ClashConfigPatch {
            overrides,
            enable_tun_mode,
            tun,
            web_ui_list,
            enable_clash_fields,
            expand_include_all,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            break_connection,
            tun_stack,
            tun_mips,
        } = patch;

        reject_present!(
            "clash settings";
            overrides,
            enable_tun_mode,
            web_ui_list,
            enable_clash_fields,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            tun_stack;
            nested: tun, external_controller, mixed_port, transparent_proxy, tun_mips,
        )?;

        Ok(Self {
            expand_include_all,
            break_connection,
        })
    }
}

impl From<ClashSettingsPatch> for ClashConfigPatch {
    /// The full patch that writes exactly the ordinary settings.
    fn from(settings: ClashSettingsPatch) -> Self {
        let ClashSettingsPatch {
            expand_include_all,
            break_connection,
        } = settings;

        Self {
            expand_include_all,
            break_connection,
            ..Self::default()
        }
    }
}

impl TryFrom<ClashConfigPatch> for ClashSystemPatch {
    type Error = FieldsRejected;

    /// Projects the control-plane fields. A present ordinary setting or whole override record,
    /// whatever its value, refuses the whole patch.
    fn try_from(patch: ClashConfigPatch) -> Result<Self, Self::Error> {
        let ClashConfigPatch {
            overrides,
            enable_tun_mode,
            tun,
            web_ui_list,
            enable_clash_fields,
            expand_include_all,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            break_connection,
            tun_stack,
            tun_mips,
        } = patch;

        reject_present!(
            "clash system settings";
            overrides,
            expand_include_all;
            nested: break_connection,
        )?;

        Ok(Self {
            enable_tun_mode,
            tun,
            web_ui_list,
            enable_clash_fields,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            tun_stack,
            tun_mips,
        })
    }
}

impl From<ClashSystemPatch> for ClashConfigPatch {
    /// The full patch that writes exactly the control-plane fields.
    fn from(system: ClashSystemPatch) -> Self {
        let ClashSystemPatch {
            enable_tun_mode,
            tun,
            web_ui_list,
            enable_clash_fields,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            tun_stack,
            tun_mips,
        } = system;

        Self {
            enable_tun_mode,
            tun,
            web_ui_list,
            enable_clash_fields,
            external_controller,
            clash_control_channel,
            clash_ipc_disable_http_controller,
            mixed_port,
            socks_port,
            http_port,
            redir_port,
            tproxy_port,
            transparent_proxy,
            tun_stack,
            tun_mips,
            ..Self::default()
        }
    }
}

macro_rules! decode_through_the_full_patch {
    ($request:ty) => {
        impl<'de> Deserialize<'de> for $request {
            /// Decodes the full patch first, so omitted and `null` fields, aliases and unknown
            /// keys behave exactly as before, then refuses a present field of another class.
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                ClashConfigPatch::deserialize(deserializer)?
                    .try_into()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}
decode_through_the_full_patch!(ClashSettingsPatch);
decode_through_the_full_patch!(ClashSystemPatch);

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::{clash::config::ClashConfig, request::testing::assert_refused};

    fn settings(value: Value) -> Result<ClashSettingsPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    fn system(value: Value) -> Result<ClashSystemPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    /// The fields the whole override record is the only way to write: neither request carries it.
    const DEDICATED: [&str; 1] = ["overrides"];

    fn default_config() -> Value {
        serde_json::to_value(ClashConfig::default()).unwrap()
    }

    /// The control-plane fields with the serialized default (the same value) and, where it
    /// differs, a valid value that changes it. Each one is proven valid for the full patch by
    /// the refusal helper.
    fn system_fields() -> Vec<(&'static str, Value)> {
        let config = default_config();
        let mut fields: Vec<_> = [
            "enable_tun_mode",
            "tun",
            "web_ui_list",
            "enable_clash_fields",
            "external_controller",
            "clash_control_channel",
            "clash_ipc_disable_http_controller",
            "mixed_port",
            "socks_port",
            "http_port",
            "redir_port",
            "tproxy_port",
            "transparent_proxy",
            "tun_stack",
            "tun_mips",
        ]
        .into_iter()
        .map(|field| (field, config[field].clone()))
        .collect();
        fields.extend([
            ("enable_tun_mode", json!(false)),
            ("enable_tun_mode", json!(true)),
            ("web_ui_list", json!([])),
            ("enable_clash_fields", json!(false)),
            ("clash_ipc_disable_http_controller", json!(false)),
            ("socks_port", json!(null)),
            ("socks_port", json!({"kind": "fixed", "start_port": 7891})),
            ("tun", json!({"auto-route": {"kind": "unmanaged"}})),
            ("transparent_proxy", json!({"local": true})),
            ("mixed_port", json!({"start_port": 7890})),
        ]);
        fields
    }

    fn settings_fields() -> Vec<(&'static str, Value)> {
        vec![
            ("expand_include_all", json!(true)),
            ("expand_include_all", json!(false)),
            ("break_connection", json!({"on_profile_change": false})),
            ("break_connection", json!({"on_profile_change": true})),
        ]
    }

    /// A valid whole override record: the default, and one that changes the proxy mode.
    fn override_records() -> Vec<Value> {
        let default = default_config()["overrides"].clone();
        let mut changed = default.clone();
        changed["mode"] = json!("global");
        vec![default, changed]
    }

    #[test]
    fn a_settings_request_refuses_every_control_plane_field_whatever_its_value() {
        for (field, value) in system_fields() {
            assert_refused::<ClashConfigPatch, ClashSettingsPatch>(
                "clash settings",
                field,
                json!({ field: value }),
            );
            // A mixed patch is refused as a whole.
            assert_refused::<ClashConfigPatch, ClashSettingsPatch>(
                "clash settings",
                field,
                json!({ field: value, "expand_include_all": false }),
            );
        }
        let error = settings(json!({"enable_tun_mode": false, "tun_stack": "system"})).unwrap_err();
        assert!(
            error.contains("[\"enable_tun_mode\", \"tun_stack\"]"),
            "{error}"
        );
    }

    #[test]
    fn a_system_request_refuses_every_ordinary_setting_whatever_its_value() {
        for (field, value) in settings_fields() {
            assert_refused::<ClashConfigPatch, ClashSystemPatch>(
                "clash system settings",
                field,
                json!({ field: value }),
            );
            assert_refused::<ClashConfigPatch, ClashSystemPatch>(
                "clash system settings",
                field,
                json!({ field: value, "enable_tun_mode": false }),
            );
        }
        assert!(
            system(json!({"break_connection": {}})).is_ok(),
            "an empty nested patch names nothing"
        );
    }

    #[test]
    fn neither_request_carries_the_whole_override_record() {
        for record in override_records() {
            for body in [
                json!({ "overrides": record }),
                json!({ "overrides": record, "enable_tun_mode": false }),
                json!({ "overrides": record, "expand_include_all": false }),
            ] {
                assert_refused::<ClashConfigPatch, ClashSystemPatch>(
                    "clash system settings",
                    "overrides",
                    body.clone(),
                );
                assert_refused::<ClashConfigPatch, ClashSettingsPatch>(
                    "clash settings",
                    "overrides",
                    body,
                );
            }
        }
        // `null` writes nothing, so it is the one spelling that passes.
        assert!(system(json!({"overrides": null})).is_ok());
        assert!(settings(json!({"overrides": null})).is_ok());
    }

    #[test]
    fn omitted_null_and_nested_fields_keep_their_meaning() {
        let patch = settings(json!({
            "expand_include_all": null,
            "break_connection": {"on_profile_change": false},
            "a_future_setting": 1,
        }))
        .unwrap();
        let full = ClashConfigPatch::from(patch);
        assert!(full.expand_include_all.is_none());
        assert!(full.break_connection.on_profile_change.is_some());

        let patch = system(json!({
            "enable_tun_mode": null,
            "socks_port": null,
            "http_port": {"kind": "fixed", "start_port": 7891},
            "mixed_port": {"start_port": 7000},
            "tun": {"auto-route": {"kind": "unmanaged"}},
        }))
        .unwrap();
        let full = ClashConfigPatch::from(patch);
        assert!(full.enable_tun_mode.is_none(), "null writes nothing");
        assert_eq!(
            full.socks_port,
            Some(None),
            "a clearable port clears on null"
        );
        assert!(full.redir_port.is_none(), "an omitted port stays");
        assert_eq!(full.mixed_port.start_port, Some(7000));
        assert!(
            full.mixed_port.kind.is_none(),
            "a nested patch names only its leaves"
        );
        assert!(full.http_port.is_some());
        assert!(full.tun.auto_route.is_some());
        assert!(
            full.overrides.is_none(),
            "a system patch never writes the override record"
        );
    }

    #[test]
    fn the_converted_patch_writes_only_its_own_class() {
        let config = default_config();
        let mut body = serde_json::Map::new();
        let mut expected = Vec::new();
        for (field, _) in system_fields() {
            if !expected.contains(&field) {
                expected.push(field);
                body.insert(field.into(), config[field].clone());
            }
        }
        let patch = ClashConfigPatch::from(system(Value::Object(body)).unwrap());
        let written = written_keys(&patch);
        assert_eq!(written.len(), expected.len(), "{written:?}");
        assert!(
            expected
                .iter()
                .all(|field| written.contains(&field.to_string()))
        );

        let patch = ClashConfigPatch::from(
            settings(json!({"expand_include_all": false, "break_connection": {"on_profile_change": false}}))
                .unwrap(),
        );
        assert_eq!(
            written_keys(&patch),
            ["break_connection", "expand_include_all"]
        );
    }

    /// The keys a patch writes: those that differ from the empty patch, which names none.
    fn written_keys(patch: &ClashConfigPatch) -> Vec<String> {
        let empty = serde_json::to_value(ClashConfigPatch::default()).unwrap();
        let mut keys: Vec<_> = serde_json::to_value(patch)
            .unwrap()
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| empty.get(key.as_str()) != Some(*value))
            .map(|(key, _)| key.clone())
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn the_schemas_describe_only_the_permitted_fields() {
        fn render<T: Type>() -> String {
            let mut types = specta::Types::default();
            let definition = T::definition(&mut types);
            format!("{definition:?}{types:?}")
        }
        let settings_schema = render::<ClashSettingsPatch>();
        let system_schema = render::<ClashSystemPatch>();
        for (field, _) in system_fields() {
            assert!(!settings_schema.contains(&format!("{field:?}")), "{field}");
            assert!(system_schema.contains(&format!("{field:?}")), "{field}");
        }
        for (field, _) in settings_fields() {
            assert!(settings_schema.contains(&format!("{field:?}")), "{field}");
            assert!(!system_schema.contains(&format!("{field:?}")), "{field}");
        }
        for field in DEDICATED {
            assert!(!settings_schema.contains(&format!("{field:?}")), "{field}");
            assert!(!system_schema.contains(&format!("{field:?}")), "{field}");
        }
    }

    /// Each configured field is written by exactly one request type, or is dedicated and written
    /// by none of them. The sample is the serialized default config, so a newly added field
    /// takes part automatically.
    #[test]
    fn every_config_field_has_exactly_one_request_type() {
        for (field, value) in default_config().as_object().unwrap() {
            let body = json!({ field: value });
            let by_settings = serde_json::from_value::<ClashSettingsPatch>(body.clone())
                .map(|patch| written_keys(&patch.into()).contains(field))
                .unwrap_or(false);
            let by_system = serde_json::from_value::<ClashSystemPatch>(body)
                .map(|patch| written_keys(&patch.into()).contains(field))
                .unwrap_or(false);
            let expected = usize::from(!DEDICATED.contains(&field.as_str()));
            assert_eq!(
                usize::from(by_settings) + usize::from(by_system),
                expected,
                "{field}: settings={by_settings} system={by_system}"
            );
        }
    }
}
