use super::IVerge;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use tracing_subscriber::filter;

#[derive(Deserialize, Serialize, Debug, Clone, specta::Type, EnumString, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LoggingLevel {
    #[serde(rename = "silent", alias = "off")]
    Silent,
    #[serde(rename = "trace", alias = "tracing")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn", alias = "warning")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

impl Default for LoggingLevel {
    #[cfg(debug_assertions)]
    fn default() -> Self {
        Self::Trace
    }

    #[cfg(not(debug_assertions))]
    fn default() -> Self {
        Self::Info
    }
}

impl From<LoggingLevel> for filter::LevelFilter {
    fn from(level: LoggingLevel) -> Self {
        match level {
            LoggingLevel::Silent => filter::LevelFilter::OFF,
            LoggingLevel::Trace => filter::LevelFilter::TRACE,
            LoggingLevel::Debug => filter::LevelFilter::DEBUG,
            LoggingLevel::Info => filter::LevelFilter::INFO,
            LoggingLevel::Warn => filter::LevelFilter::WARN,
            LoggingLevel::Error => filter::LevelFilter::ERROR,
        }
    }
}

impl IVerge {
    pub fn get_log_level(&self) -> LoggingLevel {
        self.app_log_level.clone().unwrap_or_default()
    }
}
