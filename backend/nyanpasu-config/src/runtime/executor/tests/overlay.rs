use serde_json::json;

use super::support::{ExprReply, FakeScriptRunner, PredicateReply};
use crate::runtime::{
    executor::{StepLogLevel, overlay::apply_overlay},
    value::ConfigValue,
};

fn value(json: serde_json::Value) -> ConfigValue {
    ConfigValue::try_from(json).unwrap()
}

fn apply(
    overlay: serde_json::Value,
    config: serde_json::Value,
) -> (serde_json::Value, Vec<(StepLogLevel, String)>) {
    apply_with(overlay, config, &FakeScriptRunner::default())
}

fn apply_with(
    overlay: serde_json::Value,
    config: serde_json::Value,
    runner: &FakeScriptRunner,
) -> (serde_json::Value, Vec<(StepLogLevel, String)>) {
    let mut logs = Vec::new();
    let result = apply_overlay(&value(overlay), value(config), runner, &mut logs);
    (
        result.to_json(),
        logs.into_iter()
            .map(|entry| (entry.level, entry.message))
            .collect(),
    )
}

#[test]
fn prepend_and_append_splice_sequences() {
    let (result, logs) = apply(
        json!({ "prepend-rules": ["r0"], "append__rules": ["r9"] }),
        json!({ "rules": ["r1", "r2"] }),
    );
    assert_eq!(result, json!({ "rules": ["r0", "r1", "r2", "r9"] }));
    assert!(logs.is_empty());
}

#[test]
fn prepend_missing_or_non_sequence_field_warns_and_skips() {
    let (result, logs) = apply(json!({ "prepend__rules": ["r0"] }), json!({ "a": 1 }));
    assert_eq!(result, json!({ "a": 1 }));
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].0, StepLogLevel::Warn);

    let (result, logs) = apply(json!({ "append__a": ["x"] }), json!({ "a": 1 }));
    assert_eq!(result, json!({ "a": 1 }));
    assert_eq!(logs.len(), 1);
}

#[test]
fn override_replaces_nested_paths_including_sequence_index() {
    // 对位旧 test_override（merge.rs:436-476）。
    let (result, logs) = apply(
        json!({ "override__a.f.0": "wow", "override__b": 7 }),
        json!({ "a": { "f": [123, 456] }, "b": 1 }),
    );
    assert_eq!(result, json!({ "a": { "f": ["wow", 456] }, "b": 7 }));
    assert!(logs.is_empty());
}

#[test]
fn override_missing_path_warns_and_does_not_create() {
    let (result, logs) = apply(json!({ "override__x.y": 1 }), json!({ "a": 1 }));
    assert_eq!(result, json!({ "a": 1 }));
    assert_eq!(logs.len(), 1);
}

#[test]
fn bare_key_deep_merges_maps_and_replaces_sequences() {
    // 对位旧 test_override_recursive（merge.rs:1031-1071）：
    // 映射深合并保留兄弟键；序列整体替换；缺失键插入。
    let (result, logs) = apply(
        json!({ "a": { "b": { "c": 2 } }, "f": ["wow"], "new": true }),
        json!({ "a": { "b": { "c": 1, "keep": 9 }, "sib": 3 }, "f": [123, 456] }),
    );
    assert_eq!(
        result,
        json!({
            "a": { "b": { "c": 2, "keep": 9 }, "sib": 3 },
            "f": ["wow"],
            "new": true
        })
    );
    assert!(logs.is_empty());
}

#[test]
fn directive_keys_are_lowercased_but_bare_keys_preserve_case() {
    // 怪癖原样保留（spec §13 前注、merge.rs:248 vs :310-312）：
    // 指令路径被小写化 → 找不到混合大小写字段 → WARN 跳过；裸键保留大小写。
    let (result, logs) = apply(
        json!({ "APPEND__Rules.Sub": ["x"], "BareKey": 1 }),
        json!({ "Rules": { "Sub": ["a"] } }),
    );
    assert_eq!(result, json!({ "Rules": { "Sub": ["a"] }, "BareKey": 1 }));
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].0, StepLogLevel::Warn);
}

#[test]
fn filter_string_predicate_retains_and_removes_on_error() {
    let mut runner = FakeScriptRunner::default();
    runner.predicates.insert(
        "keep_big".to_string(),
        PredicateReply::ByItem(|item| {
            item.to_json()
                .get("n")
                .and_then(|n| n.as_i64())
                .unwrap_or(0)
                > 1
        }),
    );
    runner.predicates.insert(
        "boom".to_string(),
        PredicateReply::Fail("lua error".to_string()),
    );

    let (result, logs) = apply_with(
        json!({ "filter__proxies": "keep_big" }),
        json!({ "proxies": [{ "n": 1 }, { "n": 2 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "proxies": [{ "n": 2 }] }));
    assert!(logs.is_empty());

    // 求值错误 = 项被移除 + WARN（parity）。
    let (result, logs) = apply_with(
        json!({ "filter__proxies": "boom" }),
        json!({ "proxies": [{ "n": 1 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "proxies": [] }));
    assert_eq!(logs.len(), 1);
}

#[test]
fn filter_when_variants_expr_override_merge_remove() {
    let mut runner = FakeScriptRunner::default();
    runner
        .predicates
        .insert("hit".to_string(), PredicateReply::Fixed(true));
    runner
        .predicates
        .insert("miss".to_string(), PredicateReply::Fixed(false));
    runner
        .exprs
        .insert("rewrite".to_string(), ExprReply::Fixed(json!({ "n": 99 })));

    // when + expr
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "hit", "expr": "rewrite" } }),
        json!({ "items": [{ "n": 1 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "n": 99 }] }));

    // when(miss) → 原样
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "miss", "expr": "rewrite" } }),
        json!({ "items": [{ "n": 1 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "n": 1 }] }));

    // when + override（字面替换，不经求值）
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "hit", "override": { "fixed": true } } }),
        json!({ "items": [{ "n": 1 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "fixed": true }] }));

    // when + merge（存在键深合并、缺失键插入）
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "hit", "merge": { "a": { "b": 2 }, "add": 1 } } }),
        json!({ "items": [{ "a": { "b": 1, "keep": 3 } }] }),
        &runner,
    );
    assert_eq!(
        result,
        json!({ "items": [{ "a": { "b": 2, "keep": 3 }, "add": 1 }] })
    );

    // when + remove（点路径含末尾序列索引 / 映射键）
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "hit", "remove": ["good.should_remove", "test.1"] } }),
        json!({ "items": [{ "good": { "should_remove": 1, "keep": 2 }, "test": [10, 20, 30] }] }),
        &runner,
    );
    assert_eq!(
        result,
        json!({ "items": [{ "good": { "keep": 2 }, "test": [10, 30] }] })
    );
}

#[test]
fn filter_merge_non_mapping_value_is_invalid_filter() {
    // merge.rs:153 arm guard: non-mapping `merge` matches no arm; the `_` arm
    // warns once and never evaluates `when` (a Fail reply would log if it ran).
    let mut runner = FakeScriptRunner::default();
    runner.predicates.insert(
        "hit".to_string(),
        PredicateReply::Fail("must not run".to_string()),
    );
    let (result, logs) = apply_with(
        json!({ "filter__items": { "when": "hit", "merge": 42 } }),
        json!({ "items": [{ "n": 1 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "n": 1 }] }));
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].0, StepLogLevel::Warn);
    assert!(logs[0].1.contains("invalid filter"));
}

#[test]
fn filter_non_mapping_merge_falls_through_to_remove() {
    // Legacy arm order: an unmatched merge guard falls through to the remove arm.
    let mut runner = FakeScriptRunner::default();
    runner
        .predicates
        .insert("hit".to_string(), PredicateReply::Fixed(true));
    let (result, _) = apply_with(
        json!({ "filter__items": { "when": "hit", "merge": 42, "remove": ["drop"] } }),
        json!({ "items": [{ "drop": 1, "keep": 2 }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "keep": 2 }] }));
}

#[test]
fn filter_merge_on_non_mapping_item_keeps_item() {
    // Legacy panics (merge.rs:163 unwrap); never-fail keeps the item (spec §13 #15).
    let mut runner = FakeScriptRunner::default();
    runner
        .predicates
        .insert("hit".to_string(), PredicateReply::Fixed(true));
    let (result, logs) = apply_with(
        json!({ "filter__rules": { "when": "hit", "merge": { "extra": 1 } } }),
        json!({ "rules": ["MATCH,DIRECT"] }),
        &runner,
    );
    assert_eq!(result, json!({ "rules": ["MATCH,DIRECT"] }));
    assert!(
        logs.iter()
            .any(|(level, message)| *level == StepLogLevel::Warn
                && message.contains("target item is not a mapping"))
    );
}

#[test]
fn filter_remove_entries_respect_item_shape() {
    // merge.rs:186/221: string paths → mapping items only; numeric → sequence items only.
    let mut runner = FakeScriptRunner::default();
    runner
        .predicates
        .insert("hit".to_string(), PredicateReply::Fixed(true));

    // String "0" on a sequence item: legacy logs invalid and keeps the item.
    let (result, logs) = apply_with(
        json!({ "filter__groups": { "when": "hit", "remove": ["0"] } }),
        json!({ "groups": [["a", "b"]] }),
        &runner,
    );
    assert_eq!(result, json!({ "groups": [["a", "b"]] }));
    assert!(logs.iter().any(
        |(level, message)| *level == StepLogLevel::Warn && message.contains("non-mapping item")
    ));

    // Numeric index on a mapping item: legacy logs invalid and keeps the item.
    let (result, logs) = apply_with(
        json!({ "filter__items": { "when": "hit", "remove": [0] } }),
        json!({ "items": [{ "0": "keep" }] }),
        &runner,
    );
    assert_eq!(result, json!({ "items": [{ "0": "keep" }] }));
    assert!(
        logs.iter()
            .any(|(level, message)| *level == StepLogLevel::Warn
                && message.contains("non-sequence item"))
    );

    // Numeric index on a sequence item still removes (legacy parity).
    let (result, _) = apply_with(
        json!({ "filter__groups": { "when": "hit", "remove": [0] } }),
        json!({ "groups": [["a", "b"]] }),
        &runner,
    );
    assert_eq!(result, json!({ "groups": [["b"]] }));
}

#[test]
fn filter_sequence_composes_and_invalid_filter_warns() {
    let mut runner = FakeScriptRunner::default();
    runner.predicates.insert(
        "gt1".to_string(),
        PredicateReply::ByItem(|item| item.to_json().as_i64().unwrap_or(0) > 1),
    );
    runner.predicates.insert(
        "lt3".to_string(),
        PredicateReply::ByItem(|item| item.to_json().as_i64().unwrap_or(9) < 3),
    );

    let (result, _) = apply_with(
        json!({ "filter__nums": ["gt1", "lt3"] }),
        json!({ "nums": [1, 2, 3] }),
        &runner,
    );
    assert_eq!(result, json!({ "nums": [2] }));

    let (result, logs) = apply(json!({ "filter__nums": 42 }), json!({ "nums": [1] }));
    assert_eq!(result, json!({ "nums": [1] }));
    assert_eq!(logs.len(), 1);
}

#[test]
fn overlay_non_mapping_document_warns_and_passes_through() {
    let (result, logs) = apply(json!(["not", "a", "map"]), json!({ "a": 1 }));
    assert_eq!(result, json!({ "a": 1 }));
    assert_eq!(logs.len(), 1);
}
