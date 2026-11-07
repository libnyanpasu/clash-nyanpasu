//! Transport values for clash; application behavior remains in its owner.

use clash_api::{ProviderName, ProviderType, Proxy, ProxyName, SubscriptionInfo, VehicleType};
use indexmap::IndexMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
#[serde(rename = "ClashApiConfig")]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ClashVersion {
    pub version: String,
    pub premium: Option<bool>,
    pub meta: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ClashRule {
    pub r#type: String,
    pub payload: String,
    pub proxy: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RulesRes {
    pub rules: Vec<ClashRule>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProvidersRulesRes {
    pub providers: IndexMap<String, RuleProviderItem>,
}

/// Provider metadata the application keeps after assembling its node view.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProviderSummary {
    pub name: ProviderName,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub vehicle_type: VehicleType,
    pub updated_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub subscription_info: Option<SubscriptionInfo>,
    pub proxy_count: usize,
}

/// A group's `type`, as the core reports it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
pub enum ProxyGroupKind {
    Selector,
    #[serde(rename = "URLTest")]
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Smart,
    /// A type this build does not know, kept verbatim.
    #[serde(untagged)]
    Unknown(String),
}

/// What the running core lets a user do with a group.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, Default, PartialEq, Eq,
)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupCapabilities {
    /// `PUT /proxies/{group}` chooses a member.
    pub select: bool,
    /// `DELETE /proxies/{group}` returns a pinned group to automatic selection.
    pub clear_fixed: bool,
}

/// A group's meaning, derived from its record in `Proxies::nodes`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: ProxyName,
    #[serde(rename = "type")]
    pub kind: ProxyGroupKind,
    /// Member names; look each node up in `Proxies::nodes`.
    pub all: Vec<ProxyName>,
    pub now: Option<ProxyName>,
    /// The member a user pinned; `None` while the core selects on its own.
    pub fixed: Option<ProxyName>,
    /// The URL the core tests this group's members with; `None` when unset.
    pub test_url: Option<String>,
    /// The status codes a test must return, in the core's range syntax.
    pub expected_status: Option<String>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub capabilities: ProxyGroupCapabilities,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    /// The core's GLOBAL group; `None` when the core has none.
    pub global: Option<ProxyGroup>,
    pub groups: Vec<ProxyGroup>,
    /// Every `/proxies` entry plus every provider-owned node referenced by a
    /// group, keyed by name. A node that belongs to several groups still has
    /// exactly one entry here; groups reference it by name in `all`.
    pub nodes: IndexMap<ProxyName, Proxy>,
}
