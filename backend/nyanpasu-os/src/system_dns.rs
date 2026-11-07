//! Platform DNS cache flush commands.

use std::sync::Arc;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use nyanpasu_contracts::system_dns::UnsupportedSnafu;
use nyanpasu_contracts::{
    elevation::ElevationConsent,
    system_dns::{SystemDnsCache, SystemDnsError},
};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use nyanpasu_contracts::{
    elevation::ElevationOperation,
    system_dns::{AuthorizeDnsFlushSnafu, FlushRejectedSnafu, RunFlushCommandSnafu},
};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use snafu::{ResultExt as _, ensure};

use crate::elevation::runner::{CommandRunner, OsCommandRunner};

/// Flushes the operating system DNS caches using the existing authorization mechanism.
pub struct OsSystemDnsCache {
    consent: Arc<dyn ElevationConsent>,
}

impl OsSystemDnsCache {
    /// Uses the shell's policy before requesting OS authorization.
    pub fn new(consent: Arc<dyn ElevationConsent>) -> Self {
        Self { consent }
    }
}

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
        flush_system_dns_cache(self.consent.as_ref(), &OsCommandRunner)
    }
}

#[cfg(target_os = "windows")]
fn flush_system_dns_cache(
    consent: &dyn ElevationConsent,
    runner: &dyn CommandRunner,
) -> Result<(), SystemDnsError> {
    consent
        .authorize(ElevationOperation::DnsFlush)
        .context(AuthorizeDnsFlushSnafu)?;
    let status = runner
        .runas(
            runas::Command::new(WINDOWS_PROGRAM)
                .args(WINDOWS_ARGS)
                .gui(true)
                .show(false),
        )
        .context(RunFlushCommandSnafu {
            command: WINDOWS_PROGRAM,
        })?;

    ensure_success(status, WINDOWS_PROGRAM)
}

#[cfg(target_os = "macos")]
fn flush_system_dns_cache(
    consent: &dyn ElevationConsent,
    runner: &dyn CommandRunner,
) -> Result<(), SystemDnsError> {
    consent
        .authorize(ElevationOperation::DnsFlush)
        .context(AuthorizeDnsFlushSnafu)?;
    let status = runner
        .command(std::process::Command::new(MACOS_PROGRAM).args(["-e", MACOS_SCRIPT]))
        .context(RunFlushCommandSnafu {
            command: MACOS_PROGRAM,
        })?;

    ensure_success(status, MACOS_PROGRAM)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn flush_system_dns_cache(
    _consent: &dyn ElevationConsent,
    _runner: &dyn CommandRunner,
) -> Result<(), SystemDnsError> {
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

#[cfg(all(test, any(target_os = "windows", target_os = "macos")))]
mod consent_tests {
    use super::*;
    use crate::elevation::{RefuseElevation, runner::CountingRunner};
    use nyanpasu_contracts::elevation::ElevationConsentError;
    use std::sync::atomic::Ordering;

    #[test]
    fn refused_dns_flush_never_spawns() {
        let runner = CountingRunner::default();
        let error = flush_system_dns_cache(&RefuseElevation, &runner).unwrap_err();
        assert_eq!(runner.0.load(Ordering::SeqCst), 0);
        assert!(matches!(
            error,
            SystemDnsError::AuthorizeDnsFlush {
                source: ElevationConsentError::AuthorizeElevation {
                    operation: ElevationOperation::DnsFlush
                }
            }
        ));
    }
}
