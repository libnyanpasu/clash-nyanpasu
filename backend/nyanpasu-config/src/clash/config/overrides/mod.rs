mod manageable;
mod requests;
mod tun;

pub use requests::*;

pub use manageable::ManageableField;
pub use tun::{TunOverrides, TunOverridesPatch};

use serde::{Deserialize, Serialize};
use struct_patch::Patch;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Silent,
    Error,
    Warning,
    #[default]
    Info,
    Debug,
}

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Rule,
    Global,
    Direct,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    specta::Type,
    Patch,
    nyanpasu_macro::OwnedFields,
)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
#[patch(attribute(serde(default, rename_all = "kebab-case")))]
#[serde(rename_all = "kebab-case")]
pub struct ClashGuardOverrides {
    log_level: LogLevel,
    allow_lan: ManageableField<bool>,
    mode: Mode,
    secret: String,
    unified_delay: ManageableField<bool>,
    tcp_concurrent: ManageableField<bool>,
    ipv6: ManageableField<bool>,
}

impl Default for ClashGuardOverrides {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            allow_lan: ManageableField::Unmanaged,
            mode: Mode::Rule,
            secret: uuid::Uuid::new_v4().to_string().to_lowercase(),
            unified_delay: ManageableField::Unmanaged,
            tcp_concurrent: ManageableField::Unmanaged,
            ipv6: ManageableField::Unmanaged,
        }
    }
}

impl ClashGuardOverrides {
    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn log_level(&self) -> LogLevel {
        self.log_level
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }
}

/// The overrides a `read` caller may see: everything except the controller secret.
///
/// The conversion destructures the overrides without `..`, so a new field must be classified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub struct ClashOverridesSettings {
    pub log_level: LogLevel,
    pub allow_lan: ManageableField<bool>,
    pub mode: Mode,
    pub unified_delay: ManageableField<bool>,
    pub tcp_concurrent: ManageableField<bool>,
    pub ipv6: ManageableField<bool>,
}

impl From<&ClashGuardOverrides> for ClashOverridesSettings {
    fn from(overrides: &ClashGuardOverrides) -> Self {
        let ClashGuardOverrides {
            log_level,
            allow_lan,
            mode,
            // The controller secret is only in the full config.
            secret: _,
            unified_delay,
            tcp_concurrent,
            ipv6,
        } = overrides;
        Self {
            log_level: *log_level,
            allow_lan: *allow_lan,
            mode: *mode,
            unified_delay: *unified_delay,
            tcp_concurrent: *tcp_concurrent,
            ipv6: *ipv6,
        }
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use crate::runtime::post_process::OwnedFields;
    use struct_patch::Patch;

    /// The patch shares the config's `kebab-case` wire shape: it decodes
    /// `allow-lan`/`log-level` and applies only the fields present.
    #[test]
    fn patch_uses_kebab_case_and_applies() {
        let patch: ClashGuardOverridesPatch =
            yaml_serde::from_str("allow-lan: {kind: managed, value: true}\nlog-level: debug\n")
                .expect("kebab-case patch must deserialize");

        let mut overrides = ClashGuardOverrides::default();
        let kept_secret = overrides.secret.clone();
        overrides.apply(patch);

        assert_eq!(overrides.allow_lan, ManageableField::Managed(true));
        assert_eq!(overrides.log_level, LogLevel::Debug);
        assert_eq!(overrides.mode, Mode::Rule, "absent field unchanged");
        assert_eq!(overrides.secret, kept_secret, "absent secret unchanged");
    }

    /// `ownership_entries` lists every field, so a field added to the struct
    /// cannot silently stop reaching the runtime config.
    #[test]
    fn ownership_entries_cover_every_field() {
        let overrides = ClashGuardOverrides::default();
        let serialized = serde_json::to_value(&overrides).unwrap();
        let keys: std::collections::BTreeSet<&str> = serialized
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let owned: std::collections::BTreeSet<&str> = overrides
            .ownership_entries()
            .into_iter()
            .map(|(key, _)| key)
            .collect();

        assert_eq!(owned, keys);
    }

    #[test]
    fn unmanaged_fields_are_left_to_the_profiles() {
        use crate::runtime::{post_process::Ownership, value::ConfigValue};

        let overrides = ClashGuardOverrides {
            unified_delay: ManageableField::Managed(true),
            ..Default::default()
        };

        let owned = overrides.ownership_entries();

        assert!(owned.contains(&("tcp-concurrent", Ownership::Inherited)));
        assert!(owned.contains(&("unified-delay", Ownership::Forced(ConfigValue::Bool(true)))));
    }

    /// A fresh install forces none of the profile-facing fields: a
    /// subscription's `allow-lan`, `ipv6`, `unified-delay` and
    /// `tcp-concurrent` reach the core as written.
    #[test]
    fn a_fresh_install_leaves_profile_facing_fields_to_the_profiles() {
        use crate::runtime::post_process::Ownership;

        let owned = ClashGuardOverrides::default().ownership_entries();

        for key in ["allow-lan", "ipv6", "unified-delay", "tcp-concurrent"] {
            assert!(owned.contains(&(key, Ownership::Inherited)), "{key}");
        }
    }

    /// A serialized patch keeps the `kebab-case` keys and skips absent fields.
    #[test]
    fn patch_serializes_kebab_case_and_skips_none() {
        let mut patch = ClashGuardOverrides::new_empty_patch();
        patch.log_level = Some(LogLevel::Debug);

        let dumped = yaml_serde::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("log-level: debug"), "got:\n{dumped}");
        assert!(
            !dumped.contains("allow-lan"),
            "absent skipped, got:\n{dumped}"
        );
    }
}
