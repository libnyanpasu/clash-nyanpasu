//! Shared reqwest proxy and user-agent policy; the system proxy is read per operation.

use anyhow::Result;
use nyanpasu_contracts::{bundle_metadata::BundleMetadata, net::SystemProxySource};

/// The app's own mixed port as a proxy URL. Callers pass the port the facade
/// reports (`NyanpasuClient::clash_info`): the confirmed session binding, else
/// the configured start port.
pub fn get_self_proxy(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Adds proxies in the legacy order without failing on invalid proxy URLs.
pub trait NyanpasuReqwestProxyExt {
    /// Adds HTTP, HTTPS and catch-all proxy entries for one URL.
    fn swift_set_proxy(self, url: &str) -> Self;

    /// Adds self-proxy entries before reading and adding the live system proxy.
    fn swift_set_nyanpasu_proxy(
        self,
        self_proxy_port: u16,
        system_proxy: &dyn SystemProxySource,
    ) -> Self;
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
    fn swift_set_nyanpasu_proxy(
        self,
        self_proxy_port: u16,
        system_proxy: &dyn SystemProxySource,
    ) -> Self {
        let mut builder = self.swift_set_proxy(&get_self_proxy(self_proxy_port));
        if let Ok(Some(proxy)) = system_proxy.system_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        builder
    }
}

// TODO: 添加自定义 User-Agent 等配置，说白了就是重构一下 prfitem 的那坨代码
/// Builds an icon/diagnostics client using live proxies and the app-version user agent.
///
/// # Errors
/// Returns the reqwest client-construction error without discarding its source.
pub fn get_reqwest_client(
    self_proxy_port: u16,
    metadata: &BundleMetadata,
    system_proxy: &dyn SystemProxySource,
) -> Result<reqwest::Client> {
    let builder = reqwest::ClientBuilder::new();
    let app_version = metadata.app_version;
    let client = builder
        .swift_set_nyanpasu_proxy(self_proxy_port, system_proxy)
        .user_agent(format!("clash-nyanpasu/{app_version}"))
        .build()?;
    Ok(client)
}
