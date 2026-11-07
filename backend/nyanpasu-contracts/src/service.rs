//! Service process control and failures shared with the application owner.

use nyanpasu_paths::InstallDirError;
use serde::Serialize;
use snafu::Snafu;

/// The elevated operations `nyanpasu-service` is asked to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCommand {
    Install,
    Update,
    Uninstall,
    Start,
    Stop,
    Restart,
}

impl std::fmt::Display for ServiceCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        })
    }
}

#[cfg(windows)]
type UserError = std::io::Error;
#[cfg(not(windows))]
type UserError = whoami::Error;

/// A failure of running or querying the system service, including the bounds
/// the service actor puts on every call.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum ServiceCommandError {
    #[snafu(display("could not authorize the service {command} command: {source}"))]
    AuthorizeServiceCommand {
        command: ServiceCommand,
        source: crate::elevation::ElevationConsentError,
    },
    #[snafu(display("Windows is ending the session; service commands are unavailable"))]
    SessionEnding,
    #[snafu(display("could not resolve the current user: {source}"))]
    ResolveServiceUser {
        #[serde(skip)]
        source: UserError,
    },
    #[snafu(display("could not resolve the application directories: {source}"))]
    ResolveServiceDirs {
        #[snafu(source(from(InstallDirError, Box::new)))]
        #[serde(skip)]
        source: Box<InstallDirError>,
    },
    #[snafu(display("could not run the service {command} command: {source}"))]
    RunElevated {
        command: ServiceCommand,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the service {command} command failed, exit code: {exit_code:?}, signal: {signal:?}"
    ))]
    ServiceCommandExit {
        command: ServiceCommand,
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[snafu(display("could not query the service status: {source}"))]
    RunServiceStatus {
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the service status query failed, exit code: {exit_code:?}, signal: {signal:?}"
    ))]
    ServiceStatusExit {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[snafu(display("the service status is not valid UTF-8"))]
    ServiceStatusNotUtf8 {
        #[serde(skip)]
        source: std::string::FromUtf8Error,
    },
    #[snafu(display("could not parse the service status: {source}"))]
    ParseServiceStatus {
        #[serde(skip)]
        source: serde_json::Error,
    },
    #[snafu(display("timed out after {limit_ms} ms"))]
    TimedOut { limit_ms: u64 },
    #[snafu(display("timed out after {limit_ms} ms; the command is still running"))]
    StillRunning { limit_ms: u64 },
    #[snafu(display("the command's task was cancelled"))]
    TaskCancelled,
}

/// Elevated daemon mechanics behind one narrow boundary. `probe` reports what
/// it actually knows: `Ok` is the status command's real output, `Err` means the
/// daemon could not be queried, never a synthesized "stopped" answer. The
/// consumer parses that output and bounds every call here (including `probe`)
/// with its own timeout, turning an elapsed bound into the same `Err` shape, so
/// a hung daemon surfaces as an unknown phase, not as evidence of anything.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait ServiceProcessControl: Send + Sync + 'static {
    /// Returns the status command's UTF-8 output after checking its exit status.
    async fn probe(&self) -> Result<String, ServiceCommandError>;
    async fn install(&self) -> Result<(), ServiceCommandError>;
    async fn uninstall(&self) -> Result<(), ServiceCommandError>;
    async fn start_daemon(&self) -> Result<(), ServiceCommandError>;
    async fn stop_daemon(&self) -> Result<(), ServiceCommandError>;
    async fn update(&self) -> Result<(), ServiceCommandError>;
}
