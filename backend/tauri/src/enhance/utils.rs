use serde::{Deserialize, Serialize};

use crate::config::profile::{item_type::ProfileUid, profiles::IProfiles};

use super::ChainItem;
use parking_lot::Mutex;
use std::sync::Arc;

pub fn convert_uids_to_scripts(profiles: &IProfiles, uids: &[ProfileUid]) -> Vec<ChainItem> {
    uids.iter()
        .filter_map(|uid| profiles.get_item(uid).ok())
        .filter_map(<Option<ChainItem>>::from)
        .collect::<Vec<ChainItem>>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub trait LogsExt {
    fn span<T: AsRef<str>>(&mut self, span: LogSpan, msg: T);
    fn log<T: AsRef<str>>(&mut self, msg: T);
    fn info<T: AsRef<str>>(&mut self, msg: T);
    fn warn<T: AsRef<str>>(&mut self, msg: T);
    fn error<T: AsRef<str>>(&mut self, msg: T);
}
impl LogsExt for Logs {
    fn span<T: AsRef<str>>(&mut self, span: LogSpan, msg: T) {
        self.push((span, msg.as_ref().to_string()));
    }
    fn log<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Log, msg);
    }
    fn info<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Info, msg);
    }
    fn warn<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Warn, msg);
    }
    fn error<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Error, msg);
    }
}

pub fn take_logs(logs: Arc<Mutex<Option<Logs>>>) -> Logs {
    logs.lock().take().unwrap()
}
