//! Scoped FileConfig branch: parse + own transforms; reused for the selected
//! file, composition base and contributors (clean-design §7.1/§7.2).

use std::sync::Arc;

use snafu::ResultExt;

use crate::{
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::snapshot::{ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
};

use super::{
    LogSink, apply_transform,
    error::{
        CompositionMemberInvalidSnafu, ContentSourceSnafu, ParseProfileSnafu, RuntimePipelineError,
        SelectedProfileNotConfigSnafu, SelectedProfileNotFoundSnafu,
    },
    parse_config_document,
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
) -> Result<ScopedBuild, RuntimePipelineError> {
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

    let Some(item) = profiles.items.get(profile_id) else {
        return Err(match &role {
            ConfigExecutionRole::Selected => SelectedProfileNotFoundSnafu {
                profile: profile_id.clone(),
            }
            .build(),
            _ => member_error("member not found"),
        });
    };
    let ProfileDefinition::Config {
        config: ConfigDefinition::File(file),
    } = &item.definition
    else {
        return Err(member_error("not a direct FileConfig"));
    };

    let path = file.source.materialized().file.clone();
    let text = content.read(&path).with_context(|_| ContentSourceSnafu {
        profile: profile_id.clone(),
        path: path.clone(),
    })?;
    let raw = match parse_config_document(&text) {
        Ok(raw) => raw,
        Err(message) => {
            return ParseProfileSnafu {
                profile: profile_id.clone(),
                message,
            }
            .fail();
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
        let (next, kind, entries, failed) =
            apply_transform(profiles, content, runner, transform_id, &value);
        let tag = OperatorTag::ScopedTransform {
            host_profile_id: profile_id.clone(),
            role: role.clone(),
            transform_profile_id: transform_id.clone(),
            transform_kind: kind,
            step_index: index as u32,
        };
        logs.failed_profile(transform_id, failed);
        logs.extend(tag.node_key(), entries);
        builder.push(tag, next.clone())?;
        value = next;
    }

    Ok(ScopedBuild { builder, value })
}
