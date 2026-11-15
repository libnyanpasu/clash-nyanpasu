//! The Wi-Fi name of an interface, and what to report where it cannot be read.
//!
//! Linux asks NetworkManager. macOS and Windows gate the name behind location
//! access: the platform module reports whether that access is missing or
//! refused instead of failing, so the rule editor can say what to do. Every
//! failure reads as `Unavailable`: a rule on the Wi-Fi name then does not
//! match, and the other conditions keep working.

mod cache;
#[cfg(any(windows, test))]
mod guid;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[path = "unsupported.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

use std::{sync::Mutex, time::Instant};

use nyanpasu_config::application::WifiName;
use nyanpasu_contracts::network_environment::WifiNameRead;

use self::cache::{CacheKey, NameCache};

/// Reads Wi-Fi names for the observer, remembering platform state: the last
/// name read and whether the user has been asked for consent.
pub(super) struct WifiNameReader {
    platform: platform::Platform,
    cache: Mutex<NameCache>,
}

impl WifiNameReader {
    pub(super) fn new() -> Self {
        Self {
            platform: platform::Platform::new(),
            cache: Mutex::new(NameCache::default()),
        }
    }

    /// The name of the Wi-Fi network behind `interface`, as the request
    /// allows. `gateway_mac` only tells a new network from the cached one.
    pub(super) fn name(
        &self,
        interface: &str,
        gateway_mac: Option<&str>,
        request: WifiNameRead,
    ) -> WifiName {
        let ask = match request {
            WifiNameRead::Skip => return WifiName::NotRead,
            WifiNameRead::Read => false,
            WifiNameRead::ReadAndAsk => true,
        };
        let key = CacheKey::new(interface, gateway_mac);

        // An explicit request always reaches the platform: the user is waiting
        // for the answer.
        if !ask {
            let cache = self
                .cache
                .lock()
                .expect("no holder of the name cache panics");
            if let Some(ssid) = cache.get(&key, Instant::now(), platform::NAME_CACHE_TTL) {
                return WifiName::Known {
                    ssid: ssid.to_owned(),
                };
            }
        }

        let name = self.platform.read(interface, ask);
        if let WifiName::Known { ssid } = &name {
            self.cache
                .lock()
                .expect("no holder of the name cache panics")
                .put(key, ssid.clone(), Instant::now());
        }
        name
    }
}
