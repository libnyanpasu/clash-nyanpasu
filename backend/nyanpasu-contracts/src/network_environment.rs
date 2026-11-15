//! The boundary through which network automation reads the host's network.

use nyanpasu_config::application::NetworkState;

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
    fn current(&self) -> NetworkState;
}
