//! A platform with no Wi-Fi name API this application uses.

use std::time::Duration;

use nyanpasu_config::application::{WifiName, WifiUnavailable};

pub(super) const NAME_CACHE_TTL: Duration = Duration::ZERO;

pub(super) struct Platform;

impl Platform {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) fn read(&self, _interface: &str, _ask: bool) -> WifiName {
        WifiName::Unavailable {
            reason: WifiUnavailable::Unsupported,
        }
    }
}
