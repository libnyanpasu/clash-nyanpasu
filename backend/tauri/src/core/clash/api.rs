use crate::config::Config;
use anyhow::{bail, Result};
use indexmap::IndexMap;
use reqwest::{header::HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use tracing_attributes::instrument;

/// PUT /configs
/// path 是绝对路径
#[instrument]
pub async fn put_configs(path: &str) -> Result<()> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/configs");

    let mut data = HashMap::new();
    data.insert("path", path);

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.put(&url).headers(headers).json(&data);
    let response = builder.send().await?.error_for_status()?;

    match response.status() {
        StatusCode::NO_CONTENT | StatusCode::ACCEPTED => Ok(()),
        _ => {
            bail!("failed to put configs")
        }
    }
}

/// PATCH /configs
#[instrument]
pub async fn patch_configs(config: &Mapping) -> Result<()> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/configs");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.patch(&url).headers(headers.clone()).json(config);
    builder.send().await?.error_for_status()?;
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
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/proxies");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.get(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<ProxiesRes>().await?)
}

/// GET /proxies/{name}
/// 获取单个代理
/// name: 代理名称
/// 返回代理的配置
///
#[allow(dead_code)]
#[instrument]
pub async fn get_proxy(name: String) -> Result<ProxyItem> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/proxies/{name}");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.get(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<ProxyItem>().await?)
}

/// PUT /proxies/{group}
/// 选择代理
/// group: 代理分组名称
/// name: 代理名称
#[instrument]
pub async fn update_proxy(group: &str, name: &str) -> Result<()> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/proxies/{group}");

    let mut data = HashMap::new();
    data.insert("name", name);

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.put(&url).headers(headers).json(&data);
    let response = builder.send().await?.error_for_status()?;

    match response.status() {
        StatusCode::ACCEPTED | StatusCode::NO_CONTENT => Ok(()),
        status => {
            bail!("failed to put proxy with status \"{status}\"")
        }
    }
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
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/providers/proxies");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.get(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<ProvidersProxiesRes>().await?)
}

/// GET /providers/proxies/:name
/// 获取单个代理集合的所有代理信息
/// group: 代理集合名称
#[allow(dead_code)]
#[instrument]
pub async fn get_providers_proxies_group(group: String) -> Result<ProxyProviderItem> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/providers/proxies/{group}");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.get(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<ProxyProviderItem>().await?)
}

/// PUT /providers/proxies/:name
/// 更新代理集合
/// name: 代理集合名称
#[instrument]
pub async fn update_providers_proxies_group(name: &str) -> Result<()> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/providers/proxies/{name}");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.put(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    match response.status() {
        StatusCode::NO_CONTENT | StatusCode::ACCEPTED => Ok(()),
        status => {
            bail!("failed to put providers proxies name with status \"{status}\"")
        }
    }
}

/// GET /providers/proxies/:name/healthcheck
/// 获取代理集合的健康检查
/// name: 代理集合名称
#[allow(dead_code)]
#[instrument]
pub async fn get_providers_proxies_healthcheck(name: String) -> Result<Mapping> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/providers/proxies/{name}/healthcheck");

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client.get(&url).headers(headers);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<Mapping>().await?)
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}

/// GET /proxies/{name}/delay
/// 获取代理延迟
#[instrument]
pub async fn get_proxy_delay(name: String, test_url: Option<String>) -> Result<DelayRes> {
    let (url, headers) = clash_client_info()?;
    let url = format!("{url}/proxies/{name}/delay");
    let default_url = "http://www.gstatic.com/generate_204";
    let test_url = test_url
        .map(|s| if s.is_empty() { default_url.into() } else { s })
        .unwrap_or(default_url.into());

    let client = reqwest::ClientBuilder::new().no_proxy().build()?;
    let builder = client
        .get(&url)
        .headers(headers)
        .query(&[("timeout", "10000"), ("url", &test_url)]);
    let response = builder.send().await?.error_for_status()?;

    Ok(response.json::<DelayRes>().await?)
}

/// 根据clash info获取clash服务地址和请求头
#[instrument]
fn clash_client_info() -> Result<(String, HeaderMap)> {
    let client = { Config::clash().data().get_client_info() };

    let server = format!("http://{}", client.server);

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse()?);

    if let Some(secret) = client.secret {
        let secret = format!("Bearer {}", secret).parse()?;
        headers.insert("Authorization", secret);
    }

    Ok((server, headers))
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
