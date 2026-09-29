//! The infrastructure boundaries the system-proxy actor depends on.
//!
//! Each trait is narrow and task-oriented so the actor can be exercised with
//! plain fakes: nothing here mentions `sysproxy`, `auto_launch`, `reqwest` or
//! Tauri. The concrete implementations live in [`super::adapters`].

use std::path::PathBuf;

use serde::Serialize;
use snafu::Snafu;
use tokio_util::sync::CancellationToken;

/// What the OS proxy settings look like, in the only shape this app writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsProxyConfig {
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,
}

/// Why the platform proxy settings could not be read or written. The platform's
/// own error stays in `source`, boxed because this module does not name the
/// crate that produced it; it reaches the user through the copied detail.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OsProxyError {
    #[snafu(display("could not read the system proxy"))]
    ReadOsProxy {
        #[serde(skip)]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not write the system proxy {host}:{port} (enabled: {enable})"))]
    WriteOsProxy {
        enable: bool,
        host: String,
        port: u16,
        #[serde(skip)]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[cfg(test)]
impl OsProxyError {
    pub(crate) fn unreadable(reason: &str) -> Self {
        Self::ReadOsProxy {
            source: reason.into(),
        }
    }

    pub(crate) fn refused(config: &OsProxyConfig, reason: &str) -> Self {
        Self::WriteOsProxy {
            enable: config.enable,
            host: config.host.clone(),
            port: config.port,
            source: reason.into(),
        }
    }
}

/// The platform proxy settings. Blocking by nature, so the actor calls these
/// from a blocking pool rather than from its mailbox turn.
#[cfg_attr(test, mockall::automock)]
pub trait OsProxyPort: Send + Sync + 'static {
    fn get(&self) -> Result<OsProxyConfig, OsProxyError>;
    fn set(&self, config: &OsProxyConfig) -> Result<(), OsProxyError>;
    /// Platform bypass list used when the user left the field empty.
    fn default_bypass(&self) -> &'static str;
}

/// Why the autostart entry could not be addressed, read or written. Library
/// causes are boxed for the same reason as in [`OsProxyError`].
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum AutoLaunchError {
    #[snafu(display("could not locate the running executable"))]
    LocateExecutable { source: std::io::Error },
    #[snafu(display("could not canonicalize the executable path {}", path.display()))]
    CanonicalizeExecutable {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("the executable {} has no file stem", path.display()))]
    ExecutableName { path: PathBuf },
    #[snafu(display("the executable path {} is not UTF-8", path.display()))]
    ExecutablePathNotUtf8 { path: PathBuf },
    #[snafu(display("could not build the auto-launch registration for {app_path}"))]
    BuildRegistration {
        app_path: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not read the auto-launch registration"))]
    ReadRegistration {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not register the app for auto-launch"))]
    Register {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[cfg(test)]
impl AutoLaunchError {
    pub(crate) fn refused(reason: &str) -> Self {
        Self::Register {
            source: reason.into(),
        }
    }
}

/// Login-item / autostart registration.
#[cfg_attr(test, mockall::automock)]
pub trait AutoLaunchPort: Send + Sync + 'static {
    fn is_enabled(&self) -> Result<bool, AutoLaunchError>;
    fn set_enabled(&self, enabled: bool) -> Result<(), AutoLaunchError>;
}

/// Why a PAC script could not be fetched, checked or installed. `url` is the
/// script the user configured.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PacError {
    #[snafu(display("could not build the PAC http client"))]
    BuildHttpClient {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not request the PAC script at {url}"))]
    RequestScript {
        url: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("the PAC script at {url} answered with HTTP {status}"))]
    ScriptStatus { url: String, status: u16 },
    #[snafu(display("the PAC script at {url} is over the {limit} bytes allowed"))]
    ScriptTooLarge { url: String, limit: usize },
    #[snafu(display("could not read the PAC script body from {url}"))]
    ReadScriptBody {
        url: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("the PAC script at {url} is not valid UTF-8"))]
    ScriptNotUtf8 {
        url: String,
        source: std::string::FromUtf8Error,
    },
    #[snafu(display("the PAC script at {url} has no FindProxyForURL function"))]
    MissingEntryPoint { url: String },
    #[snafu(display("could not install the PAC url {url}"))]
    InstallPac {
        url: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not clear the PAC url"))]
    ClearPac {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The shutdown token fired while the script was being fetched.
    #[snafu(display("the PAC download was cancelled by the shutdown"))]
    DownloadCancelled,
}

#[cfg(test)]
impl PacError {
    /// A script that could not be fetched.
    pub(crate) fn unreachable(reason: &str) -> Self {
        Self::RequestScript {
            url: "http://pac.test/proxy.pac".into(),
            source: reason.into(),
        }
    }

    /// An installed url the OS would not give up.
    pub(crate) fn kept(reason: &str) -> Self {
        Self::ClearPac {
            source: reason.into(),
        }
    }
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
    async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> Result<(), PacError>;
    fn disable(&self) -> Result<(), PacError>;
}
