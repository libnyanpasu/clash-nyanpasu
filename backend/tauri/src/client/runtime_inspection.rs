//! Read-only projections of one immutable, promoted pipeline build.
use std::sync::Arc;

use nyanpasu_config::{
    profile::ProfileId,
    runtime::{
        executor::{StepLog, StepLogEntry},
        snapshot::{
            BuiltinStepKind, Idx, OperatorTag, SnapshotBuildError, SnapshotDiffHunk,
            StoredConfigSnapshotsGraph, changed_fields_between,
        },
    },
};
use serde::Serialize;
use snafu::{OptionExt, ResultExt, ensure};

use super::{
    NyanpasuClient,
    runtime::RuntimeSnapshot,
    runtime_error::{
        ConvertRuntimeConfigSnafu, NoRuntimeConfigSnafu, RuntimeError, RuntimeNodeNotFoundSnafu,
        RuntimeSnapshotChangedSnafu, SerializeRuntimeConfigSnafu,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct RuntimeInspectionData {
    pub graph: StoredConfigSnapshotsGraph,
    pub step_logs: Vec<StepLog>,
}

/// The configuration the core reported running for an applied build, viewed
/// as the finalizing node's child. It stays outside the build's graph, so
/// stamping a build never copies the graph.
#[derive(Debug)]
pub(crate) struct EffectiveInspection {
    pub config: nyanpasu_ipc::api::core::v2::CoreEffectiveConfig,
    pub host: (crate::core::actor_v2::endpoint::ExecutionHost, u64),
    /// The finalizing node the effective document is compared against.
    parent: Idx,
    tag: OperatorTag,
    /// Computed once when stamped, against the finalizing node.
    changed_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspection {
    pub snapshot_id: String,
    pub applied: bool,
    pub effective_pending: bool,
    pub effective_revision: Option<nyanpasu_ipc::api::status::ConfigRevisionInfo>,
    pub revision: String,
    pub target_core: String,
    pub root_id: u32,
    pub nodes: Vec<RuntimeInspectionNode>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionNode {
    pub id: u32,
    pub tag: OperatorTag,
    pub next: Vec<u32>,
    pub has_logs: bool,
    /// None means unchanged or no comparison baseline (including independent roots).
    pub changed_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionContent {
    pub yaml: String,
    pub diff: Option<RuntimeInspectionDiff>,
    pub logs: Vec<StepLogEntry>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionDiff {
    pub parent_id: u32,
    pub hunks: Vec<SnapshotDiffHunk>,
}

impl RuntimeSnapshot {
    pub(crate) fn with_effective_config(
        &self,
        effective: nyanpasu_ipc::api::core::v2::CoreEffectiveConfig,
        host: crate::core::actor_v2::endpoint::ExecutionHost,
        generation: u64,
    ) -> anyhow::Result<Self> {
        let binding = self
            .applied_binding
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("effective snapshot has no successful apply binding"))?;
        anyhow::ensure!(
            binding.revision == effective.revision
                && (binding.host, binding.generation) == (host, generation),
            "effective snapshot does not match the applied build"
        );
        let graph = &self.inspection.graph;
        let (parent, selected_profile_id) = self
            .finalizing_node()
            .ok_or_else(|| anyhow::anyhow!("runtime graph has no finalizing node"))?;
        let config = document_json(&effective.config)?;
        let changed_fields = changed_fields_between(&graph.restore(parent)?, &config)
            .map(|fields| fields.into_iter().collect());
        let mut snapshot = self.clone();
        snapshot.inspection_id = nanoid::nanoid!();
        snapshot.effective = Some(Arc::new(EffectiveInspection {
            config: effective,
            host: (host, generation),
            parent,
            tag: OperatorTag::BuiltinStep {
                selected_profile_id,
                step: BuiltinStepKind::CoreController,
            },
            changed_fields,
        }));
        Ok(snapshot)
    }

    /// The same build without the core's view of it.
    ///
    /// A verified restore stamps a build that already carries an effective
    /// view: the document is the one this build produced, and only the
    /// instance running it has moved. Taking the old view off gives the new
    /// stamp a fresh inspection id. A build that was never stamped is returned
    /// unchanged.
    pub(crate) fn without_effective_config(&self) -> Self {
        let mut snapshot = self.clone();
        if snapshot.effective.take().is_some() {
            snapshot.inspection_id = nanoid::nanoid!();
        }
        snapshot
    }

    /// The node whose config is the built document, with its selected profile.
    fn finalizing_node(&self) -> Option<(Idx, Option<ProfileId>)> {
        self.inspection
            .graph
            .nodes
            .iter()
            .enumerate()
            .find_map(|(id, node)| match &node.tag {
                OperatorTag::BuiltinStep {
                    selected_profile_id,
                    step: BuiltinStepKind::Finalizing,
                } => Some((id as Idx, selected_profile_id.clone())),
                _ => None,
            })
    }

    /// The built document as JSON, restored from the graph rather than
    /// re-parsed from `config_text`, so the view keeps the converter's
    /// semantics instead of acquiring the YAML parser's limits.
    fn config_json(&self) -> serde_json::Value {
        let (finalizing, _) = self
            .finalizing_node()
            .expect("every runtime build ends with a finalizing node");
        restored(self.inspection.graph.restore(finalizing))
    }

    fn has_logs(&self, tag: &OperatorTag) -> bool {
        let key = tag.node_key();
        self.inspection
            .step_logs
            .iter()
            .any(|log| log.key == key && !log.entries.is_empty())
    }

    fn inspection_summary(&self) -> RuntimeInspection {
        let graph = &self.inspection.graph;
        let mut nodes: Vec<_> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(id, node)| RuntimeInspectionNode {
                id: id as u32,
                has_logs: self.has_logs(&node.tag),
                tag: node.tag.clone(),
                next: node.next.clone().unwrap_or_default(),
                changed_fields: node
                    .changed_fields
                    .as_ref()
                    .map(|fields| fields.iter().cloned().collect()),
            })
            .collect();
        // The core's view is projected as the finalizing node's last child.
        if let Some(effective) = &self.effective {
            let id = nodes.len() as u32;
            nodes[effective.parent as usize].next.push(id);
            nodes.push(RuntimeInspectionNode {
                id,
                has_logs: self.has_logs(&effective.tag),
                tag: effective.tag.clone(),
                next: Vec::new(),
                changed_fields: effective.changed_fields.clone(),
            });
        }
        RuntimeInspection {
            snapshot_id: self.inspection_id.clone(),
            applied: false,
            effective_pending: self.applied_binding.is_some() && self.effective.is_none(),
            effective_revision: self
                .effective
                .as_ref()
                .map(|effective| effective.config.revision.clone())
                .or_else(|| {
                    self.applied_binding
                        .as_ref()
                        .map(|binding| binding.revision.clone())
                }),
            revision: self.revision.get().to_string(),
            target_core: self.target_core.to_string(),
            root_id: graph.root_id,
            nodes,
        }
    }

    fn inspection_content(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> Result<RuntimeInspectionContent, RuntimeError> {
        ensure!(
            self.inspection_id == snapshot_id,
            RuntimeSnapshotChangedSnafu
        );
        let graph = &self.inspection.graph;
        let (tag, config, parent) = match &self.effective {
            Some(effective) if node_id as usize == graph.nodes.len() => (
                &effective.tag,
                document_json(&effective.config.config)?,
                Some((effective.parent, restored(graph.restore(effective.parent)))),
            ),
            _ => {
                let node = graph
                    .nodes
                    .get(node_id as usize)
                    .context(RuntimeNodeNotFoundSnafu { node_id })?;
                let (config, parent) = restored(graph.restore_with_parent(node_id));
                (&node.tag, config, parent)
            }
        };
        let yaml =
            serde_yaml::to_string(&redact_config(config)).context(SerializeRuntimeConfigSnafu)?;
        let key = tag.node_key();
        Ok(RuntimeInspectionContent {
            diff: parent
                .map(|(parent_id, parent)| -> Result<_, RuntimeError> {
                    Ok(RuntimeInspectionDiff {
                        parent_id,
                        hunks: nyanpasu_config::runtime::snapshot::ConfigSnapshot::new_unchanged(
                            redact_config(parent),
                        )
                        .diff_yaml_to(&yaml)
                        .context(SerializeRuntimeConfigSnafu)?,
                    })
                })
                .transpose()?,
            yaml,
            logs: self
                .inspection
                .step_logs
                .iter()
                .filter(|log| log.key == key)
                .flat_map(|log| log.entries.iter().cloned())
                .collect(),
        })
    }
}

/// The core's YAML document as the JSON nodes are compared in.
fn document_json(config: &str) -> Result<serde_json::Value, RuntimeError> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(config).context(SerializeRuntimeConfigSnafu)?;
    serde_json::to_value(value).context(ConvertRuntimeConfigSnafu)
}

/// The executor validated the graph and checked every delta's replay while
/// recording it, so a recorded node always restores.
fn restored<T>(result: Result<T, SnapshotBuildError>) -> T {
    result.expect("a recorded runtime graph restores every node")
}

impl NyanpasuClient {
    pub async fn inspect_runtime(&self) -> Option<RuntimeInspection> {
        self.recover_effective_snapshot().await;
        let snapshot = self.promoted_runtime().await?;
        Some(self.inspect_snapshot(&snapshot).await)
    }

    pub async fn inspect_applied_runtime(&self) -> Option<RuntimeInspection> {
        self.recover_effective_snapshot().await;
        let snapshot = self.inner.application_workflow.runtime().applied?;
        Some(self.inspect_snapshot(&snapshot).await)
    }

    /// Inspection recovery is read-only with respect to the core. A newer
    /// successful bind wins even if this query completes after another build.
    async fn recover_effective_snapshot(&self) {
        let store = self.inner.application_workflow.snapshot_store();
        let Some(pending) = store.read().pending else {
            return;
        };
        let Some(binding) = &pending.applied_binding else {
            return;
        };
        let before = self.inner.core_api.status();
        if (before.host, before.generation) != (binding.host, binding.generation) {
            return;
        }
        let Ok(Some(effective)) = self.inner.core_api.effective_config().await else {
            return;
        };
        let after = self.inner.core_api.status();
        if (after.host, after.generation) != (binding.host, binding.generation)
            || effective.revision != binding.revision
        {
            return;
        }
        match pending.with_effective_config(effective, binding.host, binding.generation) {
            Ok(snapshot) => store.applied(&pending.inspection_id, Arc::new(snapshot)),
            Err(error) => tracing::warn!(%error, "effective inspection recovery failed"),
        }
    }

    async fn inspect_snapshot(&self, snapshot: &RuntimeSnapshot) -> RuntimeInspection {
        let mut summary = snapshot.inspection_summary();
        if let Some(effective) = &snapshot.effective {
            let before = self.inner.core_api.status();
            let current = self.inner.core_api.effective_config().await.ok().flatten();
            let after = self.inner.core_api.status();
            summary.applied = effective.host == (before.host, before.generation)
                && (before.host, before.generation) == (after.host, after.generation)
                && current.is_some_and(|current| current.revision == effective.config.revision);
        }
        summary
    }

    pub async fn inspect_runtime_node(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> Result<RuntimeInspectionContent, RuntimeError> {
        let state = self.inner.application_workflow.runtime();
        let snapshot = [state.promoted, state.applied]
            .into_iter()
            .flatten()
            .find(|snapshot| snapshot.inspection_id == snapshot_id)
            .context(RuntimeSnapshotChangedSnafu)?;
        let snapshot_id = snapshot_id.to_owned();
        crate::utils::blocking::join(
            tokio::task::spawn_blocking(move || snapshot.inspection_content(&snapshot_id, node_id))
                .await,
        )
    }

    /// The promoted runtime configuration as the JSON the frontend renders.
    pub async fn runtime_config(&self) -> Result<Option<serde_json::Value>, RuntimeError> {
        let Some(state) = self.promoted_runtime().await else {
            return Ok(None);
        };
        Ok(Some(state.config_json()))
    }

    /// The promoted runtime configuration as YAML text.
    pub async fn runtime_yaml(&self) -> Result<String, RuntimeError> {
        let state = self
            .promoted_runtime()
            .await
            .context(NoRuntimeConfigSnafu)?;
        Ok(state.config_text.to_string())
    }
}

fn redact_config(mut value: serde_json::Value) -> serde_json::Value {
    match &mut value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "secret" | "password" | "token" | "private-key" | "auth-str" | "authentication"
                ) {
                    *value = serde_json::Value::String("<redacted>".into());
                } else if matches!(
                    key.as_str(),
                    "external-controller" | "external-controller-tls"
                ) {
                    if let Some(address) = value.as_str()
                        && let Some((_, authority)) = address.rsplit_once('@')
                    {
                        *value = serde_json::Value::String(format!("<redacted>@{authority}"));
                    }
                } else {
                    *value = redact_config(std::mem::take(value));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                *value = redact_config(std::mem::take(value));
            }
        }
        _ => {}
    }
    value
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        client::runtime::{RuntimeRevisionAllocator, RuntimeSnapshotData},
        enhance::PostProcessingOutput,
    };
    use nyanpasu_config::{
        application::ClashCore,
        runtime::{executor::StepLogLevel, snapshot::ConfigSnapshotsBuilder, value::ConfigValue},
    };
    use std::sync::Arc;

    pub(crate) fn inspection_data() -> RuntimeInspectionData {
        let graph = ConfigSnapshotsBuilder::new_root(
            Arc::new(
                serde_json::from_value::<ConfigValue>(serde_json::json!({"mode": "rule"})).unwrap(),
            ),
            OperatorTag::BareRoot,
        )
        .build_stored()
        .unwrap();
        let key = graph.nodes[0].tag.node_key();
        RuntimeInspectionData {
            graph,
            step_logs: vec![StepLog {
                key,
                entries: vec![StepLogEntry::new(StepLogLevel::Info, "built")],
            }],
        }
    }

    fn snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot::from_data(
            RuntimeRevisionAllocator::new().allocate(),
            ClashCore::default(),
            RuntimeSnapshotData {
                config_text: Arc::from("mode: rule\n"),
                exists_keys: Vec::new(),
                postprocessing_output: PostProcessingOutput::default(),
                inspection: Arc::new(inspection_data()),
            },
        )
    }

    #[test]
    fn effective_config_is_a_separate_node_and_store_rejects_stale_reports() {
        use crate::client::runtime::RuntimeSnapshotStore;
        use nyanpasu_config::runtime::snapshot::BuiltinStepKind;
        let input =
            serde_json::json!({"external-controller": "127.0.0.1:9090", "secret": "private"});
        let value = Arc::new(serde_json::from_value::<ConfigValue>(input.clone()).unwrap());
        let mut graph = ConfigSnapshotsBuilder::new_root(value.clone(), OperatorTag::BareRoot);
        graph
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                value,
            )
            .unwrap();
        let mut generated = snapshot();
        generated.config_text = Arc::from("external-controller: 127.0.0.1:9090\nsecret: private\n");
        generated.inspection = Arc::new(RuntimeInspectionData {
            graph: graph.build_stored().unwrap(),
            step_logs: vec![],
        });
        let effective = nyanpasu_ipc::api::core::v2::CoreEffectiveConfig {
            instance_id: "instance-1".into(),
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            config: "external-controller-unix: /tmp/managed.sock\nsecret: private\n".into(),
        };
        generated.applied_binding = Some(crate::core::actor_v2::facade::AppliedConfigBinding {
            revision: effective.revision.clone(),
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            generation: 1,
        });
        let applied = generated
            .with_effective_config(
                effective,
                crate::core::actor_v2::endpoint::ExecutionHost::Local,
                1,
            )
            .unwrap();
        assert_eq!(
            generated.config_text, applied.config_text,
            "next rebuild must use the original source"
        );
        assert!(
            Arc::ptr_eq(&generated.inspection, &applied.inspection),
            "stamping must not copy the build's graph"
        );
        let summary = applied.inspection_summary();
        assert_eq!(summary.nodes.len(), 3);
        assert_eq!(summary.nodes[1].next, [2]);
        assert_eq!(
            summary.nodes[2].changed_fields,
            Some(vec![
                "external-controller-unix".to_string(),
                "external-controller".to_string()
            ])
        );
        let unstamped = applied.without_effective_config();
        assert_ne!(unstamped.inspection_id, applied.inspection_id);
        assert_eq!(unstamped.inspection_summary().nodes.len(), 2);
        assert!(
            unstamped
                .inspection_content(&unstamped.inspection_id, 2)
                .is_err()
        );
        let node = applied
            .inspection_content(&applied.inspection_id, 2)
            .unwrap();
        assert!(node.yaml.contains("/tmp/managed.sock"));
        assert!(!node.yaml.contains("private"));
        assert!(node.diff.is_some());
        assert!(!format!("{:?}", node.diff).contains("private"));
        let store = RuntimeSnapshotStore::default();
        store.generated(Arc::new(generated.clone()));
        store.bind_applied(Arc::new(generated.clone()));
        store.applied("stale-build", Arc::new(applied.clone()));
        assert!(store.read().applied.is_none());
        store.applied(&generated.inspection_id, Arc::new(applied.clone()));
        assert!(store.read().applied.is_some());
        store.generated(Arc::new(generated.clone()));
        assert!(
            store.read().applied.is_some(),
            "a rejected candidate must retain the last applied view"
        );
        let mut next = generated.clone();
        next.inspection_id = "new-success".into();
        next.applied_binding.as_mut().unwrap().revision.generation += 1;
        let next = Arc::new(next);
        store.generated(next.clone());
        store.bind_applied(next.clone());
        store.applied(&generated.inspection_id, Arc::new(applied.clone()));
        assert_eq!(
            store.read().pending.as_ref().unwrap().inspection_id,
            "new-success"
        );
        let mut candidate = generated.clone();
        candidate.inspection_id = "new-failed-build".into();
        candidate.applied_binding = None;
        store.generated(Arc::new(candidate));
        let mut effective = applied.effective.as_ref().unwrap().config.clone();
        assert!(
            next.with_effective_config(
                effective.clone(),
                crate::core::actor_v2::endpoint::ExecutionHost::Local,
                1
            )
            .is_err()
        );
        effective.revision = next.applied_binding.as_ref().unwrap().revision.clone();
        assert!(
            next.with_effective_config(
                effective.clone(),
                crate::core::actor_v2::endpoint::ExecutionHost::Local,
                2
            )
            .is_err()
        );
        let recovered = next
            .with_effective_config(
                effective,
                crate::core::actor_v2::endpoint::ExecutionHost::Local,
                1,
            )
            .unwrap();
        store.applied(&next.inspection_id, Arc::new(recovered));
        assert!(store.read().pending.is_none());
        assert_eq!(
            store.read().promoted.unwrap().inspection_id,
            "new-failed-build"
        );
        assert_eq!(
            store.read().applied.unwrap().applied_binding,
            next.applied_binding
        );
    }

    /// The JSON view is the built document itself; nesting deeper than the
    /// YAML parser accepts must not make it unavailable.
    #[test]
    fn config_json_is_the_finalizing_document_at_any_depth() {
        let mut nested = serde_json::json!("leaf");
        for _ in 0..200 {
            nested = serde_json::json!({ "inner": nested });
        }
        let document = serde_json::json!({ "mode": "rule", "extra": nested });
        let root = Arc::new(serde_json::from_value::<ConfigValue>(document.clone()).unwrap());
        let mut graph = ConfigSnapshotsBuilder::new_root(
            Arc::new(serde_json::from_value(serde_json::json!({"mode": "rule"})).unwrap()),
            OperatorTag::BareRoot,
        );
        graph
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                root,
            )
            .unwrap();
        let mut snapshot = snapshot();
        snapshot.inspection = Arc::new(RuntimeInspectionData {
            graph: graph.build_stored().unwrap(),
            step_logs: vec![],
        });
        assert_eq!(snapshot.config_json(), document);
    }

    /// A snapshot is cloned on every apply; no clone may copy a document.
    #[test]
    fn cloning_a_stamped_snapshot_shares_every_document() {
        let value = Arc::new(
            serde_json::from_value::<ConfigValue>(serde_json::json!({"mode": "rule"})).unwrap(),
        );
        let mut graph = ConfigSnapshotsBuilder::new_root(value.clone(), OperatorTag::BareRoot);
        graph
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                value,
            )
            .unwrap();
        let mut generated = snapshot();
        generated.inspection = Arc::new(RuntimeInspectionData {
            graph: graph.build_stored().unwrap(),
            step_logs: vec![],
        });
        let revision = nyanpasu_ipc::api::status::ConfigRevisionInfo {
            epoch: 1,
            generation: 1,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        generated.applied_binding = Some(crate::core::actor_v2::facade::AppliedConfigBinding {
            revision: revision.clone(),
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            generation: 1,
        });
        let stamped = generated
            .with_effective_config(
                nyanpasu_ipc::api::core::v2::CoreEffectiveConfig {
                    instance_id: "instance-1".into(),
                    revision,
                    config: "mode: global\n".into(),
                },
                crate::core::actor_v2::endpoint::ExecutionHost::Local,
                1,
            )
            .unwrap();
        let copy = stamped.clone();
        assert!(Arc::ptr_eq(&copy.config_text, &stamped.config_text));
        assert!(Arc::ptr_eq(&copy.inspection, &stamped.inspection));
        assert!(Arc::ptr_eq(
            copy.effective.as_ref().unwrap(),
            stamped.effective.as_ref().unwrap()
        ));
    }

    #[test]
    fn inspection_projects_metadata_and_selected_content() {
        let snapshot = snapshot();
        let summary = snapshot.inspection_summary();
        assert_eq!(summary.root_id, 0);
        assert_eq!(summary.revision, "1");
        assert_eq!(summary.nodes.len(), 1);
        assert_eq!(summary.nodes[0].tag, OperatorTag::BareRoot);
        assert_eq!(summary.nodes[0].changed_fields, None);
        assert!(summary.nodes[0].has_logs);
        let content = snapshot
            .inspection_content(&summary.snapshot_id, 0)
            .unwrap();
        assert_eq!(
            serde_yaml::from_str::<serde_json::Value>(&content.yaml).unwrap(),
            serde_json::json!({"mode": "rule"})
        );
        assert_eq!(content.logs[0].message, "built");
        assert!(content.diff.is_none());
    }

    #[test]
    fn inspection_preserves_branch_links_and_change_baselines() {
        use nyanpasu_config::{
            profile::ProfileId,
            runtime::snapshot::{BuiltinStepKind, ConfigExecutionRole},
        };
        let value = |mode| {
            Arc::new(
                serde_json::from_value::<ConfigValue>(serde_json::json!({"mode": mode})).unwrap(),
            )
        };
        let mut builder = ConfigSnapshotsBuilder::new_root(value("rule"), OperatorTag::BareRoot);
        builder
            .attach_independent_branch(
                builder.root_node_id(),
                ConfigSnapshotsBuilder::new_root(
                    value("direct"),
                    OperatorTag::FileConfigRoot {
                        profile_id: ProfileId("source".into()),
                        role: ConfigExecutionRole::CompositionContributor {
                            composition_id: ProfileId("combined".into()),
                            contributor_index: 0,
                        },
                    },
                ),
            )
            .unwrap();
        builder
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                value("global"),
            )
            .unwrap();
        let mut snapshot = snapshot();
        snapshot.inspection = Arc::new(RuntimeInspectionData {
            graph: builder.build_stored().unwrap(),
            step_logs: Vec::new(),
        });
        let summary = snapshot.inspection_summary();
        let root = &summary.nodes[summary.root_id as usize];
        assert_eq!(root.next.len(), 2);
        for child in &root.next {
            let node = &summary.nodes[*child as usize];
            let content = snapshot
                .inspection_content(&summary.snapshot_id, *child)
                .unwrap();
            let config: serde_json::Value = serde_yaml::from_str(&content.yaml).unwrap();
            assert!(content.logs.is_empty());
            assert!(!node.has_logs);
            match node.tag {
                OperatorTag::FileConfigRoot { .. } => {
                    assert_eq!(node.changed_fields, None);
                    assert_eq!(config["mode"], "direct");
                    assert!(content.diff.is_none());
                }
                OperatorTag::BuiltinStep { .. } => {
                    assert_eq!(node.changed_fields, Some(vec!["mode".to_string()]));
                    assert_eq!(config["mode"], "global");
                    let diff = content.diff.unwrap();
                    assert_eq!(diff.parent_id, summary.root_id);
                    assert_eq!(diff.hunks[0].lines, ["-mode: rule", "+mode: global"]);
                }
                _ => panic!("unexpected child"),
            }
        }
    }

    #[test]
    fn inspection_does_not_count_empty_log_entries_as_activity() {
        let mut snapshot = snapshot();
        let mut data = inspection_data();
        data.step_logs[0].entries.clear();
        data.step_logs.push(StepLog {
            key: nyanpasu_config::runtime::snapshot::SnapshotNodeKey::Builtin {
                selected_profile_id: None,
                step: nyanpasu_config::runtime::snapshot::BuiltinStepKind::Finalizing,
            },
            entries: vec![StepLogEntry::new(StepLogLevel::Info, "another step")],
        });
        snapshot.inspection = Arc::new(data);
        assert!(!snapshot.inspection_summary().nodes[0].has_logs);
    }

    #[test]
    fn inspection_rejects_replaced_snapshot_and_missing_node() {
        let first = snapshot();
        let second = snapshot();
        // Even identical products/revisions from different lifetimes cannot alias.
        assert!(second.inspection_content(&first.inspection_id, 0).is_err());
        assert!(
            first
                .inspection_content(&first.inspection_id, u32::MAX)
                .is_err()
        );
    }
}
