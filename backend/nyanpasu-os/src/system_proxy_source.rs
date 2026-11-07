//! Reads the current OS proxy for network consumers.

use anyhow::Result;
use nyanpasu_contracts::net::SystemProxySource;
use sysproxy::Sysproxy;

fn get_system_proxy() -> Result<Option<String>> {
    let p = Sysproxy::get_system_proxy()?;
    if p.enable {
        let proxy_scheme = format!("http://{}:{}", p.host, p.port);
        return Ok(Some(proxy_scheme));
    }

    Ok(None)
}

/// Reads the OS proxy at the moment a consumer starts an operation.
pub struct OsSystemProxySource;

impl SystemProxySource for OsSystemProxySource {
    fn system_proxy(&self) -> Result<Option<String>> {
        get_system_proxy()
    }
}
