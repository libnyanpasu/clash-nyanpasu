//! Composition seed + extend_proxies_from (clean-design §7.3–7.5, 11 rules).

use std::sync::Arc;

use crate::{
    profile::{CompositionConfig, ProfileId, Profiles},
    runtime::{
        snapshot::{ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
        value::ConfigValue,
    },
};

use super::{
    LogSink, apply_transform,
    artifact::StepLogEntry,
    error::RuntimePipelineError,
    ports::{ProfileContentSource, ScriptRunner},
    scoped::build_scoped_file,
    value_util::{clean_seed, obj_get, obj_insert},
};

pub(super) fn run_composition(
    profiles: &Profiles,
    content: &dyn ProfileContentSource,
    runner: &dyn ScriptRunner,
    logs: &mut LogSink,
    composition_id: &ProfileId,
    composition: &CompositionConfig,
) -> Result<(ConfigSnapshotsBuilder, Arc<ConfigValue>), RuntimePipelineError> {
    // 规则 1/2：Some(base) → scoped base 起步；None → clean seed。
    let (mut builder, mut working) = match &composition.base {
        Some(base_id) => {
            let base = build_scoped_file(
                profiles,
                content,
                runner,
                logs,
                base_id,
                ConfigExecutionRole::CompositionBase {
                    composition_id: composition_id.clone(),
                },
            )?;
            let working = base.value.clone();
            let mut builder = ConfigSnapshotsBuilder::new_root(
                working.clone(),
                OperatorTag::CompositionRoot {
                    profile_id: composition_id.clone(),
                    base: Some(base_id.clone()),
                },
            );
            builder.attach_independent_branch(builder.root_node_id(), base.builder)?;
            (builder, working)
        }
        None => {
            let seed = Arc::new(clean_seed());
            let builder = ConfigSnapshotsBuilder::new_root(
                seed.clone(),
                OperatorTag::CompositionRoot {
                    profile_id: composition_id.clone(),
                    base: None,
                },
            );
            (builder, seed)
        }
    };

    // 规则 5/6：按声明序；成员先完成 scoped 解析与自身 transforms。
    for (index, member_id) in composition.extend_proxies_from.iter().enumerate() {
        let contributor_index = index as u32;
        let member = build_scoped_file(
            profiles,
            content,
            runner,
            logs,
            member_id,
            ConfigExecutionRole::CompositionContributor {
                composition_id: composition_id.clone(),
                contributor_index,
            },
        )?;
        builder.attach_independent_branch(builder.current_node_id(), member.builder)?;

        let tag = OperatorTag::ExtendProxiesStep {
            composition_id: composition_id.clone(),
            contributor_profile_id: member_id.clone(),
            contributor_index,
        };
        let mut entries = Vec::new();
        working = Arc::new(append_proxies(
            &working,
            &member.value,
            member_id,
            &mut entries,
        ));
        logs.extend(tag.node_key(), entries);
        builder.push(tag, working.clone())?;
    }

    // composition 自身 transforms：host=composition、role=Selected（composition
    // 只能被选中，不能作为成员——validate 保证）。
    for (index, transform_id) in composition.transforms.iter().enumerate() {
        let (next, kind, entries) =
            apply_transform(profiles, content, runner, transform_id, &working);
        let tag = OperatorTag::ScopedTransform {
            host_profile_id: composition_id.clone(),
            role: ConfigExecutionRole::Selected,
            transform_profile_id: transform_id.clone(),
            transform_kind: kind,
            step_index: index as u32,
        };
        logs.extend(tag.node_key(), entries);
        builder.push(tag, next.clone())?;
        working = next;
    }

    Ok((builder, working))
}

/// 规则 3/4/7/8/9/10 + D11 宽容化（缺 proxies → WARN + 空；工作配置缺键 →
/// 建列表；非序列 → WARN + 跳过，保留用户数据）。
pub(super) fn append_proxies(
    working: &ConfigValue,
    member: &ConfigValue,
    member_id: &ProfileId,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let extracted: Vec<ConfigValue> = match obj_get(member, "proxies") {
        Some(value) => match value.as_array_arc() {
            Some(items) => items.to_vec(),
            None => {
                logs.push(StepLogEntry::warn(format!(
                    "member {member_id} `proxies` is not a sequence, contributed nothing"
                )));
                Vec::new()
            }
        },
        None => {
            logs.push(StepLogEntry::warn(format!(
                "member {member_id} has no `proxies`, contributed nothing"
            )));
            Vec::new()
        }
    };

    match obj_get(working, "proxies") {
        Some(value) => match value.as_array_arc() {
            Some(items) => {
                let mut next = items.to_vec();
                next.extend(extracted);
                obj_insert(working, "proxies", ConfigValue::Array(Arc::from(next)))
            }
            None => {
                logs.push(StepLogEntry::warn(
                    "working config `proxies` is not a sequence, append skipped",
                ));
                working.clone()
            }
        },
        None => obj_insert(working, "proxies", ConfigValue::Array(Arc::from(extracted))),
    }
}
