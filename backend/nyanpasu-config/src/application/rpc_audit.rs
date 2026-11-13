//! Pure RPC audit retention settings and validation.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Retention limits for persisted HTTP and local RPC audit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct RpcAuditSettings {
    /// Maximum age in days, from 1 through 90; defaults to 7.
    pub max_age_days: u16,
    /// Maximum storage size in MiB, from 4 through 256; defaults to 16.
    pub max_size_mib: u16,
}

impl Default for RpcAuditSettings {
    fn default() -> Self {
        Self {
            max_age_days: 7,
            max_size_mib: 16,
        }
    }
}

impl RpcAuditSettings {
    /// Checks both retention limits before startup or a configuration commit.
    ///
    /// # Errors
    /// Returns the reason when either limit is outside its supported range.
    pub fn validate(self) -> Result<(), &'static str> {
        if !(1..=90).contains(&self.max_age_days) {
            return Err("RPC audit retention must be between 1 and 90 days");
        }
        if !(4..=256).contains(&self.max_size_mib) {
            return Err("RPC audit disk budget must be between 4 and 256 MiB");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use struct_patch::Patch as _;

    use crate::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};

    use super::*;

    #[test]
    fn rpc_audit_defaults_for_missing_fields() {
        let defaults = RpcAuditSettings::default();
        assert_eq!(defaults.max_age_days, 7);
        assert_eq!(defaults.max_size_mib, 16);
        assert_eq!(defaults.validate(), Ok(()));
        assert_eq!(
            serde_json::from_str::<RpcAuditSettings>("{}").unwrap(),
            defaults
        );
        let age_only: RpcAuditSettings = serde_json::from_str(r#"{"max_age_days":30}"#).unwrap();
        assert_eq!(
            age_only,
            RpcAuditSettings {
                max_age_days: 30,
                ..defaults
            }
        );
        let size_only: RpcAuditSettings = serde_json::from_str(r#"{"max_size_mib":64}"#).unwrap();
        assert_eq!(
            size_only,
            RpcAuditSettings {
                max_size_mib: 64,
                ..defaults
            }
        );

        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("rpc_audit");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.rpc_audit, defaults);
    }

    #[test]
    fn rpc_audit_validates_inclusive_bounds() {
        for max_age_days in [1, 90] {
            for max_size_mib in [4, 256] {
                assert_eq!(
                    RpcAuditSettings {
                        max_age_days,
                        max_size_mib
                    }
                    .validate(),
                    Ok(())
                );
            }
        }
        for max_age_days in [0, 91, u16::MAX] {
            assert_eq!(
                RpcAuditSettings {
                    max_age_days,
                    ..RpcAuditSettings::default()
                }
                .validate(),
                Err("RPC audit retention must be between 1 and 90 days")
            );
        }
        for max_size_mib in [0, 3, 257, u16::MAX] {
            assert_eq!(
                RpcAuditSettings {
                    max_size_mib,
                    ..RpcAuditSettings::default()
                }
                .validate(),
                Err("RPC audit disk budget must be between 4 and 256 MiB")
            );
        }
    }

    #[test]
    fn rpc_audit_patch_keeps_or_replaces_the_whole_settings() {
        let mut config = NyanpasuAppConfig::default();
        config.rpc_audit.max_size_mib = 64;
        config.apply(NyanpasuAppConfig::new_empty_patch());
        assert_eq!(config.rpc_audit.max_size_mib, 64);

        let patch: NyanpasuAppConfigPatch =
            serde_json::from_str(r#"{"rpc_audit":{"max_age_days":30}}"#).unwrap();
        config.apply(patch);
        assert_eq!(
            config.rpc_audit,
            RpcAuditSettings {
                max_age_days: 30,
                ..RpcAuditSettings::default()
            }
        );
    }
}
