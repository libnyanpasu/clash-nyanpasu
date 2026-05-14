use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt::{self, Display, Formatter};

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
    pub r#type: String, // TODO: convert to enum
    pub udp: bool,
    pub history: Vec<ProxyItemHistory>,
    pub all: Option<Vec<String>>,
    pub now: Option<String>,
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

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}
