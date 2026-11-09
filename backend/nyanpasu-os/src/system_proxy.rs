//! OS proxy settings adapter over sysproxy.

use nyanpasu_contracts::system_proxy::{
    OsProxyConfig, OsProxyError, OsProxyPort, ReadOsProxySnafu, WriteOsProxySnafu,
};
use snafu::ResultExt as _;
use sysproxy::Sysproxy;

#[cfg(target_os = "windows")]
const DEFAULT_BYPASS: &str = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
#[cfg(target_os = "linux")]
const DEFAULT_BYPASS: &str = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
#[cfg(target_os = "macos")]
const DEFAULT_BYPASS: &str =
    "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

/// The platform proxy settings, via `sysproxy`.
pub struct SysproxyOsProxy;

impl OsProxyPort for SysproxyOsProxy {
    fn get(&self) -> Result<OsProxyConfig, OsProxyError> {
        let current = read_system_proxy().boxed().context(ReadOsProxySnafu)?;
        Ok(OsProxyConfig {
            enable: current.enable,
            host: current.host,
            port: current.port,
            bypass: current.bypass,
        })
    }

    fn set(&self, config: &OsProxyConfig) -> Result<(), OsProxyError> {
        Sysproxy {
            enable: config.enable,
            host: config.host.clone(),
            port: config.port,
            bypass: config.bypass.clone(),
        }
        .set_system_proxy()
        .boxed()
        .context(WriteOsProxySnafu {
            enable: config.enable,
            host: &config.host,
            port: config.port,
        })
    }

    fn default_bypass(&self) -> &'static str {
        DEFAULT_BYPASS
    }
}

#[cfg(target_os = "linux")]
fn read_system_proxy() -> sysproxy::Result<Sysproxy> {
    recover_unconfigured_kde_proxy(Sysproxy::get_system_proxy(), Sysproxy::get_enable)
}

#[cfg(not(target_os = "linux"))]
fn read_system_proxy() -> sysproxy::Result<Sysproxy> {
    Sysproxy::get_system_proxy()
}

/// `sysproxy` reports both an absent KDE proxy group and malformed active
/// entries as the same parse error. Only the former is a disabled state.
#[cfg(target_os = "linux")]
fn recover_unconfigured_kde_proxy(
    current: sysproxy::Result<Sysproxy>,
    enabled: impl FnOnce() -> sysproxy::Result<bool>,
) -> sysproxy::Result<Sysproxy> {
    match current {
        Ok(current) => Ok(current),
        Err(sysproxy::Error::ParseStr(field)) if field == "schema" => match enabled() {
            Ok(false) => Ok(Sysproxy::default()),
            _ => Err(sysproxy::Error::ParseStr(field)),
        },
        Err(error) => Err(error),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn an_absent_kde_proxy_reads_as_disabled() {
        let current = recover_unconfigured_kde_proxy(
            Err(sysproxy::Error::ParseStr("schema".to_owned())),
            || Ok(false),
        );
        assert_eq!(
            current.expect("an absent KDE proxy is a valid disabled configuration"),
            Sysproxy::default()
        );
    }

    #[test]
    fn an_enabled_kde_proxy_with_a_missing_entry_remains_an_error() {
        let error = recover_unconfigured_kde_proxy(
            Err(sysproxy::Error::ParseStr("schema".to_owned())),
            || Ok(true),
        )
        .expect_err("an enabled proxy must not be reported as disabled");

        assert!(matches!(error, sysproxy::Error::ParseStr(field) if field == "schema"));
    }

    #[test]
    fn an_unreadable_kde_enabled_state_keeps_the_original_error() {
        let error = recover_unconfigured_kde_proxy(
            Err(sysproxy::Error::ParseStr("schema".to_owned())),
            || Err(sysproxy::Error::ParseStr("mode".to_owned())),
        )
        .expect_err("an unknown enabled state must not be reported as disabled");

        assert!(matches!(error, sysproxy::Error::ParseStr(field) if field == "schema"));
    }

    #[test]
    fn other_parse_failures_do_not_probe_the_enabled_state() {
        let error = recover_unconfigured_kde_proxy(
            Err(sysproxy::Error::ParseStr("host".to_owned())),
            || panic!("an unrelated parse failure must not read the enabled state"),
        )
        .expect_err("a malformed proxy value is a read failure");

        assert!(matches!(error, sysproxy::Error::ParseStr(field) if field == "host"));
    }
}
