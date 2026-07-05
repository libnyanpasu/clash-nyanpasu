use serde_json::json;

use super::support::pid;
use crate::runtime::{
    executor::{StepLogLevel, compose::append_proxies},
    value::ConfigValue,
};

fn value(json: serde_json::Value) -> ConfigValue {
    ConfigValue::try_from(json).unwrap()
}

#[test]
fn append_extracts_only_proxies_in_order_without_dedup() {
    // 规则 5/7/8/9/10：按序追加、只取 proxies、不并其他字段、不去重。
    let mut logs = Vec::new();
    let working = value(json!({ "proxies": [{ "name": "a" }], "rules": ["r1"] }));
    let member = value(json!({ "proxies": [{ "name": "a" }, { "name": "b" }], "rules": ["r2"] }));
    let result = append_proxies(&working, &member, &pid("m"), &mut logs).to_json();
    assert_eq!(
        result,
        json!({ "proxies": [{ "name": "a" }, { "name": "a" }, { "name": "b" }], "rules": ["r1"] })
    );
    assert!(logs.is_empty());
}

#[test]
fn member_without_proxies_warns_and_contributes_nothing() {
    // D11（旧 merge_profiles 此处 panic，差异 #3）。
    let mut logs = Vec::new();
    let working = value(json!({ "proxies": [] }));
    let member = value(json!({ "rules": [] }));
    let result = append_proxies(&working, &member, &pid("m"), &mut logs).to_json();
    assert_eq!(result, json!({ "proxies": [] }));
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, StepLogLevel::Warn);
}

#[test]
fn working_without_proxies_creates_list_then_appends() {
    let mut logs = Vec::new();
    let working = value(json!({ "rules": [] }));
    let member = value(json!({ "proxies": [{ "name": "m" }] }));
    let result = append_proxies(&working, &member, &pid("m"), &mut logs).to_json();
    assert_eq!(result, json!({ "rules": [], "proxies": [{ "name": "m" }] }));
}

#[test]
fn non_sequence_working_proxies_warns_and_skips() {
    let mut logs = Vec::new();
    let working = value(json!({ "proxies": 3 }));
    let member = value(json!({ "proxies": [{ "name": "m" }] }));
    let result = append_proxies(&working, &member, &pid("m"), &mut logs).to_json();
    assert_eq!(result, json!({ "proxies": 3 }));
    assert_eq!(logs.len(), 1);
}
