use std::time::Duration;

use super::candy;
use crate::client::bundle_metadata::BundleMetadata;

#[tracing_attributes::instrument]
pub async fn url_delay_test(
    url: &str,
    expected_status: u16,
    self_proxy_port: u16,
    metadata: &BundleMetadata,
) -> Option<u64> {
    // heat up
    let client = candy::get_reqwest_client(self_proxy_port, metadata).ok()?;
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

#[tracing_attributes::instrument]
pub async fn get_ipsb_asn(
    self_proxy_port: u16,
    metadata: &BundleMetadata,
) -> anyhow::Result<serde_json::Value> {
    let client = candy::get_reqwest_client(self_proxy_port, metadata)?;
    let response = client
        .get("https://api.ip.sb/geoip")
        .send()
        .await?
        .error_for_status()?;
    let data: serde_json::Value = response.json().await?;
    Ok(data)
}
