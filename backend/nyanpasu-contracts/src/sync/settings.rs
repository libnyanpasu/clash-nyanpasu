//! The device-scoped sync settings document.
//!
//! These settings are stored in `<config>/local/sync.yaml` and are never part
//! of the synced vault: they name the endpoint this device writes to. The
//! credentials live apart, in the [`SecretStore`](super::SecretStore).
//!
//! They live here rather than in `nyanpasu-config` because they embed
//! [`ProviderConfig`], which this crate owns; the reverse dependency would be a
//! cycle.

use nyanpasu_config::sync::ConflictPolicy;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::ProviderConfig;

/// How sync reaches the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncNetwork {
    Direct,
    #[default]
    System,
    /// Through the running core's proxy.
    ThroughCore,
}

/// What a device without a baseline does on its first cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JoinStrategy {
    PreferRemote,
    PreferLocal,
    #[default]
    Review,
}

/// The sync settings of one device, stored without secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncSettings {
    pub enabled: bool,
    pub provider: Option<ProviderConfig>,
    pub device_name: String,
    pub conflict_policy: ConflictPolicy,
    /// `0` means manual only, as for remote profile refresh.
    pub interval_minutes: u64,
    pub sync_on_change: bool,
    pub network: SyncNetwork,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            device_name: String::new(),
            conflict_policy: ConflictPolicy::Manual,
            interval_minutes: 0,
            sync_on_change: true,
            network: SyncNetwork::default(),
        }
    }
}

impl SyncSettings {
    /// Whether a provider is configured; sync cannot run without one.
    #[must_use]
    pub fn has_provider(&self) -> bool {
        self.provider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_disabled_and_manual() {
        let settings = SyncSettings::default();
        assert!(!settings.enabled);
        assert!(!settings.has_provider());
        assert_eq!(settings.conflict_policy, ConflictPolicy::Manual);
        assert_eq!(settings.interval_minutes, 0);
    }

    #[test]
    fn settings_round_trip_with_a_provider() {
        let settings = SyncSettings {
            enabled: true,
            provider: Some(ProviderConfig::Fs {
                root: "/mnt/sync".to_owned(),
            }),
            device_name: "laptop".to_owned(),
            ..Default::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            serde_json::from_value::<SyncSettings>(json).unwrap(),
            settings
        );
        assert!(
            !String::from_utf8(serde_json::to_vec(&settings).unwrap())
                .unwrap()
                .contains("secret")
        );
    }
}
