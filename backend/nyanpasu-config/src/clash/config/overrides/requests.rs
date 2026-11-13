//! The override record as request contracts, one per privilege class.
//!
//! [`ClashGuardOverridesPatch`] can write every override. Changing the proxy mode is the `proxy`
//! class, what the core listens on and the controller secret are `system`, and the rest are
//! ordinary settings. See [`crate::request`] for the shared mechanism.

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

#[cfg(feature = "default-meta")]
use super::ManageableField;
use super::{ClashGuardOverridesPatch, LogLevel, Mode};
use crate::request::{FieldsRejected, reject_present};

/// A patch of the ordinary overrides: how verbose the core is and how it behaves toward IPv6 and
/// connection handling. It cannot change the proxy mode, what the core listens on, or the secret.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct ClashOverridesSettingsPatch {
    pub log_level: Option<LogLevel>,
    pub ipv6: Option<bool>,
    #[cfg(feature = "default-meta")]
    pub unified_delay: Option<ManageableField<bool>>,
    #[cfg(feature = "default-meta")]
    pub tcp_concurrent: Option<ManageableField<bool>>,
}

/// A patch of the controller exposure: whether the core accepts connections from the LAN, and the
/// secret that guards its controller.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct ClashAccessPatch {
    pub allow_lan: Option<bool>,
    pub secret: Option<String>,
}

impl TryFrom<ClashGuardOverridesPatch> for ClashOverridesSettingsPatch {
    type Error = FieldsRejected;

    /// Projects the ordinary overrides. A present proxy mode or controller exposure field,
    /// whatever its value, refuses the whole patch.
    ///
    /// The destructure names every field of the full patch, so a field added to the overrides must
    /// be classified here and in the other requests before this compiles.
    fn try_from(patch: ClashGuardOverridesPatch) -> Result<Self, Self::Error> {
        let ClashGuardOverridesPatch {
            log_level,
            allow_lan,
            mode,
            secret,
            #[cfg(feature = "default-meta")]
            unified_delay,
            #[cfg(feature = "default-meta")]
            tcp_concurrent,
            ipv6,
        } = patch;

        reject_present!("clash override settings"; mode, allow_lan as "allow-lan", secret)?;

        Ok(Self {
            log_level,
            ipv6,
            #[cfg(feature = "default-meta")]
            unified_delay,
            #[cfg(feature = "default-meta")]
            tcp_concurrent,
        })
    }
}

impl From<ClashOverridesSettingsPatch> for ClashGuardOverridesPatch {
    /// The full patch that writes exactly the ordinary overrides.
    fn from(settings: ClashOverridesSettingsPatch) -> Self {
        let ClashOverridesSettingsPatch {
            log_level,
            ipv6,
            #[cfg(feature = "default-meta")]
            unified_delay,
            #[cfg(feature = "default-meta")]
            tcp_concurrent,
        } = settings;

        Self {
            log_level,
            ipv6,
            #[cfg(feature = "default-meta")]
            unified_delay,
            #[cfg(feature = "default-meta")]
            tcp_concurrent,
            ..Self::default()
        }
    }
}

impl TryFrom<ClashGuardOverridesPatch> for ClashAccessPatch {
    type Error = FieldsRejected;

    /// Projects the controller exposure. A present proxy mode or ordinary override, whatever its
    /// value, refuses the whole patch.
    fn try_from(patch: ClashGuardOverridesPatch) -> Result<Self, Self::Error> {
        let ClashGuardOverridesPatch {
            log_level,
            allow_lan,
            mode,
            secret,
            #[cfg(feature = "default-meta")]
            unified_delay,
            #[cfg(feature = "default-meta")]
            tcp_concurrent,
            ipv6,
        } = patch;

        reject_present!(
            "clash access";
            mode,
            log_level as "log-level",
            ipv6,
            #[cfg(feature = "default-meta")]
            unified_delay as "unified-delay",
            #[cfg(feature = "default-meta")]
            tcp_concurrent as "tcp-concurrent",
        )?;

        Ok(Self { allow_lan, secret })
    }
}

impl From<ClashAccessPatch> for ClashGuardOverridesPatch {
    /// The full patch that writes exactly the controller exposure.
    fn from(access: ClashAccessPatch) -> Self {
        let ClashAccessPatch { allow_lan, secret } = access;

        Self {
            allow_lan,
            secret,
            ..Self::default()
        }
    }
}

impl From<Mode> for ClashGuardOverridesPatch {
    /// The full patch that writes exactly the proxy mode.
    fn from(mode: Mode) -> Self {
        Self {
            mode: Some(mode),
            ..Self::default()
        }
    }
}

macro_rules! decode_through_the_full_patch {
    ($request:ty) => {
        impl<'de> Deserialize<'de> for $request {
            /// Decodes the full patch first, so omitted and `null` fields and unknown keys
            /// behave exactly as before, then refuses a present field of another class.
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                ClashGuardOverridesPatch::deserialize(deserializer)?
                    .try_into()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}
decode_through_the_full_patch!(ClashOverridesSettingsPatch);
decode_through_the_full_patch!(ClashAccessPatch);

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{super::ClashGuardOverrides, *};
    use crate::request::testing::assert_refused;

    fn settings(value: Value) -> Result<ClashOverridesSettingsPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    fn access(value: Value) -> Result<ClashAccessPatch, String> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    fn written_keys(patch: &ClashGuardOverridesPatch) -> Vec<String> {
        let mut keys: Vec<_> = serde_json::to_value(patch)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// The serialized default record: every value is a valid spelling and a same-value write.
    fn record() -> serde_json::Map<String, Value> {
        serde_json::to_value(ClashGuardOverrides::default())
            .unwrap()
            .as_object()
            .unwrap()
            .clone()
    }

    /// Extra valid spellings that change the value, so a refusal is shown not to depend on it.
    fn changed_values() -> Vec<(&'static str, Value)> {
        vec![
            ("mode", json!("global")),
            ("allow-lan", json!(true)),
            ("secret", json!("")),
            ("secret", json!("changed")),
            ("log-level", json!("debug")),
            ("ipv6", json!(true)),
            #[cfg(feature = "default-meta")]
            ("unified-delay", json!({"kind": "unmanaged"})),
            #[cfg(feature = "default-meta")]
            ("unified-delay", json!({"kind": "managed", "value": false})),
            #[cfg(feature = "default-meta")]
            ("tcp-concurrent", json!({"kind": "managed", "value": true})),
        ]
    }

    #[test]
    fn the_settings_request_refuses_mode_and_access_whatever_their_value() {
        let samples = record()
            .into_iter()
            .map(|(field, value)| (field, value))
            .chain(changed_values().into_iter().map(|(f, v)| (f.to_owned(), v)));
        for (field, value) in samples {
            if !["mode", "allow-lan", "secret"].contains(&field.as_str()) {
                continue;
            }
            assert_refused::<ClashGuardOverridesPatch, ClashOverridesSettingsPatch>(
                "clash override settings",
                &field,
                json!({ &field: value }),
            );
            // A mixed patch is refused as a whole.
            assert_refused::<ClashGuardOverridesPatch, ClashOverridesSettingsPatch>(
                "clash override settings",
                &field,
                json!({ &field: value, "ipv6": false }),
            );
        }
        let error =
            settings(json!({"mode": "global", "secret": "x", "allow-lan": true})).unwrap_err();
        assert!(
            error.contains("[\"mode\", \"allow-lan\", \"secret\"]"),
            "{error}"
        );
    }

    #[test]
    fn the_access_request_refuses_mode_and_ordinary_overrides_whatever_their_value() {
        let samples = record()
            .into_iter()
            .chain(changed_values().into_iter().map(|(f, v)| (f.to_owned(), v)));
        for (field, value) in samples {
            if ["allow-lan", "secret"].contains(&field.as_str()) {
                continue;
            }
            assert_refused::<ClashGuardOverridesPatch, ClashAccessPatch>(
                "clash access",
                &field,
                json!({ &field: value }),
            );
            assert_refused::<ClashGuardOverridesPatch, ClashAccessPatch>(
                "clash access",
                &field,
                json!({ &field: value, "allow-lan": false }),
            );
        }
    }

    #[test]
    fn every_override_in_the_record_is_covered_by_the_refusal_matrices() {
        // The matrices above iterate the serialized record, so a new override is exercised; this
        // pins the shape they rely on.
        let record = record();
        for field in ["mode", "allow-lan", "secret", "log-level", "ipv6"] {
            assert!(record.contains_key(field), "{field}");
        }
        #[cfg(feature = "default-meta")]
        for field in ["unified-delay", "tcp-concurrent"] {
            assert!(record.contains_key(field), "{field}");
        }
    }

    #[test]
    fn omitted_null_and_unknown_keys_keep_their_meaning() {
        let patch = settings(json!({
            "log-level": "debug", "ipv6": null, "mode": null, "secret": null, "future": 1,
        }))
        .unwrap();
        let full = ClashGuardOverridesPatch::from(patch);
        assert_eq!(full.log_level, Some(LogLevel::Debug));
        assert!(full.ipv6.is_none() && full.mode.is_none() && full.secret.is_none());

        let patch = access(json!({"allow-lan": true, "secret": null, "log-level": null})).unwrap();
        let full = ClashGuardOverridesPatch::from(patch);
        assert_eq!(full.allow_lan, Some(true));
        assert!(full.secret.is_none());
    }

    #[test]
    fn the_converted_patches_write_only_their_own_class() {
        let full = ClashGuardOverridesPatch::from(Mode::Global);
        assert_eq!(written_keys(&full), ["mode"]);
        let full = ClashGuardOverridesPatch::from(
            access(json!({"allow-lan": false, "secret": ""})).unwrap(),
        );
        assert_eq!(written_keys(&full), ["allow-lan", "secret"]);
        let full = ClashGuardOverridesPatch::from(
            settings(json!({"log-level": "info", "ipv6": false})).unwrap(),
        );
        assert_eq!(written_keys(&full), ["ipv6", "log-level"]);
    }

    #[test]
    fn the_schemas_describe_only_the_permitted_fields() {
        fn render<T: Type>() -> String {
            let mut types = specta::Types::default();
            let definition = T::definition(&mut types);
            format!("{definition:?}{types:?}")
        }
        let settings_schema = render::<ClashOverridesSettingsPatch>();
        let access_schema = render::<ClashAccessPatch>();
        // The schema names the Rust fields; the kebab-case spelling is applied when it is exported.
        for field in ["mode", "allow_lan", "secret"] {
            assert!(!settings_schema.contains(&format!("{field:?}")), "{field}");
        }
        for field in ["mode", "log_level", "ipv6"] {
            assert!(!access_schema.contains(&format!("{field:?}")), "{field}");
        }
        assert!(
            settings_schema.contains("\"log_level\"") && access_schema.contains("\"allow_lan\"")
        );
    }

    /// Each override is written by exactly one request type, or by the proxy-mode request. The
    /// sample is the serialized default record, so a newly added override takes part.
    #[test]
    fn every_override_has_exactly_one_request_type() {
        for (field, value) in record() {
            let body = json!({ &field: value });
            let by_settings = serde_json::from_value::<ClashOverridesSettingsPatch>(body.clone())
                .map(|patch| written_keys(&patch.into()).contains(&field))
                .unwrap_or(false);
            let by_access = serde_json::from_value::<ClashAccessPatch>(body)
                .map(|patch| written_keys(&patch.into()).contains(&field))
                .unwrap_or(false);
            let by_mode = field == "mode";
            assert_eq!(
                usize::from(by_settings) + usize::from(by_access) + usize::from(by_mode),
                1,
                "{field}: settings={by_settings} access={by_access}"
            );
        }
    }
}
