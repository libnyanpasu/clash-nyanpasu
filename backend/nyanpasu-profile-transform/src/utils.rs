use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LogSpan {
    Log,
    Info,
    Warn,
    Error,
}

impl AsRef<str> for LogSpan {
    fn as_ref(&self) -> &str {
        match self {
            LogSpan::Log => "log",
            LogSpan::Info => "info",
            LogSpan::Warn => "warn",
            LogSpan::Error => "error",
        }
    }
}

pub type Logs = Vec<(LogSpan, String)>;
