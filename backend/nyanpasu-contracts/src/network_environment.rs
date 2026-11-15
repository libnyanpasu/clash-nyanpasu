//! The boundary through which network automation reads the host's network.

use nyanpasu_config::application::NetworkState;

/// Whether a reading touches the platform's Wi-Fi name API.
///
/// macOS and Windows gate the name behind location access: reading it is what
/// makes the system ask the user, and what shows the location indicator. So a
/// caller reads it only when it needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiNameRead {
    /// Leaves the Wi-Fi name API alone; a Wi-Fi network reads as not read.
    Skip,
    /// Reads the name where the platform answers without asking the user, and
    /// reports why where it does not.
    Read,
    /// Like `Read`, and on a platform whose consent has not been decided,
    /// starts the system's request for it. The user's answer arrives later,
    /// so a following reading can differ.
    ReadAndAsk,
}

/// The host's primary network. Blocking by nature (it walks the interface,
/// route and neighbour tables), so the caller runs it on a blocking pool.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait NetworkEnvironmentPort: Send + Sync + 'static {
    /// Reads the primary network now.
    ///
    /// The primary network is the physical interface with a default gateway;
    /// an implementation never reports a tunnel device, so enabling TUN does
    /// not change the answer. A machine without such an interface is
    /// [`NetworkState::Offline`].
    fn current(&self, wifi_name: WifiNameRead) -> NetworkState;
}
