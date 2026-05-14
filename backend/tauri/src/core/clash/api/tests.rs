use super::*;
use anyhow::Context;
use url::Url;

#[test]
fn subscription_info_deserializes_pascal_case() {
    // Mihomo REST API returns PascalCase field names
    let json = r#"{"Upload":100,"Download":200,"Total":1073741824000,"Expire":1716979200}"#;
    let info: crate::config::profile::item::SubscriptionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.upload, 100);
    assert_eq!(info.download, 200);
    assert_eq!(info.total, 1_073_741_824_000);
    assert_eq!(info.expire, 1_716_979_200);
}

#[test]
fn subscription_info_deserializes_lowercase() {
    // Profile YAML uses lowercase field names; must still work
    let json = r#"{"upload":10,"download":20,"total":30,"expire":0}"#;
    let info: crate::config::profile::item::SubscriptionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.upload, 10);
    assert_eq!(info.download, 20);
}

#[test]
fn subscription_info_deserializes_partial_fields() {
    // Some providers return only partial subscription info (e.g. only Expire)
    let json = r#"{"Expire":1716979200}"#;
    let info: crate::config::profile::item::SubscriptionInfo = serde_json::from_str(json).unwrap();
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
