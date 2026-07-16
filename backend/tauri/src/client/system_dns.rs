#[cfg(any(target_os = "windows", target_os = "macos"))]
use anyhow::Context as _;

#[cfg_attr(test, mockall::automock)]
pub trait SystemDnsCache: Send + Sync + 'static {
    fn flush(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
pub struct OsSystemDnsCache;

#[cfg(target_os = "windows")]
const WINDOWS_PROGRAM: &str = "ipconfig.exe";
#[cfg(target_os = "windows")]
const WINDOWS_ARGS: &[&str] = &["/flushdns"];

#[cfg(target_os = "macos")]
const MACOS_SCRIPT: &str = concat!(
    "do shell script \"/usr/bin/dscacheutil -flushcache && ",
    "/usr/bin/killall -HUP mDNSResponder\" with administrator privileges"
);

impl SystemDnsCache for OsSystemDnsCache {
    fn flush(&self) -> anyhow::Result<()> {
        flush_system_dns_cache()
    }
}

#[cfg(target_os = "windows")]
fn flush_system_dns_cache() -> anyhow::Result<()> {
    let status = runas::Command::new(WINDOWS_PROGRAM)
        .args(WINDOWS_ARGS)
        .gui(true)
        .show(false)
        .status()
        .context("failed to request permission to flush the Windows DNS cache")?;

    ensure_success(status, "ipconfig /flushdns")
}

#[cfg(target_os = "macos")]
fn flush_system_dns_cache() -> anyhow::Result<()> {
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", MACOS_SCRIPT])
        .status()
        .context("failed to request permission to flush the macOS DNS cache")?;

    ensure_success(status, "macOS DNS cache flush")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn flush_system_dns_cache() -> anyhow::Result<()> {
    anyhow::bail!("flushing the system DNS cache is not supported on this platform")
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn ensure_success(status: std::process::ExitStatus, operation: &str) -> anyhow::Result<()> {
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("{operation} failed with status {status}")
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct NoopSystemDnsCache;

#[cfg(test)]
impl SystemDnsCache for NoopSystemDnsCache {
    fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
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
