use serde_json::json;

use crate::{
    clash::config::{overrides::ClashGuardOverrides, tun_stack::TunStack},
    runtime::{
        executor::{
            GuardInputs, ResolvedPortBindings, TunFlavor, TunParams,
            builtin::{apply_guard, finalize, known_fields, stage1_fields, whitelist_filter},
        },
        value::ConfigValue,
    },
};

fn value(json: serde_json::Value) -> ConfigValue {
    ConfigValue::try_from(json).unwrap()
}

/// Fixed-secret guard input; the default constructor generates a fresh uuid.
pub fn fixed_overrides() -> ClashGuardOverrides {
    serde_yaml_ng::from_str(
        "log-level: info\nallow-lan: false\nmode: rule\nsecret: golden-secret\nunified-delay: true\ntcp-concurrent: true\nipv6: false\n",
    )
    .unwrap()
}

fn tun_off() -> TunParams {
    TunParams {
        enable: false,
        flavor: TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
        windows_fake_ip_filter: false,
    }
}

#[test]
fn known_fields_are_exactly_45() {
    assert_eq!(known_fields().count(), 45);
    assert!(known_fields().any(|f| f == "proxies"));
    assert!(known_fields().any(|f| f == "external-controller"));
    assert!(known_fields().any(|f| f == "global-client-fingerprint"));
}

#[test]
fn known_fields_keep_default_handle_others_order() {
    let fields: Vec<&str> = known_fields().collect();
    assert_eq!(
        &fields[..5],
        &[
            "proxies",
            "proxy-groups",
            "proxy-providers",
            "rules",
            "rule-providers",
        ]
    );
    assert_eq!(
        &fields[5..14],
        &[
            "mode",
            "port",
            "socks-port",
            "mixed-port",
            "allow-lan",
            "log-level",
            "ipv6",
            "secret",
            "external-controller",
        ]
    );
    assert_eq!(fields.last(), Some(&"global-client-fingerprint"));
}

#[test]
fn stage1_fields_take_valid_intersect_others_plus_default_and_never_handle() {
    let fields = stage1_fields(&[
        "DNS".to_string(),
        "not-a-clash-field".to_string(),
        "mode".to_string(),
    ]);
    assert!(fields.contains(&"dns".to_string()));
    assert!(fields.contains(&"proxies".to_string()));
    assert!(!fields.contains(&"not-a-clash-field".to_string()));
    assert!(!fields.contains(&"mode".to_string()));
}

#[test]
fn whitelist_filter_keeps_only_allowed_and_is_noop_when_disabled() {
    let config = value(json!({ "proxies": [], "mode": "rule", "custom": 1 }));
    let allow = vec!["proxies".to_string()];
    assert_eq!(
        whitelist_filter(&config, &allow, true).to_json(),
        json!({ "proxies": [] })
    );
    assert_eq!(whitelist_filter(&config, &allow, false), config);
}

#[test]
fn guard_inserts_override_keys_and_resolved_ports() {
    let overrides = fixed_overrides();
    let guard = GuardInputs {
        overrides: &overrides,
        ports: ResolvedPortBindings {
            mixed_port: 7890,
            port: None,
            socks_port: Some(7891),
            external_controller: Some("127.0.0.1:9090".to_string()),
        },
    };
    let result = apply_guard(
        &value(json!({ "mode": "from-profile", "rules": [] })),
        &guard,
    )
    .unwrap()
    .to_json();

    assert_eq!(result["mode"], json!("rule"));
    assert_eq!(result["log-level"], json!("info"));
    assert_eq!(result["allow-lan"], json!(false));
    assert_eq!(result["ipv6"], json!(false));
    assert_eq!(result["secret"], json!("golden-secret"));
    assert_eq!(result["unified-delay"], json!(true));
    assert_eq!(result["tcp-concurrent"], json!(true));
    assert_eq!(result["mixed-port"], json!(7890));
    assert_eq!(result["socks-port"], json!(7891));
    assert_eq!(result["external-controller"], json!("127.0.0.1:9090"));
    assert!(result.get("port").is_none());
    assert_eq!(result["rules"], json!([]));
}

#[test]
fn tun_disabled_without_tun_key_is_untouched() {
    let config = value(json!({ "proxies": [] }));
    let result = finalize(&config, &tun_off(), false).to_json();
    assert!(result.get("tun").is_none());
}

#[test]
fn tun_disabled_with_existing_tun_key_forces_enable_false() {
    let config = value(json!({ "tun": { "enable": true, "stack": "system" } }));
    let result = finalize(&config, &tun_off(), false).to_json();
    assert_eq!(result["tun"]["enable"], json!(false));
    assert_eq!(result["tun"]["stack"], json!("system"));
    assert!(result.get("dns").is_none());
}

#[test]
fn tun_enabled_standard_appends_defaults_and_dns() {
    let params = TunParams {
        enable: true,
        flavor: TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
        windows_fake_ip_filter: true,
    };
    let config =
        value(json!({ "tun": { "stack": "system" }, "dns": { "nameserver": ["1.1.1.1"] } }));
    let result = finalize(&config, &params, false).to_json();

    assert_eq!(result["tun"]["enable"], json!(true));
    assert_eq!(result["tun"]["stack"], json!("system"));
    assert_eq!(result["tun"]["dns-hijack"], json!(["any:53"]));
    assert_eq!(result["tun"]["auto-route"], json!(true));
    assert_eq!(result["tun"]["auto-detect-interface"], json!(true));
    assert_eq!(result["dns"]["enable"], json!(true));
    assert_eq!(result["dns"]["nameserver"], json!(["1.1.1.1"]));
    assert_eq!(result["dns"]["enhanced-mode"], json!("fake-ip"));
    assert_eq!(result["dns"]["fake-ip-range"], json!("198.18.0.1/16"));
    assert_eq!(result["dns"]["fallback"], json!([]));
    assert_eq!(
        result["dns"]["fake-ip-filter"],
        json!([
            "dns.msftncsi.com",
            "www.msftncsi.com",
            "www.msftconnecttest.com"
        ])
    );
}

#[test]
fn tun_enabled_clash_rs_uses_device_branch_and_no_windows_filter() {
    let params = TunParams {
        enable: true,
        flavor: TunFlavor::ClashRs,
        windows_fake_ip_filter: false,
    };
    let result = finalize(&value(json!({})), &params, false).to_json();
    assert_eq!(result["tun"]["enable"], json!(true));
    assert_eq!(result["tun"]["device-id"], json!("dev://utun1989"));
    assert_eq!(result["tun"]["auto-route"], json!(true));
    assert!(result["tun"].get("stack").is_none());
    assert!(result["tun"].get("dns-hijack").is_none());
    assert!(result["dns"].get("fake-ip-filter").is_none());
}

#[test]
fn finalize_applies_include_all_cache_sort_and_stage2_filter() {
    let config = value(json!({
        "custom-unknown": 1,
        "proxies": [ { "name": "Proxy1" }, { "name": "Proxy2" } ],
        "proxy-providers": { "provider1": {} },
        "proxy-groups": [
            { "name": "GLOBAL", "type": "select", "include-all": true, "proxies": ["DIRECT"] },
            { "name": "Plain", "type": "select", "proxies": ["DIRECT"] }
        ],
        "mode": "rule"
    }));

    let result = finalize(&config, &tun_off(), true).to_json();
    assert!(result.get("custom-unknown").is_none());
    let groups = result["proxy-groups"].as_array().unwrap();
    let global = groups.iter().find(|g| g["name"] == "GLOBAL").unwrap();
    assert!(global.get("include-all").is_none());
    let proxies: Vec<&str> = global["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(proxies, vec!["Proxy1", "Proxy2", "provider1", "DIRECT"]);
    assert_eq!(
        result["profile"],
        json!({ "store-selected": true, "store-fake-ip": false })
    );
    let keys: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let pos = |k: &str| keys.iter().position(|x| *x == k).unwrap();
    assert!(pos("mode") < pos("profile"));
    assert!(pos("profile") < pos("proxies"));

    let kept = finalize(&config, &tun_off(), false).to_json();
    assert_eq!(kept["custom-unknown"], json!(1));
    let keys: Vec<&str> = kept
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(*keys.last().unwrap(), "custom-unknown");
}

#[test]
fn cache_does_not_merge_into_existing_profile_key() {
    let config = value(json!({ "profile": { "do-not-override": true } }));
    let result = finalize(&config, &tun_off(), false).to_json();
    assert_eq!(result["profile"], json!({ "do-not-override": true }));
}
