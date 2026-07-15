//! Pure mapping from the executor's RuntimeArtifact to runtime snapshot data
//! model (design §8; executor spec §9.3 endorses applied_fields ↔ exists_keys).
//! Postprocessing layout mirrors legacy PostProcessingOutput: scoped logs keyed
//! by (host profile, transform uid), global/builtin logs keyed by uid/name.

use anyhow::Context as _;
use nyanpasu_config::{
    application::ClashCore,
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::{
        executor::{RuntimeArtifact, StepLog, StepLogLevel},
        snapshot::SnapshotNodeKey,
    },
};
use serde_yaml::Mapping;

use crate::enhance::{Logs, PostProcessingOutput, builtin_transforms_for};

fn span(level: StepLogLevel) -> crate::enhance::utils::LogSpan {
    use crate::enhance::utils::LogSpan;
    match level {
        StepLogLevel::Log => LogSpan::Log,
        StepLogLevel::Info => LogSpan::Info,
        StepLogLevel::Warn => LogSpan::Warn,
        StepLogLevel::Error => LogSpan::Error,
    }
}

fn transform_uid_of(profiles: &Profiles, host: &ProfileId, step_index: u32) -> Option<String> {
    let item = profiles.items.get(host)?;
    let list = match &item.definition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(file),
        } => &file.transforms,
        ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } => &composition.transforms,
        _ => return None,
    };
    list.get(step_index as usize).map(|uid| uid.0.clone())
}

pub(crate) fn map_postprocessing(
    step_logs: &[StepLog],
    profiles: &Profiles,
    builtin_names: &[String],
) -> PostProcessingOutput {
    let mut out = PostProcessingOutput::default();
    for log in step_logs {
        let logs: Logs = log
            .entries
            .iter()
            .map(|entry| (span(entry.level), entry.message.clone()))
            .collect();
        if logs.is_empty() {
            continue;
        }
        match &log.key {
            SnapshotNodeKey::ScopedTransform {
                host_profile_id,
                step_index,
                ..
            } => {
                let transform = transform_uid_of(profiles, host_profile_id, *step_index)
                    .unwrap_or_else(|| format!("step-{step_index}"));
                out.scopes
                    .entry(host_profile_id.0.clone())
                    .or_default()
                    .insert(transform, logs);
            }
            SnapshotNodeKey::GlobalTransform { step_index, .. } => {
                let uid = profiles
                    .global_transforms
                    .get(*step_index as usize)
                    .map(|uid| uid.0.clone())
                    .unwrap_or_else(|| format!("global-{step_index}"));
                out.global.insert(uid, logs);
            }
            SnapshotNodeKey::BuiltinTransform { step_index, .. } => {
                let name = builtin_names
                    .get(*step_index as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("builtin-{step_index}"));
                out.global.insert(name, logs);
            }
            _ => out.advice.extend(logs),
        }
    }
    out
}

pub fn runtime_snapshot_data_from_artifact(
    artifact: &RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> anyhow::Result<crate::client::runtime::RuntimeSnapshotData> {
    let value = serde_yaml::to_value(&*artifact.final_config)
        .context("failed to serialize final config")?;
    let config: Mapping = value
        .as_mapping()
        .cloned()
        .context("final config is not a mapping")?;
    let exists_keys: Vec<String> = artifact.applied_fields.iter().cloned().collect();
    let builtin_names: Vec<String> = if builtin_enabled {
        builtin_transforms_for(core)
            .into_iter()
            .map(|builtin| builtin.name)
            .collect()
    } else {
        Vec::new()
    };
    Ok(crate::client::runtime::RuntimeSnapshotData {
        config,
        exists_keys,
        postprocessing_output: map_postprocessing(&artifact.step_logs, profiles, &builtin_names),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        profile::{ProfileId, Profiles},
        runtime::{
            executor::{StepLog, StepLogEntry, StepLogLevel},
            snapshot::{ConfigExecutionRole, SnapshotNodeKey},
        },
    };

    fn log(key: SnapshotNodeKey, message: &str) -> StepLog {
        StepLog {
            key,
            entries: vec![StepLogEntry::new(StepLogLevel::Info, message)],
        }
    }

    #[test]
    fn maps_scoped_global_and_builtin_logs_to_legacy_layout() {
        let mut profiles = Profiles::default();
        profiles.append_item(crate::enhance::golden_support::file_config(
            "host",
            "h.yaml",
            &["scr1"],
        ));
        profiles.append_item(crate::enhance::golden_support::overlay("scr1", "s.yaml"));
        profiles.append_item(crate::enhance::golden_support::overlay("gfix", "g.yaml"));
        profiles.global_transforms = vec![ProfileId("gfix".into())];

        let host = ProfileId("host".into());
        let logs = vec![
            log(
                SnapshotNodeKey::ScopedTransform {
                    host_profile_id: host.clone(),
                    role: ConfigExecutionRole::Selected,
                    step_index: 0,
                },
                "scoped ran",
            ),
            log(
                SnapshotNodeKey::GlobalTransform {
                    selected_profile_id: Some(host.clone()),
                    step_index: 0,
                },
                "global ran",
            ),
            log(
                SnapshotNodeKey::BuiltinTransform {
                    selected_profile_id: Some(host.clone()),
                    step_index: 0,
                },
                "builtin ran",
            ),
        ];
        let out = map_postprocessing(&logs, &profiles, &["verge_hy_alpn".to_string()]);
        assert_eq!(out.scopes["host"]["scr1"][0].1, "scoped ran");
        assert_eq!(out.global["gfix"][0].1, "global ran");
        assert_eq!(out.global["verge_hy_alpn"][0].1, "builtin ran");
        assert!(out.advice.is_empty());
    }
}
