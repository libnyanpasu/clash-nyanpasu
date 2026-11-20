//! Linux: NetworkManager answers the active connection's name to an
//! unprivileged session, with no consent step.

use std::time::Duration;

use nyanpasu_config::application::{WifiName, WifiUnavailable};

/// NetworkManager is a local D-Bus call, so every reading asks it.
pub(super) const NAME_CACHE_TTL: Duration = Duration::ZERO;

pub(super) struct Platform;

impl Platform {
    pub(super) fn new() -> Self {
        Self
    }

    /// Hosts without NetworkManager (iwd or wpa_supplicant alone) report
    /// `Unreadable`.
    pub(super) fn read(&self, interface: &str, _ask: bool) -> WifiName {
        match network_manager_ssid(interface) {
            Ok(Some(ssid)) => WifiName::Known { ssid },
            Ok(None) => WifiName::Unavailable {
                reason: WifiUnavailable::Unreadable,
            },
            Err(error) => {
                tracing::debug!(%error, "NetworkManager did not report the Wi-Fi name");
                WifiName::Unavailable {
                    reason: WifiUnavailable::Unreadable,
                }
            }
        }
    }
}

/// The observer reads on a blocking thread, so the async API is driven to
/// completion here.
fn network_manager_ssid(interface: &str) -> zbus::Result<Option<String>> {
    zbus::block_on(network_manager_ssid_async(interface))
}

async fn network_manager_ssid_async(interface: &str) -> zbus::Result<Option<String>> {
    use zbus::{Connection, OwnedObjectPath, Proxy};

    const SERVICE: &str = "org.freedesktop.NetworkManager";

    let connection = Connection::system().await?;
    let manager = Proxy::new(
        &connection,
        SERVICE,
        "/org/freedesktop/NetworkManager",
        SERVICE,
    )
    .await?;
    let device: OwnedObjectPath = manager.call("GetDeviceByIpIface", &(interface,)).await?;
    let wireless = Proxy::new(
        &connection,
        SERVICE,
        device,
        "org.freedesktop.NetworkManager.Device.Wireless",
    )
    .await?;
    let access_point: OwnedObjectPath = wireless.get_property("ActiveAccessPoint").await?;
    // NetworkManager answers the root path when no access point is active.
    if access_point.as_str() == "/" {
        return Ok(None);
    }
    let access_point = Proxy::new(
        &connection,
        SERVICE,
        access_point,
        "org.freedesktop.NetworkManager.AccessPoint",
    )
    .await?;
    let ssid: Vec<u8> = access_point.get_property("Ssid").await?;

    // An SSID is arbitrary octets; one that is not UTF-8 is matched by its
    // lossy rendering, which is also what the rule editor shows.
    Ok(Some(String::from_utf8_lossy(&ssid).into_owned()))
}
