//! The Wi-Fi name of an interface, where the platform discloses it without a
//! consent prompt. Every failure reads as `Unavailable`: a rule on the Wi-Fi
//! name then does not match, and the other conditions keep working.

use nyanpasu_config::application::WifiName;

/// Asks NetworkManager, which answers the active connection's name to an
/// unprivileged session. Hosts without it (iwd or wpa_supplicant alone)
/// report `Unavailable`.
#[cfg(target_os = "linux")]
pub(super) fn name(interface: &str) -> WifiName {
    match network_manager_ssid(interface) {
        Ok(Some(ssid)) => WifiName::Known { ssid },
        Ok(None) => WifiName::Unavailable,
        Err(error) => {
            tracing::debug!(%error, "NetworkManager did not report the Wi-Fi name");
            WifiName::Unavailable
        }
    }
}

#[cfg(target_os = "linux")]
fn network_manager_ssid(interface: &str) -> zbus::Result<Option<String>> {
    use zbus::{
        blocking::{Connection, Proxy},
        zvariant::OwnedObjectPath,
    };

    const SERVICE: &str = "org.freedesktop.NetworkManager";

    let connection = Connection::system()?;
    let manager = Proxy::new(
        &connection,
        SERVICE,
        "/org/freedesktop/NetworkManager",
        SERVICE,
    )?;
    let device: OwnedObjectPath = manager.call("GetDeviceByIpIface", &(interface,))?;
    let wireless = Proxy::new(
        &connection,
        SERVICE,
        device,
        "org.freedesktop.NetworkManager.Device.Wireless",
    )?;
    let access_point: OwnedObjectPath = wireless.get_property("ActiveAccessPoint")?;
    // NetworkManager answers the root path when no access point is active.
    if access_point.as_str() == "/" {
        return Ok(None);
    }
    let access_point = Proxy::new(
        &connection,
        SERVICE,
        access_point,
        "org.freedesktop.NetworkManager.AccessPoint",
    )?;
    let ssid: Vec<u8> = access_point.get_property("Ssid")?;

    // An SSID is arbitrary octets; one that is not UTF-8 is matched by its
    // lossy rendering, which is also what the rule editor shows.
    Ok(Some(String::from_utf8_lossy(&ssid).into_owned()))
}

/// macOS and Windows 11 24H2 gate the Wi-Fi name behind location consent,
/// which this adapter does not request.
#[cfg(not(target_os = "linux"))]
pub(super) fn name(_interface: &str) -> WifiName {
    WifiName::Unavailable
}
