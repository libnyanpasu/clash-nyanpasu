//! The system DNS cache boundary and its portable failure contract.

use serde::Serialize;
use snafu::Snafu;

/// Why the system DNS cache could not be flushed. No variant is compiled out
/// per platform, so the generated bindings do not depend on the build host.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemDnsError {
    #[snafu(display("could not authorize flushing the system DNS cache"))]
    AuthorizeDnsFlush {
        source: crate::elevation::ElevationConsentError,
    },
    /// The elevated flush could not be started: the user declined the prompt,
    /// or the platform has no way to ask for elevation.
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    #[snafu(display("could not run {command} to flush the system DNS cache"))]
    RunFlushCommand {
        command: &'static str,
        #[serde(skip)]
        source: std::io::Error,
    },
    /// The flush ran and reported failure; declining the macOS authorization
    /// dialog lands here too.
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    #[snafu(display("{command} did not succeed (exit code {code:?})"))]
    FlushRejected {
        command: &'static str,
        code: Option<i32>,
    },
    #[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
    #[snafu(display("flushing the system DNS cache is not supported on this platform"))]
    Unsupported,
}

#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait SystemDnsCache: Send + Sync + 'static {
    fn flush(&self) -> Result<(), SystemDnsError>;
}
