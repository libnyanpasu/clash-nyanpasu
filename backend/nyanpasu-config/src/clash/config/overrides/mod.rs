mod manageable;
mod tun;

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
    Script,
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
    allow_lan: bool,
    mode: Mode,
    secret: String,
    #[cfg(feature = "default-meta")]
    unified_delay: ManageableField<bool>,
    #[cfg(feature = "default-meta")]
    tcp_concurrent: ManageableField<bool>,
    ipv6: bool,
}

impl Default for ClashGuardOverrides {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            allow_lan: false,
            mode: Mode::Rule,
            secret: uuid::Uuid::new_v4().to_string().to_lowercase(),
            #[cfg(feature = "default-meta")]
            unified_delay: ManageableField::Managed(true),
            #[cfg(feature = "default-meta")]
            tcp_concurrent: ManageableField::Managed(true),
            ipv6: false,
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
            serde_yaml_ng::from_str("allow-lan: true\nlog-level: debug\n")
                .expect("kebab-case patch must deserialize");

        let mut overrides = ClashGuardOverrides::default();
        let kept_secret = overrides.secret.clone();
        overrides.apply(patch);

        assert!(overrides.allow_lan);
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

    #[cfg(feature = "default-meta")]
    #[test]
    fn unmanaged_fields_are_left_to_the_profiles() {
        use crate::runtime::{post_process::Ownership, value::ConfigValue};

        let mut overrides = ClashGuardOverrides::default();
        overrides.tcp_concurrent = ManageableField::Unmanaged;

        let owned = overrides.ownership_entries();

        assert!(owned.contains(&("tcp-concurrent", Ownership::Inherited)));
        assert!(owned.contains(&("unified-delay", Ownership::Forced(ConfigValue::Bool(true)))));
    }

    /// A serialized patch keeps the `kebab-case` keys and skips absent fields.
    #[test]
    fn patch_serializes_kebab_case_and_skips_none() {
        let mut patch = ClashGuardOverrides::new_empty_patch();
        patch.allow_lan = Some(true);

        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("allow-lan: true"), "got:\n{dumped}");
        assert!(
            !dumped.contains("log-level"),
            "absent skipped, got:\n{dumped}"
        );
    }
}
