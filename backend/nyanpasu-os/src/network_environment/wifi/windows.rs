//! Windows: the WLAN API reads the name of the connected network.
//!
//! Since Windows 11 24H2 `WlanQueryInterface` answers `ERROR_ACCESS_DENIED`
//! to an app without location access. The first such call also makes Windows
//! show a one-time consent prompt, so a reading is itself the request and
//! there is no separate way to ask. Every call is listed under the user's
//! recent location activity, which is why readings are cached.

use std::{ffi::c_void, mem, ptr, time::Duration};

use nyanpasu_config::application::{WifiName, WifiUnavailable};
use windows_sys::{
    Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_STATE, ERROR_SUCCESS, HANDLE},
        NetworkManagement::WiFi::{
            WLAN_API_VERSION_2_0, WLAN_CONNECTION_ATTRIBUTES, WlanCloseHandle, WlanFreeMemory,
            WlanOpenHandle, WlanQueryInterface, wlan_interface_state_connected,
            wlan_intf_opcode_current_connection,
        },
    },
    core::GUID,
};

use super::guid;

/// A name just read for the same network is reused, to keep the system's
/// location-in-use indicator from flickering on every poll.
pub(super) const NAME_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Platform;

/// Why no connected name came back.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// Windows refused location access to this app.
    AccessDenied,
    Os(u32),
}

impl Platform {
    pub(super) fn new() -> Self {
        Self
    }

    /// `interface` is the adapter name Windows reports, a braced GUID. The
    /// consent prompt is shown by Windows itself, so `ask` changes nothing.
    pub(super) fn read(&self, interface: &str, _ask: bool) -> WifiName {
        let Some(guid) = guid::parse(interface) else {
            return unavailable(WifiUnavailable::Unreadable);
        };
        let guid = GUID {
            data1: guid.data1,
            data2: guid.data2,
            data3: guid.data3,
            data4: guid.data4,
        };

        match connected_ssid(&guid) {
            Ok(Some(ssid)) => WifiName::Known { ssid },
            Ok(None) => unavailable(WifiUnavailable::Unreadable),
            Err(Failure::AccessDenied) => unavailable(WifiUnavailable::PermissionDenied),
            Err(Failure::Os(code)) => {
                tracing::debug!(code, "the WLAN API did not report the Wi-Fi name");
                unavailable(WifiUnavailable::Unreadable)
            }
        }
    }
}

fn unavailable(reason: WifiUnavailable) -> WifiName {
    WifiName::Unavailable { reason }
}

/// Closes a WLAN client handle when dropped.
struct ClientHandle(HANDLE);

impl Drop for ClientHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `WlanOpenHandle` and is
        // closed once, here.
        unsafe { WlanCloseHandle(self.0, ptr::null()) };
    }
}

/// Frees memory the WLAN API allocated when dropped.
struct WlanMemory(*mut c_void);

impl Drop for WlanMemory {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a successful WLAN API call that
        // allocates for the caller and is freed once, here.
        unsafe { WlanFreeMemory(self.0) };
    }
}

/// The name of the network `interface` is connected to, or `None` when it is
/// not connected.
fn connected_ssid(interface: &GUID) -> Result<Option<String>, Failure> {
    let mut negotiated_version = 0_u32;
    let mut client: HANDLE = ptr::null_mut();
    // SAFETY: both out pointers refer to live locals.
    let status = unsafe {
        WlanOpenHandle(
            WLAN_API_VERSION_2_0,
            ptr::null(),
            &mut negotiated_version,
            &mut client,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(Failure::Os(status));
    }
    let client = ClientHandle(client);

    let mut size = 0_u32;
    let mut data: *mut c_void = ptr::null_mut();
    // SAFETY: `client` is open, `interface` and the out pointers refer to live
    // locals, and the opcode's value is a `WLAN_CONNECTION_ATTRIBUTES`.
    let status = unsafe {
        WlanQueryInterface(
            client.0,
            interface,
            wlan_intf_opcode_current_connection,
            ptr::null(),
            &mut size,
            &mut data,
            ptr::null_mut(),
        )
    };
    match status {
        ERROR_SUCCESS => {}
        ERROR_ACCESS_DENIED => return Err(Failure::AccessDenied),
        // The adapter is not connected to a network.
        ERROR_INVALID_STATE => return Ok(None),
        other => return Err(Failure::Os(other)),
    }
    let memory = WlanMemory(data);
    if memory.0.is_null() || (size as usize) < mem::size_of::<WLAN_CONNECTION_ATTRIBUTES>() {
        return Ok(None);
    }

    // SAFETY: on success `data` points to a `WLAN_CONNECTION_ATTRIBUTES` of
    // at least `size` bytes that stays valid until `memory` is dropped, and
    // the size check above covers the whole struct. It may be unaligned for
    // the struct's fields, so it is copied out by `read_unaligned`.
    let attributes = unsafe {
        memory
            .0
            .cast::<WLAN_CONNECTION_ATTRIBUTES>()
            .read_unaligned()
    };
    if attributes.isState != wlan_interface_state_connected {
        return Ok(None);
    }

    let ssid = &attributes.wlanAssociationAttributes.dot11Ssid;
    let length = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
    // An SSID is arbitrary octets; one that is not UTF-8 is matched by its
    // lossy rendering, which is also what the rule editor shows.
    let ssid = String::from_utf8_lossy(&ssid.ucSSID[..length]).into_owned();
    Ok((!ssid.is_empty()).then_some(ssid))
}
