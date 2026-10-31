use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use crate::service::profile_file::ports::SelfProxyPortSource;
use nyanpasu_core::client::bundle_metadata::BundleMetadata;

pub(crate) struct ReqwestChangelogHttp {
    proxy_port: Arc<dyn SelfProxyPortSource>,
    bundle_metadata: BundleMetadata,
}

impl ReqwestChangelogHttp {
    pub fn new(proxy_port: Arc<dyn SelfProxyPortSource>, bundle_metadata: BundleMetadata) -> Self {
        Self {
            proxy_port,
            bundle_metadata,
        }
    }
}

#[async_trait]
impl crate::client::app_update::changelog::ChangelogHttp for ReqwestChangelogHttp {
    async fn get_text(&self, url: &url::Url) -> Result<String> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!(
                "clash-nyanpasu/{}",
                self.bundle_metadata.package_version
            ))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30));
        if let Some(port) = self.proxy_port.mixed_port() {
            builder = builder.proxy(reqwest::Proxy::all(
                nyanpasu_core::utils::config::get_self_proxy(port),
            )?);
        }
        if let Ok(Some(proxy)) = nyanpasu_core::utils::config::get_system_proxy() {
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
