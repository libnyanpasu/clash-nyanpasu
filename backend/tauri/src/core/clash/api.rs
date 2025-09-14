use crate::config::Config;
use anyhow::{Context, Result};
use indexmap::IndexMap;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use tracing_attributes::instrument;
use url::Url;

/// PUT /configs
/// path 是绝对路径
#[instrument]
pub async fn put_configs(config_path: &str) -> Result<()> {
    let path = "/configs";

    let mut data = HashMap::new();
    data.insert("path", config_path);

    let _ = perform_request((Method::PUT, path, Data(data))).await?;

    Ok(())
}

/// PATCH /configs
#[instrument]
pub async fn patch_configs(config: &Mapping) -> Result<()> {
    let path = "/configs";
    let _ = perform_request((Method::PATCH, path, Data(config))).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxiesRes {
    #[serde(default)]
    pub proxies: IndexMap<String, ProxyItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItemHistory {
    pub time: String,
    pub delay: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItem {
    pub name: String,
    pub r#type: String, // TODO: 考虑改成枚举
    pub udp: bool,
    pub history: Vec<ProxyItemHistory>,
    pub all: Option<Vec<String>>,
    pub now: Option<String>, // 当前选中的代理
    pub provider: Option<String>,
    pub alive: Option<bool>, // Mihomo Or Premium Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xudp: Option<bool>, // Mihomo Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tfo: Option<bool>, // Mihomo Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
                             // extra: {}, // Mihomo Only
}

impl From<ProxyProviderItem> for ProxyItem {
    fn from(item: ProxyProviderItem) -> Self {
        let ProxyProviderItem {
            name,
            r#type,
            proxies,
            vehicle_type: _,
            test_url: _,
            expected_status: _,
        } = item;

        let now = proxies
            .iter()
            .find(|p| p.now.is_some())
            .map(|p| p.name.clone())
            .unwrap_or_default();

        let all = proxies.iter().map(|p| p.name.clone()).collect();

        Self {
            name,
            r#type: r#type.to_string(),
            udp: false,
            history: vec![],
            all: Some(all),
            now: Some(now),
            provider: None,
            alive: None,
            xudp: None,
            tfo: None,
            icon: None,
            hidden: false,
        }
    }
}

/// GET /proxies
/// 获取代理列表
#[instrument]
pub async fn get_proxies() -> Result<ProxiesRes> {
    let path = "/proxies";
    let resp: ProxiesRes = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// GET /proxies/{name}
/// 获取单个代理
/// name: 代理名称
/// 返回代理的配置
///
#[allow(dead_code)]
#[instrument]
pub async fn get_proxy(name: String) -> Result<ProxyItem> {
    let path = format!("/proxies/{name}");
    let resp: ProxyItem = perform_request((Method::GET, path.as_str()))
        .await?
        .json()
        .await?;
    Ok(resp)
}

/// PUT /proxies/{group}
/// 选择代理
/// group: 代理分组名称
/// name: 代理名称
#[instrument]
pub async fn update_proxy(group: &str, name: &str) -> Result<()> {
    let path = format!("/proxies/{group}");

    let mut data = HashMap::new();
    data.insert("name", name);

    let _ = perform_request((Method::PUT, path.as_str(), Data(data))).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub enum VehicleType {
    File,
    #[serde(rename = "HTTP")]
    Http,
    Compatible,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub enum ProviderType {
    Proxy,
    Rule,
    Unknown,
}

impl Display for ProviderType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ProviderType::Proxy => write!(f, "Proxy"),
            ProviderType::Rule => write!(f, "Rule"),
            ProviderType::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProviderItem {
    pub name: String,
    pub r#type: ProviderType,
    pub proxies: Vec<ProxyItem>,
    pub vehicle_type: VehicleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_url: Option<String>, // Mihomo Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_status: Option<String>, // Mihomo Only
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersProxiesRes {
    #[serde(default)]
    pub providers: IndexMap<String, ProxyProviderItem>,
}

/// GET /providers/proxies
/// 获取所有代理集合的所有代理信息
#[instrument]
pub async fn get_providers_proxies() -> Result<ProvidersProxiesRes> {
    let path = "/providers/proxies";
    let resp: ProvidersProxiesRes = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// GET /providers/proxies/:name
/// 获取单个代理集合的所有代理信息
/// group: 代理集合名称
#[allow(dead_code)]
#[instrument]
pub async fn get_providers_proxies_group(group: String) -> Result<ProxyProviderItem> {
    let path = format!("/providers/proxies/{group}");
    let resp: ProxyProviderItem = perform_request((Method::GET, path.as_str()))
        .await?
        .json()
        .await?;
    Ok(resp)
}

/// PUT /providers/proxies/:name
/// 更新代理集合
/// name: 代理集合名称
#[instrument]
pub async fn update_providers_proxies_group(name: &str) -> Result<()> {
    let path = format!("/providers/proxies/{name}");
    let _ = perform_request((Method::PUT, path.as_str())).await?;
    Ok(())
}

/// GET /providers/proxies/:name/healthcheck
/// 获取代理集合的健康检查
/// name: 代理集合名称
#[allow(dead_code)]
#[instrument]
pub async fn get_providers_proxies_healthcheck(name: String) -> Result<Mapping> {
    let path = format!("/providers/proxies/{name}/healthcheck");
    let resp: Mapping = perform_request((Method::GET, path.as_str()))
        .await?
        .json()
        .await?;
    Ok(resp)
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}

/// GET /proxies/{name}/delay
/// 获取代理延迟
#[instrument]
pub async fn get_proxy_delay(name: String, test_url: Option<String>) -> Result<DelayRes> {
    let path = format!("/proxies/{name}/delay");
    let default_url = "http://www.gstatic.com/generate_204";
    let test_url = test_url
        .map(|s| if s.is_empty() { default_url.into() } else { s })
        .unwrap_or(default_url.into());

    let query = Query([("timeout", "10000"), ("url", &test_url)]);
    let resp: DelayRes = perform_request((Method::GET, path.as_str(), query))
        .await?
        .json()
        .await?;
    Ok(resp)
}

/// 根据clash info获取clash服务地址和请求头
#[instrument]
fn clash_client_info() -> Result<(String, HeaderMap)> {
    let client = { Config::clash().data().get_client_info() };

    let server = format!("http://{}", client.server);

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse()?);

    if let Some(secret) = client.secret {
        let secret = format!("Bearer {secret}").parse()?;
        headers.insert("Authorization", secret);
    }

    Ok((server, headers))
}

/// The Request Parameters
struct PerformRequest<D = (), Q = ()> {
    method: reqwest::Method,
    path: String,
    query: Option<Q>,
    data: Option<D>,
}
/// A newtype wrapper for query parameters
struct Query<T>(T);
/// A newtype wrapper for request body
struct Data<T>(T);

impl From<(reqwest::Method, &str)> for PerformRequest<(), ()> {
    fn from((method, path): (reqwest::Method, &str)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Data<T>)> for PerformRequest<T, ()>
where
    T: Serialize,
{
    fn from((method, path, Data(data)): (reqwest::Method, &str, Data<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: Some(data),
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Query<T>)> for PerformRequest<(), T>
where
    T: Serialize,
{
    fn from((method, path, Query(query)): (reqwest::Method, &str, Query<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: Some(query),
        }
    }
}

impl<D, Q> From<(reqwest::Method, &str, Query<Q>, Data<D>)> for PerformRequest<D, Q>
where
    D: Serialize,
    Q: Serialize,
{
    fn from(
        (method, path, Query(query), Data(data)): (reqwest::Method, &str, Query<Q>, Data<D>),
    ) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: Some(data),
            query: Some(query),
        }
    }
}

#[instrument(skip_all, fields(
    method = tracing::field::Empty,
    url = tracing::field::Empty,
    query = tracing::field::Empty,
    data = tracing::field::Empty,
))]
async fn perform_request<D, Q>(param: impl Into<PerformRequest<D, Q>>) -> Result<reqwest::Response>
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
    let (host, headers) = clash_client_info().context("failed to get clash client info")?;
    let base_url = Url::parse(&host).context("failed to parse host")?;
    let opts = url::Url::options().base_url(Some(&base_url));
    let url = opts.parse(&path).context("failed to parse path")?;

    let span = tracing::Span::current();
    span.record("method", tracing::field::display(&method));
    span.record("url", tracing::field::display(&url));
    span.record("query", tracing::field::debug(&query));
    span.record("data", tracing::field::debug(&data));

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
                // Try To parse error message
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
    .inspect_err(|e| tracing::error!(method = %method, url = %url, query = ?query, data = ?data, "failed to perform request: {:?}", e))
}

/// 缩短clash的日志
#[instrument]
pub fn parse_log(log: String) -> String {
    if log.starts_with("time=") && log.len() > 33 {
        return log[33..].to_owned();
    }
    if log.len() > 9 {
        return log[9..].to_owned();
    }
    log
}

/// 缩短clash -t的错误输出
/// 仅适配 clash p核 8-26、clash meta 1.13.1
#[instrument]
pub fn parse_check_output(log: String) -> String {
    let t = log.find("time=");
    let m = log.find("msg=");
    let mr = log.rfind('"');

    if let (Some(_), Some(m), Some(mr)) = (t, m, mr) {
        let e = match log.find("level=error msg=") {
            Some(e) => e + 17,
            None => m + 5,
        };

        if mr > m {
            return log[e..mr].to_owned();
        }
    }

    let l = log.find("error=");
    let r = log.find("path=").or(Some(log.len()));

    if let (Some(l), Some(r)) = (l, r) {
        return log[(l + 6)..(r - 1)].to_owned();
    }

    log
}

/// DELETE /connections
/// Close all connections or a specific connection by ID
#[instrument]
pub async fn delete_connections(id: Option<&str>) -> Result<()> {
    let path = match id {
        Some(id) => format!("/connections/{}", id),
        None => "/connections".to_string(),
    };
    
    let _ = perform_request((Method::DELETE, path.as_str())).await?;
    Ok(())
}

#[test]
fn test_parse_check_output() {
    let str1 = r#"xxxx\n time="2022-11-18T20:42:58+08:00" level=error msg="proxy 0: 'alpn' expected type 'string', got unconvertible type '[]interface {}'""#;
    let str2 = r#"20:43:49 ERR [Config] configuration file test failed error=proxy 0: unsupport proxy type: hysteria path=xxx"#;
    let str3 = r#"
    "time="2022-11-18T21:38:01+08:00" level=info msg="Start initial configuration in progress"
    time="2022-11-18T21:38:01+08:00" level=error msg="proxy 0: 'alpn' expected type 'string', got unconvertible type '[]interface {}'"
    configuration file xxx\n
    "#;

    let res1 = parse_check_output(str1.into());
    let res2 = parse_check_output(str2.into());
    let res3 = parse_check_output(str3.into());

    println!("res1: {res1}");
    println!("res2: {res2}");
    println!("res3: {res3}");

    assert_eq!(res1, res3);
}

#[test]
fn test_path() {
    let host = "http://127.0.0.1:9090";
    let path_with_prefix = "/configs";

    let base_url = Url::parse(host).context("failed to parse host").unwrap();
    let opts = url::Url::options().base_url(Some(&base_url));
    let url = opts
        .parse(path_with_prefix)
        .context("failed to parse path")
        .unwrap();
    assert_eq!(url.to_string(), "http://127.0.0.1:9090/configs");

    let path_without_prefix = "configs";
    let url = opts
        .parse(path_without_prefix)
        .context("failed to parse path")
        .unwrap();
    assert_eq!(url.to_string(), "http://127.0.0.1:9090/configs");
}
