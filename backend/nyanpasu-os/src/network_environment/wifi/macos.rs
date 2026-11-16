//! macOS: CoreWLAN reads the name, but only for an app the user has allowed
//! to use location. Without that access `ssid()` answers nothing, so this
//! module checks the Core Location authorization first and reports what is
//! missing instead of a bare failure.
//!
//! The authorization prompt needs a bundled app: an unbundled binary (such as
//! a development build) is never offered the prompt and stays undecided.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use dispatch2::DispatchQueue;
use nyanpasu_config::application::{WifiName, WifiUnavailable};
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
use objc2_core_wlan::CWWiFiClient;
use objc2_foundation::NSString;

/// Reading the name counts as using location, which macOS shows in the menu
/// bar, so a name just read for the same network is reused.
pub(super) const NAME_CACHE_TTL: Duration = Duration::from_secs(30);

pub(super) struct Platform {
    /// Whether this process has already asked. The prompt stays on screen
    /// until the user answers it, so asking again only stacks requests.
    asked: AtomicBool,
}

/// What the user has decided about this app's location access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Granted,
    Undecided,
    /// Denied, restricted by the device's policy, or Location Services off.
    Refused,
}

impl Platform {
    pub(super) fn new() -> Self {
        Self {
            asked: AtomicBool::new(false),
        }
    }

    pub(super) fn read(&self, interface: &str, ask: bool) -> WifiName {
        match access() {
            Access::Granted => ssid(interface),
            Access::Undecided => {
                if ask {
                    self.ask();
                }
                unavailable(WifiUnavailable::PermissionRequired)
            }
            Access::Refused => unavailable(WifiUnavailable::PermissionDenied),
        }
    }

    /// Shows the system prompt. Core Location delivers it through a manager
    /// created on the main thread.
    fn ask(&self) {
        if self.asked.swap(true, Ordering::AcqRel) {
            return;
        }

        DispatchQueue::main().exec_async(|| {
            // SAFETY: a block on the main queue runs on the main thread, which
            // is where the manager must be created and asked; `new` and
            // `requestWhenInUseAuthorization` have no other preconditions.
            let manager = unsafe { CLLocationManager::new() };
            unsafe { manager.requestWhenInUseAuthorization() };
            // The prompt belongs to the manager and goes away with it. One
            // manager per process is kept, because `asked` allows one request.
            std::mem::forget(manager);
        });
    }
}

fn unavailable(reason: WifiUnavailable) -> WifiName {
    WifiName::Unavailable { reason }
}

fn access() -> Access {
    // SAFETY: reading the authorization status of a fresh manager has no
    // preconditions and needs no run loop, because no delegate is involved.
    let status = unsafe { CLLocationManager::new().authorizationStatus() };
    classify(status)
}

fn classify(status: CLAuthorizationStatus) -> Access {
    match status {
        CLAuthorizationStatus::AuthorizedAlways | CLAuthorizationStatus::AuthorizedWhenInUse => {
            Access::Granted
        }
        CLAuthorizationStatus::NotDetermined => Access::Undecided,
        // An unknown future status is treated as refused: no name is read.
        _ => Access::Refused,
    }
}

fn ssid(interface: &str) -> WifiName {
    let name = NSString::from_str(interface);
    // SAFETY: CoreWLAN's shared client and its interfaces may be used from any
    // thread, and `name` outlives the call.
    let ssid = unsafe {
        CWWiFiClient::sharedWiFiClient()
            .interfaceWithName(Some(&name))
            .and_then(|interface| interface.ssid())
    }
    .map(|ssid| ssid.to_string())
    .filter(|ssid| !ssid.is_empty());

    match ssid {
        Some(ssid) => WifiName::Known { ssid },
        // Authorized, yet no name: the interface is not associated.
        None => unavailable(WifiUnavailable::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_authorized_status_grants_access() {
        assert_eq!(
            classify(CLAuthorizationStatus::AuthorizedAlways),
            Access::Granted
        );
        assert_eq!(
            classify(CLAuthorizationStatus::AuthorizedWhenInUse),
            Access::Granted
        );
        assert_eq!(
            classify(CLAuthorizationStatus::NotDetermined),
            Access::Undecided
        );
        assert_eq!(classify(CLAuthorizationStatus::Denied), Access::Refused);
        assert_eq!(classify(CLAuthorizationStatus::Restricted), Access::Refused);
        assert_eq!(classify(CLAuthorizationStatus(99)), Access::Refused);
    }
}
