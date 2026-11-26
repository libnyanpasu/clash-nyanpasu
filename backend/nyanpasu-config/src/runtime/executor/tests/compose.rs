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

#[test]
fn disabled_transform_engine_is_a_structured_pipeline_failure() {
    use super::{
        orders::base_inputs,
        support::{
            FakeScriptRunner, MapContentSource, RunReply, composition_item, config_file_item,
            profiles_with, script_item,
        },
    };
    use crate::{
        profile::ScriptRuntime,
        runtime::executor::{
            ExecutionTarget, RuntimePipelineError, ScriptExecutionError, ScriptOperation, execute,
        },
    };

    for runtime in [ScriptRuntime::JavaScript, ScriptRuntime::Lua] {
        for host in ["composition", "base", "first", "second"] {
            let transforms = |id| if host == id { vec!["script"] } else { vec![] };
            let profiles = profiles_with(
                Some("composition"),
                &[],
                &[],
                vec![
                    composition_item(
                        "composition",
                        Some("base"),
                        &["first", "second"],
                        &transforms("composition"),
                    ),
                    config_file_item("base", "base.yaml", &transforms("base")),
                    config_file_item("first", "first.yaml", &transforms("first")),
                    config_file_item("second", "second.yaml", &transforms("second")),
                    script_item("script", "script.txt", runtime),
                ],
            );
            profiles.validate().unwrap();
            let content = MapContentSource::from_pairs(&[
                ("base.yaml", "proxies: []"),
                ("first.yaml", "proxies: []"),
                ("second.yaml", "proxies: []"),
                ("script.txt", "disabled"),
            ]);
            let mut runner = FakeScriptRunner::default();
            runner.runs.insert("disabled".into(), RunReply::Unavailable);
            let overrides = super::builtin::fixed_overrides();
            let inputs = base_inputs(
                &profiles,
                ExecutionTarget::Selected(pid("composition")),
                &overrides,
                &[],
            );
            let failure = execute(&inputs, &content, &runner).unwrap_err();
            // The stopped build names the transform it could not run.
            assert_eq!(
                failure
                    .partial
                    .as_ref()
                    .and_then(|partial| partial.failed_step.clone()),
                Some(crate::runtime::executor::HaltedStep::Transform {
                    profile_id: pid("script")
                }),
                "{host}"
            );
            let error = failure.error;
            assert!(
                matches!(
                    error,
                    RuntimePipelineError::ExecuteScript {
                        source: ScriptExecutionError::StartEngine {
                            runtime: actual,
                            operation: ScriptOperation::Transform,
                        }
                    } if actual == runtime
                ),
                "{host}: {error:?}"
            );
        }
    }
}

#[test]
fn a_member_that_cannot_be_read_keeps_the_members_before_it() {
    use super::{
        orders::base_inputs,
        support::{
            FakeScriptRunner, MapContentSource, composition_item, config_file_item, profiles_with,
        },
    };
    use crate::runtime::{
        executor::{ExecutionTarget, RuntimePipelineError, execute},
        snapshot::OperatorTag,
    };

    let profiles = profiles_with(
        Some("composition"),
        &[],
        &[],
        vec![
            composition_item("composition", Some("base"), &["first", "second"], &[]),
            config_file_item("base", "base.yaml", &[]),
            config_file_item("first", "first.yaml", &[]),
            config_file_item("second", "second.yaml", &[]),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        ("base.yaml", "proxies: []"),
        ("first.yaml", "proxies: [{name: a}]"),
    ]);
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("composition")),
        &overrides,
        &[],
    );
    let failure = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap_err();
    assert!(
        matches!(&failure.error, RuntimePipelineError::ContentSource { profile, .. } if *profile == pid("second")),
        "{:?}",
        failure.error
    );
    let partial = failure.partial.expect("the first member completed");
    assert_eq!(
        partial.failed_step,
        Some(crate::runtime::executor::HaltedStep::Profile {
            profile_id: pid("second")
        })
    );
    let tags: Vec<_> = partial.graph.nodes.iter().map(|node| &node.tag).collect();
    assert!(matches!(
        tags[partial.graph.root_id as usize],
        OperatorTag::CompositionRoot { .. }
    ));
    assert!(matches!(
        tags[partial.last_node as usize],
        OperatorTag::ExtendProxiesStep { contributor_profile_id, .. } if *contributor_profile_id == pid("first")
    ));
    for id in ["base", "first"] {
        assert!(tags.iter().any(
            |tag| matches!(tag, OperatorTag::FileConfigRoot { profile_id, .. } if *profile_id == pid(id))
        ));
    }
    // The failed member produced no output, so it has no node.
    assert!(!tags.iter().any(|tag| matches!(
        tag,
        OperatorTag::FileConfigRoot { profile_id, .. } if *profile_id == pid("second")
    )));
    assert_eq!(
        partial.graph.restore(partial.last_node).unwrap()["proxies"],
        json!([{ "name": "a" }])
    );
}

#[test]
fn a_selected_file_that_cannot_be_read_has_no_completed_steps() {
    use super::{
        orders::base_inputs,
        support::{FakeScriptRunner, MapContentSource, config_file_item, profiles_with},
    };
    use crate::runtime::executor::{ExecutionTarget, execute};

    let profiles = profiles_with(
        Some("selected"),
        &[],
        &[],
        vec![config_file_item("selected", "selected.yaml", &[])],
    );
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("selected")),
        &overrides,
        &[],
    );
    let failure = execute(
        &inputs,
        &MapContentSource::from_pairs(&[]),
        &FakeScriptRunner::default(),
    )
    .unwrap_err();
    assert!(failure.partial.is_none());
}
