//! Platform DNS cache flush commands.

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use nyanpasu_contracts::system_dns::UnsupportedSnafu;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use nyanpasu_contracts::system_dns::{FlushRejectedSnafu, RunFlushCommandSnafu};
use nyanpasu_contracts::system_dns::{SystemDnsCache, SystemDnsError};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use snafu::{ResultExt as _, ensure};

/// Flushes the operating system DNS caches using the existing authorization mechanism.
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
