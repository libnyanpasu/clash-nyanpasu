//! Pure runtime pipeline executor: the "execution half" of the runtime
//! snapshot store (spec: docs/spec/2026-07-04-runtime-pipeline-executor-design.md).

mod artifact;
mod builtin;
mod compose;
mod error;
mod include_all;
mod overlay;
mod plan;
mod ports;
mod scoped;
mod value_util;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

pub use artifact::{RuntimeArtifact, StepLog, StepLogEntry, StepLogLevel, TransformFailure};
pub use builtin::known_fields;
pub use error::RuntimePipelineError;
pub use ports::{PortError, ProfileContentSource, ScriptRunner};

use error::{SelectedProfileNotConfigSnafu, SelectedProfileNotFoundSnafu};

use crate::{
    clash::config::{
        overrides::{ClashGuardOverrides, TunOverrides},
        tun_stack::TunStack,
    },
    profile::{
        ProfileDefinition, ProfileId, Profiles, ScriptRuntime, TransformDefinition, TransformKind,
    },
    runtime::{
        post_process::{apply_defaults, apply_settings, keep_inherited},
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
    /// `ClashConfig.enable_clash_fields`: whether the field filter runs.
    pub whitelist_enabled: bool,
    pub tun: TunParams,
    /// `ClashConfig.expand_include_all`: expand `include-all*` groups here
    /// instead of leaving them to the core.
    pub expand_include_all: bool,
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
    /// The `tun` keys these settings decide while `enable` is on; an unmanaged
    /// one is left to the profiles.
    pub overrides: TunOverrides,
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
struct LogSink {
    entries: IndexMap<SnapshotNodeKey, Vec<StepLogEntry>>,
    failures: Vec<TransformFailure>,
}

impl LogSink {
    fn extend(&mut self, key: SnapshotNodeKey, entries: Vec<StepLogEntry>) {
        if entries.is_empty() {
            return;
        }
        self.entries.entry(key).or_default().extend(entries);
    }

    fn failed_profile(&mut self, id: &ProfileId, failed: bool) {
        if failed {
            self.failures
                .push(TransformFailure::Profile { id: id.to_string() });
        }
    }

    fn into_step_logs(self) -> Vec<StepLog> {
        self.entries
            .into_iter()
            .map(|(key, entries)| StepLog { key, entries })
            .collect()
    }
}

/// Shared transform application (scoped / composition / global). Lenient by
/// design: every failure passes the config through and logs (spec D7). The
/// `TransformKind::Overlay` placeholder on defensive paths is display-only —
/// `node_key()` drops the kind.
#[tracing::instrument(skip_all, fields(transform_id = ?transform_id))]
pub(crate) fn apply_transform(
    profiles: &Profiles,
    content: &dyn ProfileContentSource,
    runner: &dyn ScriptRunner,
    transform_id: &ProfileId,
    current: &Arc<ConfigValue>,
) -> (Arc<ConfigValue>, TransformKind, Vec<StepLogEntry>, bool) {
    let mut entries = Vec::new();

    let Some(item) = profiles.items.get(transform_id) else {
        entries.push(StepLogEntry::error(format!(
            "transform {transform_id} not found, passthrough"
        )));
        return (current.clone(), TransformKind::Overlay, entries, true);
    };
    let ProfileDefinition::Transform { transform } = &item.definition else {
        entries.push(StepLogEntry::error(format!(
            "profile {transform_id} is not a transform, passthrough"
        )));
        return (current.clone(), TransformKind::Overlay, entries, true);
    };

    let kind = transform.kind();
    let path = transform.source().materialized().file.clone();
    let text = match content.read(&path) {
        Ok(text) => text,
        Err(error) => {
            entries.push(StepLogEntry::error(format!(
                "read transform {transform_id} source at {path} failed, passthrough: {error}"
            )));
            return (current.clone(), kind, entries, true);
        }
    };

    match transform {
        TransformDefinition::Overlay(_) => match parse_config_document(&text) {
            Ok(document) => {
                let (next, failed) =
                    overlay::apply_overlay(&document, (**current).clone(), runner, &mut entries);
                (Arc::new(next), kind, entries, failed)
            }
            Err(message) => {
                entries.push(StepLogEntry::error(format!(
                    "parse overlay {transform_id} failed, passthrough: {message}"
                )));
                (current.clone(), kind, entries, true)
            }
        },
        TransformDefinition::Script(script) => {
            match runner.run(script.runtime, &text, current, &mut entries) {
                Ok(next) => (Arc::new(next), kind, entries, false),
                Err(error) => {
                    // Parity: enhance/utils.rs:118 — error log + passthrough.
                    entries.push(StepLogEntry::error(error.to_string()));
                    (current.clone(), kind, entries, true)
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
            let Some(item) = inputs.profiles.items.get(id) else {
                return Err(SelectedProfileNotFoundSnafu {
                    profile: id.clone(),
                }
                .build());
            };
            let ProfileDefinition::Config { config } = &item.definition else {
                return Err(SelectedProfileNotConfigSnafu {
                    profile: id.clone(),
                }
                .build());
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

    // Shared tail (spec §7.1): global → sample → field filter → keep
    // unmanaged → managed settings → defaults → builtin×N → include-all
    // expansion. Nyanpasu's own steps record the patch they made instead of
    // diffing the trees.
    for (index, transform_id) in inputs.profiles.global_transforms.iter().enumerate() {
        let (next, kind, entries, failed) =
            apply_transform(inputs.profiles, content, runner, transform_id, &working);
        let tag = OperatorTag::GlobalTransform {
            selected_profile_id: selected.clone(),
            transform_profile_id: transform_id.clone(),
            transform_kind: kind,
            step_index: index as u32,
        };
        logs.failed_profile(transform_id, failed);
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

    let step = |step| OperatorTag::BuiltinStep {
        selected_profile_id: selected.clone(),
        step,
    };
    let ownership = plan::ownership(&inputs.guard, &inputs.tun);
    if inputs.whitelist_enabled {
        let stage1 = builtin::stage1_fields(&inputs.profiles.valid);
        let (filtered, patch) = builtin::filter_fields(&working, &stage1);
        let filtered = Arc::new(filtered);
        builder.push_delta(step(BuiltinStepKind::FieldFilter), filtered.clone(), patch)?;
        // A field left to the profiles keeps their value whatever the filter
        // allows: choosing it is the user's say on that one key.
        let (next, patch) = keep_inherited(&filtered, &working, &ownership);
        working = Arc::new(next);
        builder.push_delta(step(BuiltinStepKind::KeepUnmanaged), working.clone(), patch)?;
    }

    let (next, patch) = apply_settings(&working, &ownership);
    working = Arc::new(next);
    builder.push_delta(
        step(BuiltinStepKind::ManagedSettings),
        working.clone(),
        patch,
    )?;
    let (next, patch) = apply_defaults(&working, &ownership);
    working = Arc::new(next);
    builder.push_delta(step(BuiltinStepKind::Defaults), working.clone(), patch)?;

    for (index, builtin_transform) in inputs.builtin_transforms.iter().enumerate() {
        let mut entries = Vec::new();
        let result = runner.run(
            builtin_transform.runtime,
            &builtin_transform.source,
            &working,
            &mut entries,
        );
        let tag = OperatorTag::BuiltinTransform {
            selected_profile_id: selected.clone(),
            name: builtin_transform.name.clone(),
            step_index: index as u32,
        };
        let next = match result {
            Ok(value) => Arc::new(value),
            Err(error) => {
                // Parity: builtin errors are swallowed with a log (mod.rs:136-141),
                // now retained instead of discarded (spec §13 #7).
                entries.push(StepLogEntry::error(error.to_string()));
                logs.failures.push(TransformFailure::Builtin {
                    name: builtin_transform.name.clone(),
                });
                working.clone()
            }
        };
        logs.extend(tag.node_key(), entries);
        builder.push(tag, next.clone())?;
        working = next;
    }

    if inputs.expand_include_all {
        let (next, entries) = include_all::expand_include_all(&working);
        let tag = OperatorTag::BuiltinStep {
            selected_profile_id: selected.clone(),
            step: BuiltinStepKind::IncludeAllExpansion,
        };
        logs.extend(tag.node_key(), entries);
        working = Arc::new(next);
        builder.push(tag, working.clone())?;
    }

    // TODO: 可选排序算法
    // working = Arc::new(builtin::apply_sort(&working));

    // Legacy mod.rs:155-157 parity: ∩45 unconditionally; IndexSet preserves
    // order.
    applied_fields.retain(|key| builtin::known_fields().any(|field| field == key));

    let (graph, final_node) = builder.build_stored_with_current()?;
    let transform_failures = logs.failures.clone();
    Ok(RuntimeArtifact {
        final_config: working,
        graph,
        final_node,
        step_logs: logs.into_step_logs(),
        applied_fields,
        transform_failures,
    })
}
