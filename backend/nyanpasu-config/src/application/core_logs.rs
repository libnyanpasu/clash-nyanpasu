use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CoreLogCompression {
    None,
    #[default]
    Preset,
    Trained,
}

/// Current-session Core log rotation and compression, applied as soon as it is
/// committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct CoreLogSettings {
    pub shard_size_mib: u32,
    pub max_size_mib: u32,
    pub compression: CoreLogCompression,
}

impl Default for CoreLogSettings {
    fn default() -> Self {
        Self {
            shard_size_mib: 16,
            max_size_mib: 64,
            compression: CoreLogCompression::default(),
        }
    }
}

impl CoreLogSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.shard_size_mib < 4 {
            return Err("core log shards must be at least 4 MiB");
        }
        if u64::from(self.max_size_mib) < 2 * u64::from(self.shard_size_mib) {
            return Err("core log disk budget must hold at least two shards");
        }
        Ok(())
    }

    pub fn shard_bytes(self) -> u64 {
        u64::from(self.shard_size_mib) * 1024 * 1024
    }

    pub fn max_bytes(self) -> u64 {
        u64::from(self.max_size_mib) * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_defaults_and_validation() {
        let settings: CoreLogSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, CoreLogSettings::default());
        assert!(settings.validate().is_ok());
        assert!(
            CoreLogSettings {
                shard_size_mib: 0,
                ..settings
            }
            .validate()
            .is_err()
        );
        assert!(
            CoreLogSettings {
                max_size_mib: 16,
                ..settings
            }
            .validate()
            .is_err()
        );
    }
}
