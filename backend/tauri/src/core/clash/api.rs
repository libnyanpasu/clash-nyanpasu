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

#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
pub struct ClashConfig {
    pub port: Option<u16>,
    pub mode: Option<String>,
    pub ipv6: Option<bool>,
    #[serde(rename = "socket-port")]
    pub socket_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ClashVersion {
    pub version: String,
    pub premium: Option<bool>,
    pub meta: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ClashRule {
    pub r#type: String,
    pub payload: String,
    pub proxy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct RulesRes {
    pub rules: Vec<ClashRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct RuleProviderItem {
    pub behavior: Option<String>,
    pub format: Option<String>,
    pub name: String,
    #[serde(rename = "ruleCount")]
    pub rule_count: Option<u32>,
    pub r#type: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "vehicleType")]
    pub vehicle_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ProvidersRulesRes {
    pub providers: IndexMap<String, RuleProviderItem>,
}

/// GET /configs
#[instrument]
pub async fn get_configs() -> Result<ClashConfig> {
    let path = "/configs";
    let resp: ClashConfig = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// GET /version
#[instrument]
pub async fn get_version() -> Result<ClashVersion> {
    let path = "/version";
    let resp: ClashVersion = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// GET /rules
#[instrument]
pub async fn get_rules() -> Result<RulesRes> {
    let path = "/rules";
    let resp: RulesRes = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// GET /providers/rules
#[instrument]
pub async fn get_providers_rules() -> Result<ProvidersRulesRes> {
    let path = "/providers/rules";
    let resp: ProvidersRulesRes = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// PUT /providers/rules/:name
#[instrument]
pub async fn update_providers_rules_group(name: &str) -> Result<()> {
    let path = format!("/providers/rules/{name}");
    let _ = perform_request((Method::PUT, path.as_str())).await?;
    Ok(())
}

/// GET /group/:name/delay
#[instrument]
pub async fn get_group_delay(group: String, url: Option<String>) -> Result<HashMap<String, u32>> {
    let path = format!("/group/{group}/delay");
    let default_url = "http://www.gstatic.com/generate_204";
    let test_url = url
        .map(|s| if s.is_empty() { default_url.into() } else { s })
        .unwrap_or(default_url.into());

    let query = Query([("timeout", "10000"), ("url", &test_url)]);
    let resp: HashMap<String, u32> = perform_request((Method::GET, path.as_str(), query))
        .await?
        .json()
        .await?;
    Ok(resp)
}

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
            updated_at: _,
            subscription_info: _,
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
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_info: Option<crate::config::profile::item::SubscriptionInfo>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_info_deserializes_pascal_case() {
        // Mihomo REST API returns PascalCase field names
        let json = r#"{"Upload":100,"Download":200,"Total":1073741824000,"Expire":1716979200}"#;
        let info: crate::config::profile::item::SubscriptionInfo =
            serde_json::from_str(json).unwrap();
        assert_eq!(info.upload, 100);
        assert_eq!(info.download, 200);
        assert_eq!(info.total, 1_073_741_824_000);
        assert_eq!(info.expire, 1_716_979_200);
    }

    #[test]
    fn subscription_info_deserializes_lowercase() {
        // Profile YAML uses lowercase field names; must still work
        let json = r#"{"upload":10,"download":20,"total":30,"expire":0}"#;
        let info: crate::config::profile::item::SubscriptionInfo =
            serde_json::from_str(json).unwrap();
        assert_eq!(info.upload, 10);
        assert_eq!(info.download, 20);
    }

    #[test]
    fn subscription_info_deserializes_partial_fields() {
        // Some providers return only partial subscription info (e.g. only Expire)
        let json = r#"{"Expire":1716979200}"#;
        let info: crate::config::profile::item::SubscriptionInfo =
            serde_json::from_str(json).unwrap();
        assert_eq!(info.upload, 0);
        assert_eq!(info.expire, 1_716_979_200);
    }

    #[test]
    fn providers_proxies_res_deserializes_without_subscription_info() {
        let json = r#"{
            "providers": {
                "MyProvider": {
                    "name": "MyProvider",
                    "type": "Proxy",
                    "proxies": [],
                    "vehicleType": "HTTP"
                }
            }
        }"#;
        let res: ProvidersProxiesRes = serde_json::from_str(json).unwrap();
        let provider = res.providers.get("MyProvider").unwrap();
        assert!(provider.subscription_info.is_none());
    }

    #[test]
    fn providers_proxies_res_deserializes_with_pascal_subscription_info() {
        // Reproduces the original crash: Mihomo returns PascalCase SubscriptionInfo
        let json = r#"{
            "providers": {
                "MyProvider": {
                    "name": "MyProvider",
                    "type": "Proxy",
                    "proxies": [],
                    "vehicleType": "HTTP",
                    "subscriptionInfo": {
                        "Upload": 100000,
                        "Download": 200000,
                        "Total": 1073741824000,
                        "Expire": 1716979200
                    }
                }
            }
        }"#;
        let res: ProvidersProxiesRes = serde_json::from_str(json).unwrap();
        let info = res
            .providers
            .get("MyProvider")
            .unwrap()
            .subscription_info
            .as_ref()
            .unwrap();
        assert_eq!(info.upload, 100_000);
        assert_eq!(info.expire, 1_716_979_200);
    }

    #[test]
    fn providers_proxies_res_deserializes_with_partial_subscription_info() {
        // Some providers may return subscriptionInfo with only some fields set
        let json = r#"{
            "providers": {
                "P": {
                    "name": "P",
                    "type": "Proxy",
                    "proxies": [],
                    "vehicleType": "File",
                    "subscriptionInfo": {"Expire": 9999}
                }
            }
        }"#;
        let res: ProvidersProxiesRes = serde_json::from_str(json).unwrap();
        let info = res
            .providers
            .get("P")
            .unwrap()
            .subscription_info
            .as_ref()
            .unwrap();
        assert_eq!(info.upload, 0);
        assert_eq!(info.expire, 9999);
    }

    #[test]
    fn clash_config_deserializes_partial_fields() {
        // Not all cores return all config fields; all must be optional
        let json = r#"{"mode":"rule","mixed-port":7890}"#;
        let cfg: ClashConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode.as_deref(), Some("rule"));
        assert_eq!(cfg.mixed_port, Some(7890));
        assert!(cfg.port.is_none());
        assert!(cfg.allow_lan.is_none());
    }

    #[test]
    fn clash_version_deserializes_without_premium_meta() {
        // clash-rs returns only version
        let json = r#"{"version":"2025.01.01"}"#;
        let v: ClashVersion = serde_json::from_str(json).unwrap();
        assert!(v.premium.is_none());
        assert!(v.meta.is_none());
    }

    #[test]
    fn clash_version_deserializes_meta() {
        let json = r#"{"version":"1.18.0","meta":true}"#;
        let v: ClashVersion = serde_json::from_str(json).unwrap();
        assert_eq!(v.meta, Some(true));
        assert!(v.premium.is_none());
    }

    #[test]
    fn rule_provider_item_deserializes_all_optional_fields_absent() {
        // clash-rs may return minimal provider info
        let json = r#"{"name":"GeoIP"}"#;
        let item: RuleProviderItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.name, "GeoIP");
        assert!(item.rule_count.is_none());
        assert!(item.vehicle_type.is_none());
    }

    #[test]
    fn rule_provider_item_deserializes_full_mihomo_response() {
        let json = r#"{
            "behavior": "ipcidr",
            "format": "mrs",
            "name": "GeoIP",
            "ruleCount": 17523,
            "type": "Rule",
            "updatedAt": "2025-01-01T00:00:00Z",
            "vehicleType": "HTTP"
        }"#;
        let item: RuleProviderItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.name, "GeoIP");
        assert_eq!(item.rule_count, Some(17523));
        assert_eq!(item.vehicle_type.as_deref(), Some("HTTP"));
    }
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
