use anyhow::Context;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
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

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_struct_attr(serde_with::skip_serializing_none)]
#[serde(rename_all = "kebab-case")]
pub struct ClashGuardOverrides {
    log_level: LogLevel,
    allow_lan: bool,
    mode: Mode,
    secret: String,
    #[cfg(feature = "default-meta")]
    unified_delay: bool,
    #[cfg(feature = "default-meta")]
    tcp_concurrent: bool,
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
            unified_delay: true,
            #[cfg(feature = "default-meta")]
            tcp_concurrent: true,
            ipv6: false,
        }
    }
}

impl ClashGuardOverrides {
    /// Apply overrides to the config
    /// # Arguments
    ///
    /// * `config` - The config to apply overrides to
    ///
    /// # Returns
    ///
    /// The config with overrides applied
    ///
    pub fn apply_overrides(&self, mut config: Mapping) -> anyhow::Result<Mapping> {
        use crate::utils::yaml::apply_overrides;
        let overrides =
            serde_yaml::to_value(self).context("failed to convert overrides to value")?;
        let overrides = overrides
            .as_mapping()
            .context("failed to convert overrides to mapping")?;
        apply_overrides(&mut config, overrides);
        Ok(config)
    }
}

