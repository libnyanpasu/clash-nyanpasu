use anyhow::Result;
use sysproxy::Sysproxy;

/// The app's own mixed port as a proxy URL. Callers pass the port the facade
/// reports (`NyanpasuClient::clash_info`): the confirmed session binding, else
/// the configured start port.
pub fn get_self_proxy(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn get_system_proxy() -> Result<Option<String>> {
    let p = Sysproxy::get_system_proxy()?;
    if p.enable {
        let proxy_scheme = format!("http://{}:{}", p.host, p.port);
        return Ok(Some(proxy_scheme));
    }

    Ok(None)
}

pub trait NyanpasuReqwestProxyExt {
    fn swift_set_proxy(self, url: &str) -> Self;

    fn swift_set_nyanpasu_proxy(self, self_proxy_port: u16) -> Self;
}

impl NyanpasuReqwestProxyExt for reqwest::ClientBuilder {
    fn swift_set_proxy(self, url: &str) -> Self {
        let mut builder = self;
        if let Ok(proxy) = reqwest::Proxy::http(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::https(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::all(url) {
            builder = builder.proxy(proxy);
        }
        builder
    }

    // TODO: 修改成按枚举配置
    fn swift_set_nyanpasu_proxy(self, self_proxy_port: u16) -> Self {
        let mut builder = self.swift_set_proxy(&get_self_proxy(self_proxy_port));
        if let Ok(Some(proxy)) = get_system_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        builder
    }
}
