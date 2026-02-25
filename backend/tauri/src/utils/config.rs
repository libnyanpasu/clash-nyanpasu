use anyhow::Result;
use sysproxy::Sysproxy;

use crate::config::ConfigService;

pub fn get_self_proxy() -> Result<String> {
    let port = ConfigService::verge()
        .latest()
        .verge_mixed_port
        .unwrap_or(ConfigService::clash().data().get_mixed_port());

    let proxy_scheme = format!("http://127.0.0.1:{port}");
    Ok(proxy_scheme)
}

pub fn get_system_proxy() -> Result<Option<String>> {
    let p = Sysproxy::get_system_proxy()?;
    if p.enable {
        let proxy_scheme = format!("http://{}:{}", p.host, p.port);
        return Ok(Some(proxy_scheme));
    }

    Ok(None)
}

pub fn get_current_clash_mode() -> String {
    ConfigService::clash()
        .latest()
        .0
        .get("mode")
        .map(|val| val.as_str().unwrap_or("rule"))
        .unwrap_or("rule")
        .to_owned()
}

pub trait NyanpasuReqwestProxyExt {
    fn swift_set_proxy(self, url: &str) -> Self;

    fn swift_set_nyanpasu_proxy(self) -> Self;
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
    fn swift_set_nyanpasu_proxy(self) -> Self {
        let mut builder = self;
        if let Ok(proxy) = get_self_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        if let Ok(Some(proxy)) = get_system_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        builder
    }
}
