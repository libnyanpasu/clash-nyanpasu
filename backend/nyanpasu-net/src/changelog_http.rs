//! Changelog HTTP adapter with live self and system proxy sources.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use nyanpasu_contracts::{
    bundle_metadata::BundleMetadata,
    net::{SelfProxyPortSource, SystemProxySource},
};

/// Fetches release notes with the bundle user agent and existing HTTP deadlines.
pub struct ReqwestChangelogHttp {
    proxy_port: Arc<dyn SelfProxyPortSource>,
    bundle_metadata: BundleMetadata,
    system_proxy: Arc<dyn SystemProxySource>,
}

impl ReqwestChangelogHttp {
    /// Retains metadata and readers; proxy settings are resolved for each request.
    pub fn new(
        proxy_port: Arc<dyn SelfProxyPortSource>,
        bundle_metadata: BundleMetadata,
        system_proxy: Arc<dyn SystemProxySource>,
    ) -> Self {
        Self {
            proxy_port,
            bundle_metadata,
            system_proxy,
        }
    }
}

#[async_trait]
impl nyanpasu_contracts::net::ChangelogHttp for ReqwestChangelogHttp {
    async fn get_text(&self, url: &url::Url) -> Result<String> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!(
                "clash-nyanpasu/{}",
                self.bundle_metadata.package_version
            ))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30));
        if let Some(port) = self.proxy_port.mixed_port() {
            builder = builder.proxy(reqwest::Proxy::all(crate::policy::get_self_proxy(port))?);
        }
        if let Ok(Some(proxy)) = self.system_proxy.system_proxy() {
            builder = builder.proxy(reqwest::Proxy::all(proxy).context("invalid system proxy")?);
        }
        Ok(builder
            .build()?
            .get(url.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }
}
