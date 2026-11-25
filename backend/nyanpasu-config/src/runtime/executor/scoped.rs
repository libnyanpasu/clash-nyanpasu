//! Scoped FileConfig branch: parse + own transforms; reused for the selected
//! file, composition base and contributors (clean-design §7.1/§7.2).

use std::sync::Arc;

use snafu::ResultExt;

use crate::{
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::snapshot::{ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
};

use super::{
    Halted, HaltedStep, LogSink, apply_transform,
    error::{
        CompositionMemberInvalidSnafu, ContentSourceSnafu, ParseProfileSnafu,
        SelectedProfileNotConfigSnafu, SelectedProfileNotFoundSnafu,
    },
    or_halt, parse_config_document,
    ports::{ProfileContentSource, ScriptRunner},
};
use crate::runtime::value::ConfigValue;

pub(super) struct ScopedBuild {
    pub builder: ConfigSnapshotsBuilder,
    pub value: Arc<ConfigValue>,
}

pub(super) fn build_scoped_file(
    profiles: &Profiles,
    content: &dyn ProfileContentSource,
    runner: &dyn ScriptRunner,
    logs: &mut LogSink,
    profile_id: &ProfileId,
    role: ConfigExecutionRole,
) -> Result<ScopedBuild, Halted> {
    let member_error = |reason: &str| match &role {
        ConfigExecutionRole::Selected => SelectedProfileNotConfigSnafu {
            profile: profile_id.clone(),
        }
        .build(),
        ConfigExecutionRole::CompositionBase { composition_id }
        | ConfigExecutionRole::CompositionContributor { composition_id, .. } => {
            CompositionMemberInvalidSnafu {
                composition: composition_id.clone(),
                member: profile_id.clone(),
                reason,
            }
            .build()
        }
    };

    // A failure before the root is recorded names the profile being read.
    let profile_step = || HaltedStep::Profile {
        profile_id: profile_id.clone(),
    };

    let Some(item) = profiles.items.get(profile_id) else {
        let error = match &role {
            ConfigExecutionRole::Selected => SelectedProfileNotFoundSnafu {
                profile: profile_id.clone(),
            }
            .build(),
            _ => member_error("member not found"),
        };
        return Err(Halted::at(error, profile_step()));
    };
    let ProfileDefinition::Config {
        config: ConfigDefinition::File(file),
    } = &item.definition
    else {
        return Err(Halted::at(
            member_error("not a direct FileConfig"),
            profile_step(),
        ));
    };

    let path = file.source.materialized().file.clone();
    let text = content
        .read(&path)
        .with_context(|_| ContentSourceSnafu {
            profile: profile_id.clone(),
            path: path.clone(),
        })
        .map_err(|error| Halted::at(error, profile_step()))?;
    let raw = match parse_config_document(&text) {
        Ok(raw) => raw,
        Err(message) => {
            return Err(Halted::at(
                ParseProfileSnafu {
                    profile: profile_id.clone(),
                    message,
                }
                .build(),
                profile_step(),
            ));
        }
    };

    let mut value = Arc::new(raw);
    let mut builder = ConfigSnapshotsBuilder::new_root(
        value.clone(),
        OperatorTag::FileConfigRoot {
            profile_id: profile_id.clone(),
            role: role.clone(),
        },
    );

    for (index, transform_id) in file.transforms.iter().enumerate() {
        let (next, kind, entries, failed) = or_halt!(
            apply_transform(profiles, content, runner, transform_id, &value),
            builder,
            HaltedStep::Transform {
                profile_id: transform_id.clone()
            }
        );
        let tag = OperatorTag::ScopedTransform {
            host_profile_id: profile_id.clone(),
            role: role.clone(),
            transform_profile_id: transform_id.clone(),
            transform_kind: kind,
            step_index: index as u32,
        };
        logs.failed_profile(transform_id, failed);
        logs.extend(tag.node_key(), entries);
        or_halt!(builder.push(tag, next.clone()), builder);
        value = next;
    }

    Ok(ScopedBuild { builder, value })
}
