//! Byte-equivalence against legacy enhance() on script-free paths (spec T8).
//! Scenarios deliberately avoid §13 divergences; script-involved end-to-end
//! parity is handed off to PR-3 (real boa/mlua only exist in tauri).

use serde_json::Value;

use super::{orders::base_inputs, support::*};
use crate::runtime::executor::{ExecutionTarget, execute};

const EXPECTED_SINGLE: &str = include_str!("fixtures/parity_single_expected.yaml");
const EXPECTED_MERGED: &str = include_str!("fixtures/parity_merged_expected.yaml");
const EXPECTED_BARE: &str = include_str!("fixtures/parity_bare_expected.yaml");

fn expected(yaml: &str) -> Value {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
    normalized(serde_json::to_value(value).unwrap())
}

/// spec §13 #5：typed guard 恒插 unified-delay/tcp-concurrent，旧 HANDLE 覆盖
/// 不含这两键——parity 比较前双边剥除，其余键逐字节等价。
fn normalized(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove("unified-delay");
        map.remove("tcp-concurrent");
    }
    value
}

fn content() -> MapContentSource {
    MapContentSource::from_pairs(&[
        ("sub-a.yaml", include_str!("fixtures/sub_a.yaml")),
        ("sub-b.yaml", include_str!("fixtures/sub_b.yaml")),
    ])
}

#[test]
fn parity_single_file_config() {
    let profiles = profiles_with(
        Some("a"),
        &[],
        &["dns", "unified-delay", "tcp-concurrent"],
        vec![config_file_item("a", "sub-a.yaml", &[])],
    );
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("a")),
        &overrides,
        &[],
    );
    let artifact = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    assert_eq!(
        normalized(artifact.final_config.to_json()),
        expected(EXPECTED_SINGLE)
    );
}

#[test]
fn parity_merged_equals_base_composition() {
    // 旧 current: [a, b] ≡ 新 Composition{base: a, extend: [b]}（迁移映射语义）。
    let profiles = profiles_with(
        Some("all"),
        &[],
        &["dns", "unified-delay", "tcp-concurrent"],
        vec![
            config_file_item("a", "sub-a.yaml", &[]),
            config_file_item("b", "sub-b.yaml", &[]),
            composition_item("all", Some("a"), &["b"], &[]),
        ],
    );
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("all")),
        &overrides,
        &[],
    );
    let artifact = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    assert_eq!(
        normalized(artifact.final_config.to_json()),
        expected(EXPECTED_MERGED)
    );
}

#[test]
fn parity_bare_mode() {
    let profiles = profiles_with(
        None,
        &[],
        &["dns", "unified-delay", "tcp-concurrent"],
        vec![],
    );
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Bare, &overrides, &[]);
    let artifact = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    assert_eq!(
        normalized(artifact.final_config.to_json()),
        expected(EXPECTED_BARE)
    );
}
