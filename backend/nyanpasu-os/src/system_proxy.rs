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
        let current = Sysproxy::get_system_proxy()
            .boxed()
            .context(ReadOsProxySnafu)?;
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
