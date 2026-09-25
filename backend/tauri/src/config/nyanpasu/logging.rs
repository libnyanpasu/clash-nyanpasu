use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, specta::Type)]
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
