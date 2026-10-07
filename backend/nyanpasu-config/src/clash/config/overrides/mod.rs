mod manageable;

pub use manageable::ManageableField;

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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, specta::Type, Patch)]
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

    /// The top-level runtime config keys these overrides force, in field
    /// order. An unmanaged field forces nothing, so whatever the profiles and
    /// transforms produced for it stays.
    pub fn forced_entries(&self) -> Vec<(&'static str, serde_json::Value)> {
        let mut entries = vec![
            ("log-level", serde_json::json!(self.log_level)),
            ("allow-lan", serde_json::json!(self.allow_lan)),
            ("mode", serde_json::json!(self.mode)),
            ("secret", serde_json::json!(self.secret)),
        ];
        #[cfg(feature = "default-meta")]
        {
            if let Some(value) = self.unified_delay.managed() {
                entries.push(("unified-delay", serde_json::json!(value)));
            }
            if let Some(value) = self.tcp_concurrent.managed() {
                entries.push(("tcp-concurrent", serde_json::json!(value)));
            }
        }
        entries.push(("ipv6", serde_json::json!(self.ipv6)));
        entries
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
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

    /// `forced_entries` lists every field when all are managed, so a field
    /// added to the struct cannot silently stop reaching the runtime config.
    #[test]
    fn forced_entries_cover_every_managed_field() {
        let overrides = ClashGuardOverrides::default();
        let serialized = serde_json::to_value(&overrides).unwrap();
        let keys: std::collections::BTreeSet<&str> = serialized
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let forced: std::collections::BTreeSet<&str> = overrides
            .forced_entries()
            .into_iter()
            .map(|(key, _)| key)
            .collect();

        assert_eq!(forced, keys);
    }

    #[cfg(feature = "default-meta")]
    #[test]
    fn unmanaged_fields_are_not_forced() {
        let mut overrides = ClashGuardOverrides::default();
        overrides.tcp_concurrent = ManageableField::Unmanaged;

        let forced = overrides.forced_entries();

        assert!(!forced.iter().any(|(key, _)| *key == "tcp-concurrent"));
        assert!(forced.contains(&("unified-delay", serde_json::json!(true))));
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
