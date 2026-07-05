//! Five-order golden coverage against clean-design §6.1-shaped inputs, plus
//! the determinism invariant (spec §10.2).

use serde_json::json;

use super::{orders::base_inputs, support::*};
use crate::runtime::executor::{ExecutionTarget, execute};

const SUB_A: &str = include_str!("fixtures/sub_a.yaml");
const SUB_B: &str = include_str!("fixtures/sub_b.yaml");
const BUILD_GROUPS: &str = include_str!("fixtures/build_groups.yaml");
const GLOBAL_FIX: &str = include_str!("fixtures/global_fix.yaml");

fn content() -> MapContentSource {
    MapContentSource::from_pairs(&[
        ("sub-a.yaml", SUB_A),
        ("sub-b.yaml", SUB_B),
        ("build-groups.yaml", BUILD_GROUPS),
        ("global-fix.yaml", GLOBAL_FIX),
    ])
}

/// §6.1 缩影：两个订阅、一个 base 组合、一个 clean-seed 组合、
/// overlay transforms + global transform。
fn golden_profiles() -> crate::profile::Profiles {
    profiles_with(
        Some("clean-subscriptions"),
        &["global-fix"],
        &["dns", "unified-delay", "tcp-concurrent"],
        vec![
            config_file_item("subscription-a", "sub-a.yaml", &[]),
            config_file_item("subscription-b", "sub-b.yaml", &[]),
            composition_item(
                "all-subscriptions",
                Some("subscription-a"),
                &["subscription-b"],
                &[],
            ),
            composition_item(
                "clean-subscriptions",
                None,
                &["subscription-a", "subscription-b"],
                &["build-groups"],
            ),
            overlay_item("build-groups", "build-groups.yaml"),
            overlay_item("global-fix", "global-fix.yaml"),
        ],
    )
}

#[test]
fn golden_clean_seed_composition_full_config() {
    let profiles = golden_profiles();
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("clean-subscriptions")),
        &overrides,
        &[],
    );

    let artifact = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    // 完整 golden：固定 secret 下逐键断言。注意两处语义要点：
    // ① clean seed 无 `rules` 键 → global-fix 的 `append__rules` 按旧语义
    //    「目标不存在 → WARN 跳过」，最终配置**没有** rules 键（不隐式创建）；
    // ② ResolvedPortBindings 的 port/socks_port/external_controller 均为
    //    None → 对应键**不插入**（guard 只插 mixed-port）。
    assert_eq!(
        config,
        json!({
            "mode": "rule",
            "mixed-port": 7890,
            "allow-lan": false,
            "log-level": "info",
            "ipv6": false,
            "secret": "golden-secret",
            "profile": { "store-selected": true, "store-fake-ip": false },
            "unified-delay": true,
            "tcp-concurrent": true,
            "proxies": [
                { "name": "a1", "type": "ss", "server": "a.example.com", "port": 443 },
                { "name": "b1", "type": "vmess", "server": "b.example.com", "port": 8080 }
            ],
            "proxy-groups": [
                { "name": "Auto", "type": "select", "proxies": ["a1", "b1"] }
            ]
        })
    );

    // append 在缺失字段上跳过时必须留下 WARN（锚定在 GlobalTransform 节点）。
    assert!(artifact.step_logs.iter().any(|log| {
        log.entries
            .iter()
            .any(|entry| entry.message.contains("rules"))
    }));
}

#[test]
fn golden_base_composition_inherits_base_fields() {
    let profiles = golden_profiles();
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("all-subscriptions")),
        &overrides,
        &[],
    );

    let artifact = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    // base 的完整字段(dns/rules)继承；成员只贡献 proxies；global 追加 rules。
    assert_eq!(config["dns"], json!({ "enable": true }));
    assert_eq!(
        config["rules"],
        json!(["MATCH,DIRECT", "DOMAIN,example.com,DIRECT"])
    );
    let names: Vec<&str> = config["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["a1", "b1"]);
    // 成员 scoped 之外的字段不合并（规则 9）。
    assert!(config.get("custom-field").is_none());
}

#[test]
fn determinism_same_inputs_same_artifact() {
    let profiles = golden_profiles();
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("clean-subscriptions")),
        &overrides,
        &[],
    );

    let first = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    let second = execute(&inputs, &content(), &FakeScriptRunner::default()).unwrap();
    assert_eq!(first, second); // graph 节点 id、键、日志序全等
}
