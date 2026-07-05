//! Pure runtime pipeline executor: the "execution half" of the runtime
//! snapshot store (spec: docs/superpowers/specs/2026-07-04-runtime-pipeline-executor-design.md).

mod artifact;
mod builtin;
mod compose;
mod error;
mod overlay;
mod ports;
mod scoped;
mod value_util;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

pub use artifact::{RuntimeArtifact, StepLog, StepLogEntry, StepLogLevel};
pub use error::RuntimePipelineError;
pub use ports::{PortError, ProfileContentSource, ScriptRunOutcome, ScriptRunner};

use crate::{
    clash::config::{overrides::ClashGuardOverrides, tun_stack::TunStack},
    profile::{
        ProfileDefinition, ProfileId, Profiles, ScriptRuntime, TransformDefinition, TransformKind,
    },
    runtime::{
        snapshot::{
            BuiltinStepKind, ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag,
            SnapshotNodeKey,
        },
        value::ConfigValue,
    },
};

pub struct RuntimePipelineInputs<'a> {
    /// Snapshot that already passed `Profiles::validate()` (spec §4.2).
    pub profiles: &'a Profiles,
    pub target: ExecutionTarget,
    pub guard: GuardInputs<'a>,
    /// `ClashConfig.enable_clash_fields`: gates both whitelist passes.
    pub whitelist_enabled: bool,
    pub tun: TunParams,
    /// Pre-gated and ordered by the caller against `ClashCore` (spec D3).
    pub builtin_transforms: &'a [BuiltinTransform],
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionTarget {
    Selected(ProfileId),
    /// current = None: legacy bare-config path (spec §4.3).
    Bare,
}

pub struct GuardInputs<'a> {
    /// Serialized to kebab-case top-level keys and force-inserted (spec D6).
    pub overrides: &'a ClashGuardOverrides,
    /// Ports resolved by the caller — port probing IO never enters here.
    pub ports: ResolvedPortBindings,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedPortBindings {
    pub mixed_port: u16,
    /// Legacy `port` (HTTP) key; absent when `None`.
    pub port: Option<u16>,
    pub socks_port: Option<u16>,
    /// `host:port`; absent when `None`.
    pub external_controller: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TunParams {
    pub enable: bool,
    pub flavor: TunFlavor,
    /// Platform conditional as input: caller passes `cfg!(windows)` (spec §7.4).
    pub windows_fake_ip_filter: bool,
}

/// Caller derives from (core, tun_stack), including the legacy
/// Premium+Mixed→Gvisor downgrade (tun.rs:58-60); executor stays core-free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TunFlavor {
    ClashRs,
    Standard { stack: TunStack },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinTransform {
    /// Display name recorded in the `BuiltinTransform` tag (e.g. "verge_hy_alpn").
    pub name: String,
    pub runtime: ScriptRuntime,
    pub source: String,
}

/// YAML text → `<<:` merge-key expansion → ConfigValue (spec D13; parity with
/// legacy `help::read_merge_mapping`, utils/help.rs:45-57).
pub(crate) fn parse_config_document(text: &str) -> Result<ConfigValue, String> {
    let mut value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(text).map_err(|error| error.to_string())?;
    value.apply_merge().map_err(|error| error.to_string())?;
    ConfigValue::try_from(value).map_err(|error| format!("{error:?}"))
}

/// Accumulates step logs keyed by semantic node position (spec D8).
#[derive(Default)]
struct LogSink(IndexMap<SnapshotNodeKey, Vec<StepLogEntry>>);

impl LogSink {
    fn extend(&mut self, key: SnapshotNodeKey, entries: Vec<StepLogEntry>) {
        if entries.is_empty() {
            return;
        }
        self.0.entry(key).or_default().extend(entries);
    }

    fn into_step_logs(self) -> Vec<StepLog> {
        self.0
            .into_iter()
            .map(|(key, entries)| StepLog { key, entries })
            .collect()
    }
}

/// Shared transform application (scoped / composition / global). Lenient by
/// design: every failure passes the config through and logs (spec D7). The
/// `TransformKind::Overlay` placeholder on defensive paths is display-only —
/// `node_key()` drops the kind.
pub(crate) fn apply_transform(
    profiles: &Profiles,
    content: &dyn ProfileContentSource,
    runner: &dyn ScriptRunner,
    transform_id: &ProfileId,
    current: &Arc<ConfigValue>,
) -> (Arc<ConfigValue>, TransformKind, Vec<StepLogEntry>) {
    let mut entries = Vec::new();

    let Some(item) = profiles.items.get(transform_id) else {
        entries.push(StepLogEntry::error(format!(
            "transform {transform_id} not found, passthrough"
        )));
        return (current.clone(), TransformKind::Overlay, entries);
    };
    let ProfileDefinition::Transform { transform } = &item.definition else {
        entries.push(StepLogEntry::error(format!(
            "profile {transform_id} is not a transform, passthrough"
        )));
        return (current.clone(), TransformKind::Overlay, entries);
    };

    let kind = transform.kind();
    let path = transform.source().materialized().file.clone();
    let text = match content.read(&path) {
        Ok(text) => text,
        Err(error) => {
            entries.push(StepLogEntry::error(format!(
                "read transform {transform_id} source at {path} failed, passthrough: {error}"
            )));
            return (current.clone(), kind, entries);
        }
    };

    match transform {
        TransformDefinition::Overlay(_) => match parse_config_document(&text) {
            Ok(document) => {
                let next =
                    overlay::apply_overlay(&document, (**current).clone(), runner, &mut entries);
                (Arc::new(next), kind, entries)
            }
            Err(message) => {
                entries.push(StepLogEntry::error(format!(
                    "parse overlay {transform_id} failed, passthrough: {message}"
                )));
                (current.clone(), kind, entries)
            }
        },
        TransformDefinition::Script(script) => {
            let outcome = runner.run(script.runtime, &text, current);
            entries.extend(outcome.logs);
            match outcome.result {
                Ok(next) => (Arc::new(next), kind, entries),
                Err(error) => {
                    // Parity: enhance/utils.rs:118 — error log + passthrough.
                    entries.push(StepLogEntry::error(error.to_string()));
                    (current.clone(), kind, entries)
                }
            }
        }
    }
}

/// Executes the full pipeline for one target (spec §7.1 table + shared tail).
/// Strict on structural failures, lenient on transform failures (spec D7).
pub fn execute(
    inputs: &RuntimePipelineInputs<'_>,
    content: &dyn ProfileContentSource,
    runner: &dyn ScriptRunner,
) -> Result<RuntimeArtifact, RuntimePipelineError> {
    let mut logs = LogSink::default();

    let (mut builder, mut working, selected) = match &inputs.target {
        ExecutionTarget::Bare => {
            let empty = Arc::new(value_util::empty_object());
            (
                ConfigSnapshotsBuilder::new_root(empty.clone(), OperatorTag::BareRoot),
                empty,
                None,
            )
        }
        ExecutionTarget::Selected(id) => {
            let item = inputs
                .profiles
                .items
                .get(id)
                .ok_or_else(|| RuntimePipelineError::SelectedProfileNotFound(id.clone()))?;
            let ProfileDefinition::Config { config } = &item.definition else {
                return Err(RuntimePipelineError::SelectedProfileNotConfig(id.clone()));
            };
            match config {
                crate::profile::ConfigDefinition::File(_) => {
                    let scoped = scoped::build_scoped_file(
                        inputs.profiles,
                        content,
                        runner,
                        &mut logs,
                        id,
                        ConfigExecutionRole::Selected,
                    )?;
                    (scoped.builder, scoped.value, Some(id.clone()))
                }
                crate::profile::ConfigDefinition::Composition(composition) => {
                    let (builder, value) = compose::run_composition(
                        inputs.profiles,
                        content,
                        runner,
                        &mut logs,
                        id,
                        composition,
                    )?;
                    (builder, value, Some(id.clone()))
                }
            }
        }
    };

    // Shared tail (spec §7.1): global → sample → whitelist → guard →
    // builtin×N → finalizing.
    for (index, transform_id) in inputs.profiles.global_transforms.iter().enumerate() {
        let (next, kind, entries) =
            apply_transform(inputs.profiles, content, runner, transform_id, &working);
        let tag = OperatorTag::GlobalTransform {
            selected_profile_id: selected.clone(),
            transform_profile_id: transform_id.clone(),
            transform_kind: kind,
            step_index: index as u32,
        };
        logs.extend(tag.node_key(), entries);
        builder.push(tag, next.clone())?;
        working = next;
    }

    // applied_fields sample (spec D9): current top-level keys, lowercased,
    // insertion order preserved.
    let mut applied_fields: IndexSet<String> = working
        .as_object_arc()
        .map(|map| map.keys().map(|key| key.to_ascii_lowercase()).collect())
        .unwrap_or_default();

    let stage1 = builtin::stage1_fields(&inputs.profiles.valid);
    working = Arc::new(builtin::whitelist_filter(
        &working,
        &stage1,
        inputs.whitelist_enabled,
    ));
    builder.push(
        OperatorTag::BuiltinStep {
            selected_profile_id: selected.clone(),
            step: BuiltinStepKind::WhitelistFieldFilter,
        },
        working.clone(),
    )?;

    working = Arc::new(builtin::apply_guard(&working, &inputs.guard)?);
    builder.push(
        OperatorTag::BuiltinStep {
            selected_profile_id: selected.clone(),
            step: BuiltinStepKind::GuardOverrides,
        },
        working.clone(),
    )?;

    for (index, builtin_transform) in inputs.builtin_transforms.iter().enumerate() {
        let outcome = runner.run(
            builtin_transform.runtime,
            &builtin_transform.source,
            &working,
        );
        let tag = OperatorTag::BuiltinTransform {
            selected_profile_id: selected.clone(),
            name: builtin_transform.name.clone(),
            step_index: index as u32,
        };
        let mut entries = outcome.logs;
        let next = match outcome.result {
            Ok(value) => Arc::new(value),
            Err(error) => {
                // Parity: builtin errors are swallowed with a log (mod.rs:136-141),
                // now retained instead of discarded (spec §13 #7).
                entries.push(StepLogEntry::error(error.to_string()));
                working.clone()
            }
        };
        logs.extend(tag.node_key(), entries);
        builder.push(tag, next.clone())?;
        working = next;
    }

    working = Arc::new(builtin::finalize(
        &working,
        &inputs.tun,
        inputs.whitelist_enabled,
    ));
    builder.push(
        OperatorTag::BuiltinStep {
            selected_profile_id: selected.clone(),
            step: BuiltinStepKind::Finalizing,
        },
        working.clone(),
    )?;

    // Legacy mod.rs:155-157 parity: ∩45 unconditionally; IndexSet preserves
    // order.
    applied_fields.retain(|key| builtin::known_fields().any(|field| field == key));

    let graph = builder.build()?;
    Ok(RuntimeArtifact {
        final_config: working,
        graph,
        step_logs: logs.into_step_logs(),
        applied_fields,
    })
}
