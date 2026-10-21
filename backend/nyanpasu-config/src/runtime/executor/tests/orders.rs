use serde_json::json;

use super::support::*;
use crate::{
    clash::config::{
        overrides::{ManageableField, TunOverrides},
        tun_stack::TunStack,
    },
    profile::{Profiles, ScriptRuntime},
    runtime::{
        executor::{
            BuiltinTransform, ExecutionTarget, GuardInputs, ResolvedPortBindings,
            RuntimePipelineError, RuntimePipelineInputs, StepLogLevel, TunFlavor, TunParams,
            execute,
        },
        snapshot::{BuiltinStepKind, ConfigExecutionRole, OperatorTag},
    },
};

pub fn base_inputs<'a>(
    profiles: &'a Profiles,
    target: ExecutionTarget,
    overrides: &'a crate::clash::config::overrides::ClashGuardOverrides,
    builtin_transforms: &'a [BuiltinTransform],
) -> RuntimePipelineInputs<'a> {
    RuntimePipelineInputs {
        profiles,
        target,
        guard: GuardInputs {
            overrides,
            ports: ResolvedPortBindings {
                mixed_port: 7890,
                port: None,
                socks_port: None,
                external_controller: None,
            },
        },
        whitelist_enabled: true,
        tun: TunParams {
            enable: false,
            overrides: TunOverrides::default(),
            flavor: TunFlavor::Standard {
                stack: TunStack::Gvisor,
            },
            windows_fake_ip_filter: false,
        },
        expand_include_all: true,
        builtin_transforms,
    }
}

fn overrides() -> crate::clash::config::overrides::ClashGuardOverrides {
    super::builtin::fixed_overrides()
}

#[test]
fn selected_file_config_records_canonical_mainline() {
    // §7.1：root → scoped → global → field filter → keep unmanaged → managed
    // settings → defaults → builtin-transform → include-all
    let profiles = profiles_with(
        Some("sub-a"),
        &["global-fix"],
        &["dns"],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &["normalize"]),
            overlay_item("normalize", "normalize.yaml"),
            overlay_item("global-fix", "global-fix.yaml"),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        (
            "sub-a.yaml",
            "proxies:\n  - name: a1\nrules: [r1]\ndns: { enable: true }\n",
        ),
        ("normalize.yaml", "append__rules: [r-scoped]\n"),
        ("global-fix.yaml", "append__rules: [r-global]\n"),
    ]);
    let ov = overrides();
    let builtins = vec![BuiltinTransform {
        name: "config_fixer".to_string(),
        runtime: ScriptRuntime::JavaScript,
        source: "builtin-src".to_string(),
    }];
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("sub-a")),
        &ov,
        &builtins,
    );

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let tags: Vec<_> = artifact.graph.nodes.iter().map(|node| &node.tag).collect();
    assert!(matches!(
        tags[0],
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::Selected,
            ..
        }
    ));
    assert!(matches!(
        tags[1],
        OperatorTag::ScopedTransform { step_index: 0, .. }
    ));
    assert!(matches!(
        tags[2],
        OperatorTag::GlobalTransform { step_index: 0, .. }
    ));
    assert!(matches!(
        tags[3],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::FieldFilter,
            ..
        }
    ));
    assert!(matches!(
        tags[4],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::KeepUnmanaged,
            ..
        }
    ));
    assert!(matches!(
        tags[5],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::ManagedSettings,
            ..
        }
    ));
    assert!(matches!(
        tags[6],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::Defaults,
            ..
        }
    ));
    assert!(matches!(
        tags[7],
        OperatorTag::BuiltinTransform { step_index: 0, .. }
    ));
    assert!(matches!(
        tags[8],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::IncludeAllExpansion,
            ..
        }
    ));
    assert_eq!(artifact.graph.nodes.len(), 9);
    assert_eq!(artifact.final_node, 8);

    // 语义断言：scoped + global 追加都生效，guard 覆盖 mode。
    let config = artifact.final_config.to_json();
    assert_eq!(config["rules"], json!(["r1", "r-scoped", "r-global"]));
    assert_eq!(config["mode"], json!("rule"));
    assert_eq!(config["mixed-port"], json!(7890));

    // applied_fields：全局变换后采样(proxies/rules/dns)，确定序。
    let applied: Vec<&str> = artifact.applied_fields.iter().map(String::as_str).collect();
    assert_eq!(applied, vec!["proxies", "rules", "dns"]);
}

#[test]
fn composition_with_base_matches_recording_contract() {
    // §7.3 + spec §8.1：base/成员分支独立嫁接，extend 主线推进。
    let profiles = profiles_with(
        Some("all"),
        &[],
        &[],
        vec![
            config_file_item("base", "base.yaml", &[]),
            config_file_item("member", "member.yaml", &[]),
            composition_item("all", Some("base"), &["member"], &[]),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        ("base.yaml", "proxies:\n  - name: b1\nrules: [r1]\n"),
        ("member.yaml", "proxies:\n  - name: m1\nextra: dropped\n"),
    ]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("all")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    // 11 条规则：只提取 proxies、按序追加、不合并其他字段、不去重。
    let names: Vec<&str> = config["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["b1", "m1"]);
    assert_eq!(config["rules"], json!(["r1"]));
    assert!(config.get("extra").is_none());

    crate::runtime::snapshot::assert_restore_matches_materialize(&artifact.graph);
    // 图形状（对齐 snapshot.rs composition 测试期望形态）。
    let nodes = &artifact.graph.nodes;
    let root = artifact.graph.root_id as usize;
    assert!(matches!(
        &nodes[root].tag,
        OperatorTag::CompositionRoot { base: Some(_), .. }
    ));
    let root_children = nodes[root].next.as_deref().unwrap();
    assert_eq!(root_children.len(), 3); // base 分支根、member 分支根、extend 步
    let base_branch = root_children[0] as usize;
    assert!(matches!(
        &nodes[base_branch].tag,
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::CompositionBase { .. },
            ..
        }
    ));
    let member_branch = root_children[1] as usize;
    assert!(matches!(
        &nodes[member_branch].tag,
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::CompositionContributor { .. },
            ..
        }
    ));
    let extend = root_children[2] as usize;
    assert!(matches!(
        &nodes[extend].tag,
        OperatorTag::ExtendProxiesStep {
            contributor_index: 0,
            ..
        }
    ));
}

#[test]
fn composition_without_base_starts_from_clean_seed() {
    // §7.4：clean seed = {proxies: []}，不隐式注入其他字段。
    let profiles = profiles_with(
        Some("clean"),
        &[],
        &[],
        vec![
            config_file_item("member", "member.yaml", &[]),
            composition_item("clean", None, &["member"], &[]),
        ],
    );
    let content = MapContentSource::from_pairs(&[("member.yaml", "proxies:\n  - name: m1\n")]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("clean")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let root = artifact.graph.root_id as usize;
    assert!(matches!(
        &artifact.graph.nodes[root].tag,
        OperatorTag::CompositionRoot { base: None, .. }
    ));
    assert_eq!(
        artifact.graph.restore(root as u32).unwrap(),
        json!({ "proxies": [] })
    );
    let final_config = artifact.final_config.to_json();
    let names: Vec<&str> = final_config["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["m1"]);
}

#[test]
fn bare_target_reuses_shared_tail_with_none_selected() {
    // §7.1 场景⑤：BareRoot → global(选中=None) → 尾部四步；产出可用裸配置。
    let profiles = profiles_with(
        None,
        &["global-fix"],
        &[],
        vec![overlay_item("global-fix", "g.yaml")],
    );
    let content = MapContentSource::from_pairs(&[("g.yaml", "injected: { a: 1 }\n")]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Bare, &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let tags: Vec<_> = artifact.graph.nodes.iter().map(|node| &node.tag).collect();
    assert!(matches!(tags[0], OperatorTag::BareRoot));
    assert!(matches!(
        tags[1],
        OperatorTag::GlobalTransform {
            selected_profile_id: None,
            ..
        }
    ));
    assert!(matches!(
        tags[2],
        OperatorTag::BuiltinStep {
            selected_profile_id: None,
            step: BuiltinStepKind::FieldFilter
        }
    ));

    let config = artifact.final_config.to_json();
    assert_eq!(config["mode"], json!("rule")); // guard 生效
    assert_eq!(config["mixed-port"], json!(7890));
    assert!(config.get("injected").is_none()); // whitelist 过滤了非法注入
    assert_eq!(config["profile"]["store-selected"], json!(true)); // cache 生效
}

#[test]
fn script_transform_failure_is_lenient_with_anchored_error_log() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &["boom"]),
            script_item("boom", "boom.js", ScriptRuntime::JavaScript),
        ],
    );
    let content =
        MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n"), ("boom.js", "explode")]);
    let mut runner = FakeScriptRunner::default();
    runner.runs.insert(
        "explode".to_string(),
        RunReply::Fail("script exploded".to_string(), vec![]),
    );
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let artifact = execute(&inputs, &content, &runner).unwrap();

    // 透传：scoped 节点值 == root 值。
    assert_eq!(
        artifact.graph.restore(1).unwrap(),
        artifact.graph.restore(0).unwrap()
    );
    // error 日志锚定在该 ScopedTransform 节点键上。
    let scoped_key = artifact.graph.nodes[1].tag.node_key();
    let log = artifact
        .step_logs
        .iter()
        .find(|log| log.key == scoped_key)
        .unwrap();
    assert!(
        log.entries
            .iter()
            .any(|entry| entry.level == StepLogLevel::Error)
    );
}

#[test]
fn missing_selected_content_is_strict_error() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "missing.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let error = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap_err();
    assert!(matches!(error, RuntimePipelineError::ContentSource { .. }));
}

#[test]
fn applied_fields_keep_prefilter_known_keys_and_drop_unknown_even_unfiltered() {
    // D9：采样点在全局变换后；∩45 无条件（enable_filter=false 时 config 保留
    // 未知键但 applied_fields 不列——旧 mod.rs:155-157 怪癖保留）。
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "mode: from-profile\ncustom-unknown: 1\nproxies: []\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    // mode ∈ HANDLE ⊂ 45 → 计入(即使 stage-1 会滤掉它——采样在过滤前)；
    // custom-unknown ∉ 45 → 不计入，但 config 里保留(whitelist 关闭)。
    assert!(artifact.applied_fields.contains("mode"));
    assert!(artifact.applied_fields.contains("proxies"));
    assert!(!artifact.applied_fields.contains("custom-unknown"));
    assert_eq!(artifact.final_config.to_json()["custom-unknown"], json!(1));
}

/// #5678: with the whitelist off, a subscription's `port: 7890` used to reach
/// the core next to `mixed-port: 7890`; the HTTP listener then took the port
/// and the mixed listener failed to bind. Nyanpasu owns the proxy ports.
#[test]
fn unfiltered_profile_cannot_open_managed_proxy_ports() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "port: 7890\nsocks-port: 7891\ncustom-unknown: 1\nproxies: []\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;

    let unset = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert!(unset.get("port").is_none());
    assert!(unset.get("socks-port").is_none());
    assert_eq!(unset["mixed-port"], json!(7890));
    assert_eq!(unset["custom-unknown"], json!(1));

    inputs.guard.ports.port = Some(7892);
    inputs.guard.ports.socks_port = Some(7893);
    let configured = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert_eq!(configured["port"], json!(7892));
    assert_eq!(configured["socks-port"], json!(7893));
}

/// An unmanaged `auto-route` keeps the profile's value even where the field
/// filter drops the rest of `tun`, while the TUN switch stays forced.
#[test]
fn unmanaged_auto_route_survives_field_filtering_while_tun_enable_stays_forced() {
    for valid_fields in [vec![], vec!["tun"]] {
        let profiles = profiles_with(
            Some("sub-a"),
            &[],
            &valid_fields,
            vec![config_file_item("sub-a", "sub-a.yaml", &[])],
        );
        let content = MapContentSource::from_pairs(&[(
            "sub-a.yaml",
            "tun: { enable: false, auto-route: global, mtu: 1400 }\n",
        )]);
        let ov = overrides();
        let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
        inputs.tun.overrides.auto_route = ManageableField::Unmanaged;
        for whitelist_enabled in [false, true] {
            inputs.whitelist_enabled = whitelist_enabled;
            for enabled in [false, true] {
                inputs.tun.enable = enabled;
                let config = execute(&inputs, &content, &FakeScriptRunner::default())
                    .unwrap()
                    .final_config
                    .to_json();
                if !whitelist_enabled || valid_fields.contains(&"tun") {
                    assert_eq!(config["tun"]["auto-route"], json!("global"));
                    assert_eq!(config["tun"]["mtu"], json!(1400));
                    assert_eq!(config["tun"]["enable"], json!(enabled));
                } else if enabled {
                    assert_eq!(config["tun"]["auto-route"], json!("global"));
                    assert!(config["tun"].get("mtu").is_none());
                    assert_eq!(config["tun"]["enable"], json!(true));
                } else {
                    assert!(config.get("tun").is_none());
                }
            }
        }
    }
}

/// #5609: a global Merge owns `tcp-concurrent` and `unified-delay` once they
/// are unmanaged; it sets one and leaves the other out.
#[test]
fn global_merge_owns_unmanaged_guard_fields() {
    let profiles = profiles_with(
        Some("sub-a"),
        &["merge"],
        &["dns", "unified-delay", "tcp-concurrent"],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &[]),
            overlay_item("merge", "merge.yaml"),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        ("sub-a.yaml", "proxies: []\nrules: []\n"),
        ("merge.yaml", "tcp-concurrent: false\n"),
    ]);
    let ov = super::builtin::unmanaged_overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let config = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();

    assert_eq!(config["tcp-concurrent"], json!(false));
    assert!(config.get("unified-delay").is_none());
    assert_eq!(config["mode"], json!("rule"));
}

/// With the field filter off, no filter step is recorded.
#[test]
fn the_field_filter_step_is_left_out_while_the_filter_is_off() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n")]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;
    inputs.expand_include_all = false;

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let steps: Vec<_> = artifact
        .graph
        .nodes
        .iter()
        .filter_map(|node| match &node.tag {
            OperatorTag::BuiltinStep { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    assert_eq!(
        steps,
        [BuiltinStepKind::ManagedSettings, BuiltinStepKind::Defaults]
    );
    assert_eq!(artifact.final_node as usize, artifact.graph.nodes.len() - 1);
}

/// The filter, managed settings and defaults steps record the patches they
/// made instead of diffing. Replaying the graph must still rebuild the config
/// the pipeline produced, key order included, whatever the settings.
#[test]
fn recorded_patches_replay_into_the_built_config() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &["dns"],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "port: 7890\nmode: global\ntun: { enable: true, stack: system, auto-route: false }\n\
         custom-unknown: 1\ndns: { enable: false, nameserver: [1.1.1.1] }\nproxies: []\n\
         profile: { store-selected: false }\nrules: []\n",
    )]);
    let ov = overrides();
    for whitelist_enabled in [false, true] {
        for tun in [false, true] {
            for port in [None, Some(7892)] {
                let mut inputs =
                    base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
                inputs.whitelist_enabled = whitelist_enabled;
                inputs.tun.enable = tun;
                inputs.tun.windows_fake_ip_filter = tun;
                inputs.guard.ports.port = port;

                let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

                let case = format!("filter {whitelist_enabled}, tun {tun}, port {port:?}");
                assert_eq!(
                    serde_json::to_string(&artifact.graph.restore(artifact.final_node).unwrap())
                        .unwrap(),
                    serde_json::to_string(&artifact.final_config.to_json()).unwrap(),
                    "{case}"
                );
                crate::runtime::snapshot::assert_restore_matches_materialize(&artifact.graph);
            }
        }
    }
}

/// #5791: a field left to the profiles keeps their value even where the
/// field filter does not allow its key, so one setting is enough. The keep
/// step records just that key.
#[test]
fn unmanaged_fields_keep_the_profile_value_the_filter_drops() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "unified-delay: false\ntcp-concurrent: false\ncustom-unknown: 1\nproxies: []\n",
    )]);
    let unmanaged = super::builtin::unmanaged_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("sub-a")),
        &unmanaged,
        &[],
    );

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let config = artifact.final_config.to_json();
    assert_eq!(config["unified-delay"], json!(false));
    assert_eq!(config["tcp-concurrent"], json!(false));
    assert!(config.get("custom-unknown").is_none());
    let kept = artifact
        .graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::KeepUnmanaged,
                    ..
                }
            )
        })
        .unwrap();
    let mut fields: Vec<_> = kept.changed_fields.iter().flatten().cloned().collect();
    fields.sort();
    assert_eq!(fields, ["tcp-concurrent", "unified-delay"]);

    // Managed, the same keys are forced instead.
    let managed = overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("sub-a")),
        &managed,
        &[],
    );
    let config = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert_eq!(config["unified-delay"], json!(true));
    assert_eq!(config["tcp-concurrent"], json!(true));
}
