use serde::Serialize;
use snafu::Snafu;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use snafu::{ResultExt as _, ensure};

/// Why the system DNS cache could not be flushed. No variant is compiled out
/// per platform, so the generated bindings do not depend on the build host.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemDnsError {
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

#[cfg_attr(test, mockall::automock)]
pub trait SystemDnsCache: Send + Sync + 'static {
    fn flush(&self) -> Result<(), SystemDnsError>;
}

#[derive(Debug, Default)]
pub struct OsSystemDnsCache;

#[cfg(target_os = "windows")]
const WINDOWS_PROGRAM: &str = "ipconfig.exe";
#[cfg(target_os = "windows")]
const WINDOWS_ARGS: &[&str] = &["/flushdns"];

#[cfg(target_os = "macos")]
const MACOS_PROGRAM: &str = "/usr/bin/osascript";
#[cfg(target_os = "macos")]
const MACOS_SCRIPT: &str = concat!(
    "do shell script \"/usr/bin/dscacheutil -flushcache && ",
    "/usr/bin/killall -HUP mDNSResponder\" with administrator privileges"
);

impl SystemDnsCache for OsSystemDnsCache {
    fn flush(&self) -> Result<(), SystemDnsError> {
        flush_system_dns_cache()
    }
}

#[cfg(target_os = "windows")]
fn flush_system_dns_cache() -> Result<(), SystemDnsError> {
    let status = runas::Command::new(WINDOWS_PROGRAM)
        .args(WINDOWS_ARGS)
        .gui(true)
        .show(false)
        .status()
        .context(RunFlushCommandSnafu {
            command: WINDOWS_PROGRAM,
        })?;

    ensure_success(status, WINDOWS_PROGRAM)
}

#[cfg(target_os = "macos")]
fn flush_system_dns_cache() -> Result<(), SystemDnsError> {
    let status = std::process::Command::new(MACOS_PROGRAM)
        .args(["-e", MACOS_SCRIPT])
        .status()
        .context(RunFlushCommandSnafu {
            command: MACOS_PROGRAM,
        })?;

    ensure_success(status, MACOS_PROGRAM)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn flush_system_dns_cache() -> Result<(), SystemDnsError> {
    UnsupportedSnafu.fail()
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn ensure_success(
    status: std::process::ExitStatus,
    command: &'static str,
) -> Result<(), SystemDnsError> {
    ensure!(
        status.success(),
        FlushRejectedSnafu {
            command,
            code: status.code()
        }
    );
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::{WINDOWS_ARGS, WINDOWS_PROGRAM};

    #[test]
    fn windows_flush_uses_ipconfig() {
        assert_eq!(WINDOWS_PROGRAM, "ipconfig.exe");
        assert_eq!(WINDOWS_ARGS, ["/flushdns"]);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::MACOS_SCRIPT;

    #[test]
    fn macos_flush_covers_both_resolver_caches() {
        assert!(MACOS_SCRIPT.contains("dscacheutil -flushcache"));
        assert!(MACOS_SCRIPT.contains("killall -HUP mDNSResponder"));
    }
}
