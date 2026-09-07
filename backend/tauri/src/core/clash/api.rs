use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt::{self, Display, Formatter};
use tracing_attributes::instrument;

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

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub enum VehicleType {
    File,
    #[serde(rename = "HTTP")]
    Http,
    Compatible,
    Inline,
    #[serde(untagged)]
    Unknown(String),
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub enum ProviderType {
    Proxy,
    Rule,
    #[serde(untagged)]
    Unknown(String),
}

impl Display for ProviderType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ProviderType::Proxy => write!(f, "Proxy"),
            ProviderType::Rule => write!(f, "Rule"),
            ProviderType::Unknown(value) => write!(f, "{value}"),
        }
    }
}

// Subscription usage returned inline by the Clash providers REST API.
// Relocated here (PR-3 T10) from the retired legacy profile module; it is a
// clash-API concern, not part of the profiles domain model.
#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    // Clash REST API returns PascalCase; profile YAML uses lowercase.
    // aliases accept both; default handles provider responses with partial fields.
    #[serde(alias = "Upload", default)]
    pub upload: usize,
    #[serde(alias = "Download", default)]
    pub download: usize,
    #[serde(alias = "Total", default)]
    pub total: usize,
    #[serde(alias = "Expire", default)]
    pub expire: usize,
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
    pub subscription_info: Option<SubscriptionInfo>,
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
    pub delay: u64,
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
#[allow(dead_code)]
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
        let info: SubscriptionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.upload, 100);
        assert_eq!(info.download, 200);
        assert_eq!(info.total, 1_073_741_824_000);
        assert_eq!(info.expire, 1_716_979_200);
    }

    #[test]
    fn subscription_info_deserializes_lowercase() {
        // Profile YAML uses lowercase field names; must still work
        let json = r#"{"upload":10,"download":20,"total":30,"expire":0}"#;
        let info: SubscriptionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.upload, 10);
        assert_eq!(info.download, 20);
    }

    #[test]
    fn subscription_info_deserializes_partial_fields() {
        // Some providers return only partial subscription info (e.g. only Expire)
        let json = r#"{"Expire":1716979200}"#;
        let info: SubscriptionInfo = serde_json::from_str(json).unwrap();
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
