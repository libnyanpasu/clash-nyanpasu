use super::{
    request::{Data, PerformRequest, Query},
    types::*,
};
use anyhow::{Context, Result};
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde::Serialize;
use serde_yaml::Mapping;
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone)]
pub struct ClashClient {
    pub server: String,
    pub secret: Option<String>,
}

impl ClashClient {
    pub fn from_global_config() -> Self {
        let info = crate::config::Config::clash().data().get_client_info();
        Self {
            server: info.server,
            secret: info.secret,
        }
    }

    pub fn from_info(info: &crate::config::ClashInfo) -> Self {
        Self {
            server: info.server.clone(),
            secret: info.secret.clone(),
        }
    }

    fn make_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse()?);

        if let Some(secret) = &self.secret {
            let auth = format!("Bearer {secret}").parse()?;
            headers.insert("Authorization", auth);
        }

        Ok(headers)
    }

    fn base_url(&self) -> Result<Url> {
        let url = format!("http://{}", self.server);
        Url::parse(&url).context("failed to parse server url")
    }

    async fn request<D, Q>(
        &self,
        param: impl Into<PerformRequest<D, Q>>,
    ) -> Result<reqwest::Response>
    where
        Q: Serialize + core::fmt::Debug,
        D: Serialize + core::fmt::Debug,
    {
        let PerformRequest {
            method,
            path,
            data,
            query,
        } = param.into();

        let headers = self.make_headers().context("failed to build headers")?;
        let base_url = self.base_url()?;
        let opts = Url::options().base_url(Some(&base_url));
        let url = opts.parse(&path).context("failed to parse path")?;

        async {
            let client = reqwest::ClientBuilder::new().no_proxy().build()?;
            let mut builder = client.request(method.clone(), url.clone()).headers(headers);

            if let Some(query) = &query {
                builder = builder.query(query);
            }
            if let Some(data) = &data {
                builder = builder.json(data);
            }

            let resp = builder.send().await?;

            if let Err(err) = resp.error_for_status_ref() {
                match err.status() {
                    Some(StatusCode::BAD_REQUEST) => {
                        let Ok(bytes) = resp.bytes().await else {
                            return Err(err.into());
                        };

                        let message: serde_json::Value = match serde_json::from_slice(&bytes) {
                            Ok(v) => v,
                            Err(_) => {
                                let s = String::from_utf8_lossy(&bytes);
                                serde_json::Value::String(s.to_string())
                            }
                        };

                        return Err(err).context(format!("message: {message}"));
                    }
                    _ => return Err(err).context("clash api error"),
                }
            }
            Ok(resp)
        }
        .await
        .inspect_err(|e| {
            tracing::error!(
                method = %method,
                url = %url,
                query = ?query,
                data = ?data,
                "failed to perform request: {:?}",
                e
            )
        })
    }

    /// GET /configs
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_configs(&self) -> Result<ClashConfig> {
        let path = "/configs";
        let resp: ClashConfig = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// GET /version
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_version(&self) -> Result<ClashVersion> {
        let path = "/version";
        let resp: ClashVersion = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// GET /rules
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_rules(&self) -> Result<RulesRes> {
        let path = "/rules";
        let resp: RulesRes = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// GET /providers/rules
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_providers_rules(&self) -> Result<ProvidersRulesRes> {
        let path = "/providers/rules";
        let resp: ProvidersRulesRes = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// PUT /providers/rules/:name
    #[tracing_attributes::instrument(skip(self))]
    pub async fn update_providers_rules_group(&self, name: &str) -> Result<()> {
        let path = format!("/providers/rules/{name}");
        let _ = self.request((Method::PUT, path.as_str())).await?;
        Ok(())
    }

    /// GET /group/:name/delay
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_group_delay(
        &self,
        group: &str,
        url: Option<&str>,
    ) -> Result<HashMap<String, u32>> {
        let path = format!("/group/{group}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = url
            .map(|s| if s.is_empty() { default_url } else { s })
            .unwrap_or(default_url);

        let query = Query([("timeout", "10000"), ("url", test_url)]);
        let resp: HashMap<String, u32> = self
            .request((Method::GET, path.as_str(), query))
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// PUT /configs — `config_path` must be an absolute filesystem path.
    #[tracing_attributes::instrument(skip(self))]
    pub async fn put_configs(&self, config_path: &str) -> Result<()> {
        let path = "/configs";

        let mut data = HashMap::new();
        data.insert("path", config_path);

        let _ = self.request((Method::PUT, path, Data(data))).await?;

        Ok(())
    }

    /// PATCH /configs
    #[tracing_attributes::instrument(skip(self))]
    pub async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        let path = "/configs";
        let _ = self.request((Method::PATCH, path, Data(config))).await?;
        Ok(())
    }

    /// GET /proxies
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_proxies(&self) -> Result<ProxiesRes> {
        let path = "/proxies";
        let resp: ProxiesRes = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// GET /proxies/{name}
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_proxy(&self, name: &str) -> Result<ProxyItem> {
        let path = format!("/proxies/{name}");
        let resp: ProxyItem = self
            .request((Method::GET, path.as_str()))
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// PUT /proxies/{group}
    #[tracing_attributes::instrument(skip(self))]
    pub async fn update_proxy(&self, group: &str, name: &str) -> Result<()> {
        let path = format!("/proxies/{group}");

        let mut data = HashMap::new();
        data.insert("name", name);

        let _ = self
            .request((Method::PUT, path.as_str(), Data(data)))
            .await?;
        Ok(())
    }

    /// GET /providers/proxies
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_providers_proxies(&self) -> Result<ProvidersProxiesRes> {
        let path = "/providers/proxies";
        let resp: ProvidersProxiesRes = self.request((Method::GET, path)).await?.json().await?;
        Ok(resp)
    }

    /// GET /providers/proxies/:name
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_providers_proxies_group(&self, group: &str) -> Result<ProxyProviderItem> {
        let path = format!("/providers/proxies/{group}");
        let resp: ProxyProviderItem = self
            .request((Method::GET, path.as_str()))
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// PUT /providers/proxies/:name
    #[tracing_attributes::instrument(skip(self))]
    pub async fn update_providers_proxies_group(&self, name: &str) -> Result<()> {
        let path = format!("/providers/proxies/{name}");
        let _ = self.request((Method::PUT, path.as_str())).await?;
        Ok(())
    }

    /// GET /providers/proxies/:name/healthcheck
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_providers_proxies_healthcheck(&self, name: &str) -> Result<Mapping> {
        let path = format!("/providers/proxies/{name}/healthcheck");
        let resp: Mapping = self
            .request((Method::GET, path.as_str()))
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// GET /proxies/{name}/delay
    #[tracing_attributes::instrument(skip(self))]
    pub async fn get_proxy_delay(&self, name: &str, test_url: Option<&str>) -> Result<DelayRes> {
        let path = format!("/proxies/{name}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = test_url
            .map(|s| if s.is_empty() { default_url } else { s })
            .unwrap_or(default_url);

        let query = Query([("timeout", "10000"), ("url", test_url)]);
        let resp: DelayRes = self
            .request((Method::GET, path.as_str(), query))
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// DELETE /connections
    #[tracing_attributes::instrument(skip(self))]
    pub async fn delete_connections(&self, id: Option<&str>) -> Result<()> {
        let path = match id {
            Some(id) => format!("/connections/{}", id),
            None => "/connections".to_string(),
        };

        let _ = self.request((Method::DELETE, path.as_str())).await?;
        Ok(())
    }
}
