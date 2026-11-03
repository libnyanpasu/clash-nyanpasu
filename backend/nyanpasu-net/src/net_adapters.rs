//! HTTP icon and diagnostic adapters using injected metadata and live proxy settings.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use bytes::Bytes;
use nyanpasu_contracts::{
    bundle_metadata::BundleMetadata,
    net::{IconFetcher, NetworkDiagnostics, SystemProxySource},
};
use url::Url;

use crate::policy;

/// Builds a fresh policy-configured client for each operation, using its caller-supplied port.
pub struct ReqwestNetworkAdapter {
    metadata: BundleMetadata,
    system_proxy: Arc<dyn SystemProxySource>,
}

impl ReqwestNetworkAdapter {
    /// Keeps stable metadata and a live proxy reader; no session port is captured.
    pub fn new(metadata: BundleMetadata, system_proxy: Arc<dyn SystemProxySource>) -> Self {
        Self {
            metadata,
            system_proxy,
        }
    }
}

#[async_trait::async_trait]
impl IconFetcher for ReqwestNetworkAdapter {
    async fn fetch(&self, url: &Url, self_proxy_port: u16) -> Result<(String, Bytes)> {
        let client = policy::get_reqwest_client(
            self_proxy_port,
            &self.metadata,
            self.system_proxy.as_ref(),
        )?;
        let response = client.get(url.clone()).send().await?.error_for_status()?;
        let mime = response
            .headers()
            .get("content-type")
            .context("icon response has no content-type")?
            .to_str()?
            .to_owned();
        Ok((mime, response.bytes().await?))
    }
}

#[async_trait::async_trait]
impl NetworkDiagnostics for ReqwestNetworkAdapter {
    #[tracing_attributes::instrument(skip(self), fields(metadata = ?self.metadata))]
    async fn url_delay_test(
        &self,
        url: &str,
        expected_status: u16,
        self_proxy_port: u16,
    ) -> Option<u64> {
        // heat up
        let client =
            policy::get_reqwest_client(self_proxy_port, &self.metadata, self.system_proxy.as_ref())
                .ok()?;
        let _ = tokio::time::timeout(Duration::from_secs(10), client.get(url).send())
            .await
            .ok()?
            .ok()?;
        let tick = tokio::time::Instant::now();
        let response = tokio::time::timeout(Duration::from_secs(10), client.get(url).send())
            .await
            .ok()?
            .ok()?;
        if response.status().as_u16() != expected_status {
            return None;
        }
        Some(tick.elapsed().as_millis() as u64)
    }

    #[tracing_attributes::instrument(skip(self), fields(metadata = ?self.metadata))]
    async fn get_ipsb_asn(&self, self_proxy_port: u16) -> Result<serde_json::Value> {
        let client = policy::get_reqwest_client(
            self_proxy_port,
            &self.metadata,
            self.system_proxy.as_ref(),
        )?;
        let response = client
            .get("https://api.ip.sb/geoip")
            .send()
            .await?
            .error_for_status()?;
        let data: serde_json::Value = response.json().await?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nyanpasu_contracts::{
        bundle_metadata::TEST_BUNDLE_METADATA,
        net::{IconFetcher as _, MockSystemProxySource},
    };
    use url::Url;

    use super::ReqwestNetworkAdapter;

    #[tokio::test]
    async fn system_proxy_is_read_for_each_operation() {
        let mut source = MockSystemProxySource::new();
        source.expect_system_proxy().times(2).returning(|| Ok(None));
        let adapter = ReqwestNetworkAdapter::new(TEST_BUNDLE_METADATA, Arc::new(source));
        // An unsupported scheme exercises client construction without making an outbound request.
        let url = Url::parse("file:///icon.png").unwrap();

        for port in [7891, 7892] {
            let error = adapter.fetch(&url, port).await.unwrap_err();
            assert!(error.is::<reqwest::Error>());
        }
    }
}
