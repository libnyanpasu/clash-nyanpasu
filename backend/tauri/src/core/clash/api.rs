use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use tracing_attributes::instrument;

// The container rename only names the TS export (JSON ignores struct names):
// the typed persistent `ClashConfig` owns the plain name.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
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
