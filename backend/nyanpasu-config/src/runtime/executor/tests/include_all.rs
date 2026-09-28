use serde_json::json;

use super::{orders::base_inputs, support::*};
use crate::runtime::{
    executor::{ExecutionTarget, StepLogLevel, execute, include_all::expand_include_all},
    snapshot::{BuiltinStepKind, OperatorTag},
    value::ConfigValue,
};

fn value(json: serde_json::Value) -> ConfigValue {
    ConfigValue::try_from(json).unwrap()
}

fn expand(json: serde_json::Value) -> (serde_json::Value, Vec<String>) {
    let (next, logs) = expand_include_all(&value(json));
    (
        next.to_json(),
        logs.into_iter().map(|entry| entry.message).collect(),
    )
}

fn base(groups: serde_json::Value) -> serde_json::Value {
    json!({
        "proxies": [ { "name": "b-hk" }, { "name": "a-us" }, { "name": "c-hk" } ],
        "proxy-providers": { "sub2": {}, "sub1": {} },
        "proxy-groups": groups,
        "rules": []
    })
}

#[test]
fn include_all_appends_sorted_proxies_after_explicit_ones_and_replaces_use() {
    let (result, logs) = expand(base(json!([{
        "name": "All",
        "type": "select",
        "include-all": true,
        "proxies": ["DIRECT", "Other"],
        "use": ["ignored"],
        "exclude-type": "direct",
    }])));

    assert!(logs.is_empty());
    assert_eq!(
        result["proxy-groups"][0],
        json!({
            "name": "All",
            "type": "select",
            "proxies": ["DIRECT", "Other", "a-us", "b-hk", "c-hk"],
            "use": ["sub1", "sub2"],
            "exclude-type": "direct",
        })
    );
    let keys: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["proxies", "proxy-providers", "proxy-groups", "rules"]
    );
}

#[test]
fn filter_selects_proxies_per_pattern_and_stays_on_the_group() {
    let (result, _) = expand(base(json!([{
        "name": "HK",
        "type": "url-test",
        "include-all-proxies": true,
        "filter": "(?i)HK`^c-",
    }])));

    // mihomo appends once per matching pattern: c-hk matches both.
    assert_eq!(
        result["proxy-groups"][0],
        json!({
            "name": "HK",
            "type": "url-test",
            "filter": "(?i)HK`^c-",
            "proxies": ["b-hk", "c-hk", "c-hk"],
        })
    );
}

#[test]
fn filter_supports_regexp2_lookaround() {
    let (result, _) = expand(base(json!([{
        "name": "NotUS",
        "type": "select",
        "include-all-proxies": true,
        "filter": "^(?!a-).*",
    }])));
    assert_eq!(
        result["proxy-groups"][0]["proxies"],
        json!(["b-hk", "c-hk"])
    );
}

#[test]
fn providers_only_keeps_explicit_proxies_untouched() {
    let (result, _) = expand(base(json!([{
        "name": "P",
        "type": "select",
        "include-all-providers": true,
        "proxies": ["DIRECT"],
    }])));
    assert_eq!(
        result["proxy-groups"][0],
        json!({ "name": "P", "type": "select", "proxies": ["DIRECT"], "use": ["sub1", "sub2"] })
    );
}

#[test]
fn empty_expansion_falls_back_like_mihomo() {
    let config = json!({
        "proxies": [ { "name": "a-us" } ],
        "proxy-groups": [
            { "name": "None", "type": "select", "include-all-proxies": true, "filter": "zz" },
            {
                "name": "Custom", "type": "select", "include-all-proxies": 1,
                "filter": "zz", "empty-fallback": "REJECT"
            }
        ]
    });
    let (result, _) = expand(config);
    assert_eq!(result["proxy-groups"][0]["proxies"], json!(["COMPATIBLE"]));
    assert_eq!(result["proxy-groups"][1]["proxies"], json!(["REJECT"]));
    assert!(
        result["proxy-groups"][1]
            .get("include-all-proxies")
            .is_none()
    );
}

#[test]
fn shapes_mihomo_rejects_are_left_for_the_core() {
    let groups = json!([
        { "name": "Bad", "type": "select", "include-all": true, "filter": "(" },
        { "name": "Str", "type": "select", "include-all": "true" },
        { "name": "Plain", "type": "select", "proxies": ["DIRECT"] }
    ]);
    let (result, logs) = expand(base(groups.clone()));
    assert_eq!(result["proxy-groups"], groups);
    assert_eq!(logs.len(), 1);
    assert!(logs[0].contains("`Bad`"));
}

#[test]
fn pipeline_records_the_step_only_when_enabled() {
    let profiles = profiles_with(
        Some("sub"),
        &[],
        &[],
        vec![config_file_item("sub", "sub.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub.yaml",
        "proxies:\n  - name: n1\nproxy-groups:\n  - name: G\n    type: select\n    include-all: true\n    filter: '('\n",
    )]);
    let ov = super::builtin::fixed_overrides();
    let is_expansion = |tag: &OperatorTag| {
        matches!(
            tag,
            OperatorTag::BuiltinStep {
                step: BuiltinStepKind::IncludeAllExpansion,
                ..
            }
        )
    };

    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub")), &ov, &[]);
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let node = artifact
        .graph
        .nodes
        .iter()
        .find(|node| is_expansion(&node.tag))
        .expect("expansion step recorded");
    let log = artifact
        .step_logs
        .iter()
        .find(|log| log.key == node.tag.node_key())
        .expect("invalid filter logged on the expansion step");
    assert_eq!(log.entries[0].level, StepLogLevel::Error);

    inputs.expand_include_all = false;
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    assert!(
        !artifact
            .graph
            .nodes
            .iter()
            .any(|node| is_expansion(&node.tag))
    );
    assert_eq!(
        artifact.final_config.to_json()["proxy-groups"][0]["include-all"],
        json!(true)
    );
}
