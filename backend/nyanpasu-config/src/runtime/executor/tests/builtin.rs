use serde_json::json;

use crate::{
    clash::config::{
        overrides::{ClashGuardOverrides, ManageableField, TunOverrides},
        tun_stack::TunStack,
    },
    runtime::{
        executor::{
            GuardInputs, ResolvedPortBindings, TunFlavor, TunParams,
            builtin::{filter_fields, known_fields, stage1_fields},
            plan::{cache_ownership, guard_ownership, tun_ownership},
        },
        post_process::{apply_defaults, apply_settings},
        value::ConfigValue,
    },
};

fn value(json: serde_json::Value) -> ConfigValue {
    ConfigValue::try_from(json).unwrap()
}

/// The guard's keys, written as the managed settings step does.
fn apply_guard(config: &ConfigValue, guard: &GuardInputs<'_>) -> ConfigValue {
    apply_settings(config, &guard_ownership(guard)).0
}

/// `tun`, its DNS takeover and the `profile` cache, written as the managed
/// settings and defaults steps do.
fn apply_tun_and_cache(config: &ConfigValue, tun: &TunParams) -> ConfigValue {
    let mut ownership = tun_ownership(tun);
    ownership.push(cache_ownership());
    let config = apply_settings(config, &ownership).0;
    apply_defaults(&config, &ownership).0
}

/// Fixed-secret guard input; the default constructor generates a fresh uuid.
pub fn fixed_overrides() -> ClashGuardOverrides {
    serde_yaml_ng::from_str(
        "log-level: info\nallow-lan: false\nmode: rule\nsecret: golden-secret\nunified-delay: {kind: managed, value: true}\ntcp-concurrent: {kind: managed, value: true}\nipv6: false\n",
    )
    .unwrap()
}

/// [`fixed_overrides`] with `unified-delay` and `tcp-concurrent` left to the
/// profiles.
pub fn unmanaged_overrides() -> ClashGuardOverrides {
    serde_yaml_ng::from_str(
        "log-level: info\nallow-lan: false\nmode: rule\nsecret: golden-secret\nunified-delay: {kind: unmanaged}\ntcp-concurrent: {kind: unmanaged}\nipv6: false\n",
    )
    .unwrap()
}

fn auto_route(auto_route: ManageableField<bool>) -> TunOverrides {
    TunOverrides { auto_route }
}

fn tun_off() -> TunParams {
    TunParams {
        enable: false,
        overrides: TunOverrides::default(),
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
fn filter_fields_keeps_only_allowed_and_records_the_removals() {
    let config = value(json!({ "proxies": [], "mode": "rule", "custom": 1 }));
    let allow = vec!["proxies".to_string()];

    let (filtered, patch) = filter_fields(&config, &allow);

    assert_eq!(filtered.to_json(), json!({ "proxies": [] }));
    assert_eq!(
        serde_json::to_value(&patch).unwrap(),
        json!([{ "op": "remove", "path": "/mode" }, { "op": "remove", "path": "/custom" }])
    );
}

#[test]
fn filter_preserves_retained_field_and_nested_order() {
    let config = value(json!({
        "mode": "rule", "proxies": [{"name": "a", "type": "direct"}],
        "custom": true, "rules": ["MATCH,DIRECT"]
    }));
    let (filtered, _) = filter_fields(&config, &stage1_fields(&[]));
    assert_eq!(
        serde_json::to_string(&filtered.to_json()).unwrap(),
        r#"{"proxies":[{"name":"a","type":"direct"}],"rules":["MATCH,DIRECT"]}"#,
    );
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

/// An unset `port`/`socks-port` removes the profile's value; a configured one
/// replaces it, the same ownership as `mixed-port`.
#[test]
fn guard_owns_proxy_ports_like_mixed_port() {
    let overrides = fixed_overrides();
    let profile = value(json!({ "port": 7890, "socks-port": 7891, "mixed-port": 1 }));

    let unset = apply_guard(&profile, &guard_inputs(&overrides)).to_json();
    assert!(unset.get("port").is_none());
    assert!(unset.get("socks-port").is_none());
    assert_eq!(unset["mixed-port"], json!(7890));

    let mut configured = guard_inputs(&overrides);
    configured.ports.port = Some(7892);
    configured.ports.socks_port = Some(7893);
    let configured = apply_guard(&profile, &configured).to_json();
    assert_eq!(configured["port"], json!(7892));
    assert_eq!(configured["socks-port"], json!(7893));
}

fn guard_inputs(overrides: &ClashGuardOverrides) -> GuardInputs<'_> {
    GuardInputs {
        overrides,
        ports: ResolvedPortBindings {
            mixed_port: 7890,
            port: None,
            socks_port: None,
            external_controller: None,
        },
    }
}

/// A managed field overrides the profile's value.
#[test]
fn guard_overwrites_managed_fields() {
    let overrides = fixed_overrides();
    let result = apply_guard(
        &value(json!({ "tcp-concurrent": false, "unified-delay": false })),
        &guard_inputs(&overrides),
    )
    .to_json();

    assert_eq!(result["tcp-concurrent"], json!(true));
    assert_eq!(result["unified-delay"], json!(true));
}

/// An unmanaged field keeps the profile's value and is not added when the
/// profile has none, while the other guard fields stay forced.
#[test]
fn guard_leaves_unmanaged_fields_to_the_profile() {
    let overrides = unmanaged_overrides();
    let result = apply_guard(
        &value(json!({ "tcp-concurrent": false, "mode": "from-profile" })),
        &guard_inputs(&overrides),
    )
    .to_json();

    assert_eq!(result["tcp-concurrent"], json!(false));
    assert!(result.get("unified-delay").is_none());
    assert_eq!(result["mode"], json!("rule"));
    assert_eq!(result["secret"], json!("golden-secret"));
    assert_eq!(result["mixed-port"], json!(7890));
}

#[test]
fn tun_disabled_without_tun_key_is_untouched() {
    let config = value(json!({ "proxies": [] }));
    let result = apply_tun_and_cache(&config, &tun_off()).to_json();
    assert!(result.get("tun").is_none());
}

#[test]
fn tun_disabled_with_existing_tun_key_forces_enable_false() {
    let config = value(json!({ "tun": { "enable": true, "stack": "system" } }));
    let result = apply_tun_and_cache(&config, &tun_off()).to_json();
    assert_eq!(result["tun"]["enable"], json!(false));
    assert_eq!(result["tun"]["stack"], json!("system"));
    assert!(result.get("dns").is_none());
}

#[test]
fn tun_enabled_standard_appends_defaults_and_dns() {
    let params = TunParams {
        enable: true,
        overrides: TunOverrides::default(),
        flavor: TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
        windows_fake_ip_filter: true,
    };
    let config =
        value(json!({ "tun": { "stack": "system" }, "dns": { "nameserver": ["1.1.1.1"] } }));
    let result = apply_tun_and_cache(&config, &params).to_json();

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
        overrides: TunOverrides::default(),
        flavor: TunFlavor::ClashRs,
        windows_fake_ip_filter: false,
    };
    let result = apply_tun_and_cache(&value(json!({})), &params).to_json();
    assert_eq!(result["tun"]["enable"], json!(true));
    assert_eq!(result["tun"]["device-id"], json!("dev://utun1989"));
    assert_eq!(result["tun"]["auto-route"], json!(true));
    assert!(result["tun"].get("stack").is_none());
    assert!(result["tun"].get("dns-hijack").is_none());
    assert!(result["dns"].get("fake-ip-filter").is_none());
}

#[test]
fn managed_auto_route_overwrites_profile_values_for_both_tun_flavors() {
    for flavor in [
        TunFlavor::ClashRs,
        TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
    ] {
        for enabled in [false, true] {
            let params = TunParams {
                enable: true,
                overrides: auto_route(ManageableField::Managed(enabled)),
                flavor,
                ..tun_off()
            };
            let config = value(json!({ "tun": { "auto-route": "global" } }));
            let result = apply_tun_and_cache(&config, &params).to_json();
            assert_eq!(result["tun"]["auto-route"], json!(enabled));
            assert_eq!(result["tun"]["enable"], json!(true));
        }
    }
}

#[test]
fn unmanaged_auto_route_preserves_profile_values_and_other_tun_defaults() {
    for flavor in [
        TunFlavor::ClashRs,
        TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
    ] {
        for route in [json!("global"), json!(false), json!(true)] {
            let params = TunParams {
                enable: true,
                overrides: auto_route(ManageableField::Unmanaged),
                flavor,
                ..tun_off()
            };
            let config = value(json!({ "tun": {
                "enable": false,
                "auto-route": route,
                "mtu": 1400,
            } }));
            let result = apply_tun_and_cache(&config, &params).to_json();
            assert_eq!(result["tun"]["auto-route"], route);
            assert_eq!(result["tun"]["enable"], json!(true));
            assert_eq!(result["tun"]["mtu"], json!(1400));
            assert_eq!(result["dns"]["enable"], json!(true));
            match flavor {
                TunFlavor::ClashRs => {
                    assert_eq!(result["tun"]["device-id"], json!("dev://utun1989"))
                }
                TunFlavor::Standard { .. } => {
                    assert_eq!(result["tun"]["stack"], json!("gvisor"));
                    assert_eq!(result["tun"]["dns-hijack"], json!(["any:53"]));
                    assert_eq!(result["tun"]["auto-detect-interface"], json!(true));
                }
            }
        }
    }
}

/// Left unmanaged, a missing `auto-route` stays missing, as for an unmanaged
/// guard field.
#[test]
fn unmanaged_auto_route_does_not_supply_a_default() {
    for flavor in [
        TunFlavor::ClashRs,
        TunFlavor::Standard {
            stack: TunStack::Gvisor,
        },
    ] {
        let params = TunParams {
            enable: true,
            overrides: auto_route(ManageableField::Unmanaged),
            flavor,
            ..tun_off()
        };
        let result = apply_tun_and_cache(&value(json!({})), &params).to_json();
        assert!(result["tun"].get("auto-route").is_none());
        assert_eq!(result["tun"]["enable"], json!(true));
        assert_eq!(result["dns"]["enable"], json!(true));
    }
}

/// With TUN off the forced `tun` keys are not written either.
#[test]
fn managed_auto_route_is_not_forced_while_tun_is_off() {
    let params = TunParams {
        overrides: auto_route(ManageableField::Managed(false)),
        ..tun_off()
    };
    let config = value(json!({ "tun": { "enable": true, "auto-route": "global" } }));
    let result = apply_tun_and_cache(&config, &params).to_json();
    assert_eq!(
        result["tun"],
        json!({ "enable": false, "auto-route": "global" })
    );

    let absent = apply_tun_and_cache(&value(json!({})), &params).to_json();
    assert!(absent.get("tun").is_none());
}

#[test]
fn unmanaged_auto_route_does_not_let_the_profile_enable_tun() {
    let params = TunParams {
        overrides: auto_route(ManageableField::Unmanaged),
        ..tun_off()
    };
    let config = value(json!({ "tun": { "enable": true, "auto-route": "global" } }));
    let result = apply_tun_and_cache(&config, &params).to_json();
    assert_eq!(result["tun"]["enable"], json!(false));
    assert_eq!(result["tun"]["auto-route"], json!("global"));
    assert!(result.get("dns").is_none());

    let absent = apply_tun_and_cache(&value(json!({})), &params).to_json();
    assert!(absent.get("tun").is_none());
    assert!(absent.get("dns").is_none());
}

#[test]
fn cache_is_appended_without_reordering() {
    let config = value(json!({
        "custom-unknown": 1,
        "proxies": [ { "name": "Proxy1" } ],
        "proxy-groups": [
            { "name": "GLOBAL", "type": "select", "include-all": true, "proxies": ["DIRECT"] }
        ],
        "mode": "rule"
    }));

    let result = apply_tun_and_cache(&config, &tun_off()).to_json();
    // include-all expansion is its own step; these steps leave groups alone.
    assert_eq!(result["proxy-groups"], config.to_json()["proxy-groups"]);
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
    assert_eq!(
        keys,
        vec![
            "custom-unknown",
            "proxies",
            "proxy-groups",
            "mode",
            "profile"
        ]
    );
}

#[test]
fn cache_does_not_merge_into_existing_profile_key() {
    let config = value(json!({ "profile": { "do-not-override": true } }));
    let result = apply_tun_and_cache(&config, &tun_off()).to_json();
    assert_eq!(result["profile"], json!({ "do-not-override": true }));
}

#[test]
fn filtering_large_config_does_not_create_spurious_block_moves() {
    use crate::runtime::{
        snapshot::{BuiltinStepKind, ConfigSnapshotsBuilder, OperatorTag},
        value::ConfigValue,
    };
    use std::sync::Arc;
    let config = Arc::new(
        ConfigValue::try_from(serde_json::json!({
            "mode": "rule",
            "proxies": (0..5000).map(|i| format!("proxy{i}")).collect::<Vec<_>>(),
            "rules": (0..5000).map(|i| format!("DOMAIN,host{i},DIRECT")).collect::<Vec<_>>(),
        }))
        .unwrap(),
    );
    let filtered = Arc::new(filter_fields(&config, &stage1_fields(&[])).0);
    let mut builder = ConfigSnapshotsBuilder::new_root(config, OperatorTag::BareRoot);
    builder
        .push(
            OperatorTag::BuiltinStep {
                selected_profile_id: None,
                step: BuiltinStepKind::FieldFilter,
            },
            filtered,
        )
        .unwrap();
    let graph = builder.build().unwrap();
    let after = serde_yaml_ng::to_string(&graph.nodes[1].snapshot.config).unwrap();
    let hunks = graph.nodes[0].snapshot.diff_yaml_to(&after).unwrap();
    let changes: Vec<_> = hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| !l.starts_with(' '))
        .map(String::as_str)
        .collect();
    assert_eq!(changes, ["-mode: rule"]);
}
