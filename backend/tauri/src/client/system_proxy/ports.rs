//! The infrastructure boundaries the system-proxy actor depends on.
//!
//! Each trait is narrow and task-oriented so the actor can be exercised with
//! plain fakes: nothing here mentions `sysproxy`, `auto_launch`, `reqwest` or
//! Tauri. The concrete implementations live in [`super::adapters`].

use tokio_util::sync::CancellationToken;

/// What the OS proxy settings look like, in the only shape this app writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsProxyConfig {
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,
}

/// The platform proxy settings. Blocking by nature, so the actor calls these
/// from a blocking pool rather than from its mailbox turn.
#[cfg_attr(test, mockall::automock)]
pub trait OsProxyPort: Send + Sync + 'static {
    fn get(&self) -> anyhow::Result<OsProxyConfig>;
    fn set(&self, config: &OsProxyConfig) -> anyhow::Result<()>;
    /// Platform bypass list used when the user left the field empty.
    fn default_bypass(&self) -> &'static str;
}

/// Login-item / autostart registration.
#[cfg_attr(test, mockall::automock)]
pub trait AutoLaunchPort: Send + Sync + 'static {
    fn is_enabled(&self) -> anyhow::Result<bool>;
    fn set_enabled(&self, enabled: bool) -> anyhow::Result<()>;
}

/// Proxy auto-configuration. `apply` downloads, validates, caches and installs
/// the script URL; the caller decides what to do when it fails.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait PacPort: Send + Sync + 'static {
    fn is_supported(&self) -> bool;
    /// Runs on the actor's mailbox turn and reaches the network, so it takes
    /// the shutdown token: an implementation must abandon whatever it is
    /// waiting on as soon as the token fires, or the exit path queues behind
    /// a download it cannot outlast.
    async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> anyhow::Result<()>;
    fn disable(&self) -> anyhow::Result<()>;
}
