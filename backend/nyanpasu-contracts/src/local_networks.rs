//! The boundary through which remote access lists the networks this computer is attached to.

use nyanpasu_config::application::LocalNetwork;

/// The IPv4 networks of the host's interfaces that are up. Blocking by nature (it walks the
/// interface table), so the caller runs it on a blocking pool.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait LocalNetworkSource: Send + Sync + 'static {
    /// Reads the networks now, one per interface address, without loopback, link-local or
    /// duplicate networks.
    fn networks(&self) -> Vec<LocalNetwork>;
}
