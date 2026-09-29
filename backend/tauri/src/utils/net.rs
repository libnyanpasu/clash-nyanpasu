use std::{sync::Arc, time::Duration};

use anyhow::Result;
use serde_json::Value;

#[async_trait::async_trait]
pub trait HttpGet: Send + Sync {
    async fn get_status(&self, url: &str) -> Result<u16>;
    async fn get_json(&self, url: &str) -> Result<Value>;
}

#[derive(Clone)]
pub struct NetworkHttp(pub Arc<dyn HttpGet>);

impl NetworkHttp {
    pub fn new(http: Arc<dyn HttpGet>) -> Self {
        Self(http)
    }
}

pub struct ReqwestHttpGet {
    client: reqwest::Client,
}

impl ReqwestHttpGet {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl HttpGet for ReqwestHttpGet {
    async fn get_status(&self, url: &str) -> Result<u16> {
        Ok(self.client.get(url).send().await?.status().as_u16())
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        Ok(response.json().await?)
    }
}

#[tracing_attributes::instrument(skip(http))]
pub async fn url_delay_test(http: &dyn HttpGet, url: &str, expected_status: u16) -> Option<u64> {
    // heat up
    let _ = tokio::time::timeout(Duration::from_secs(10), http.get_status(url))
        .await
        .ok()?
        .ok()?;
    let tick = tokio::time::Instant::now();
    let status = tokio::time::timeout(Duration::from_secs(10), http.get_status(url))
        .await
        .ok()?
        .ok()?;
    if status != expected_status {
        return None;
    }
    Some(tick.elapsed().as_millis() as u64)
}

#[tracing_attributes::instrument(skip(http))]
pub async fn get_ipsb_asn(http: &dyn HttpGet) -> anyhow::Result<serde_json::Value> {
    http.get_json("https://api.ip.sb/geoip").await
}
