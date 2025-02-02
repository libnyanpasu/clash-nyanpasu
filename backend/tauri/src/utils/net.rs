use std::time::Duration;

use super::candy::get_reqwest_client;

#[tracing_attributes::instrument]
pub async fn url_delay_test(url: &str, expected_status: u16) -> Option<u64> {
    // heat up
    let client = get_reqwest_client().ok()?;
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
pub async fn get_ipsb_asn() -> anyhow::Result<serde_json::Value> {
    let client = get_reqwest_client()?;
    let response = client
        .get("https://api.ip.sb/geoip")
        .send()
        .await?
        .error_for_status()?;
    let data: serde_json::Value = response.json().await?;
    Ok(data)
}
