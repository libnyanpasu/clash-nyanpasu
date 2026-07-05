//! A snapshot of the clash config processing.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use indexmap::IndexSet;
use json_patch::{Patch, PatchOperation};
use serde::{Deserialize, Serialize};
use slab::Slab;
use thiserror::Error;

use crate::{
    profile::{ProfileId, TransformKind},
    runtime::value::ConfigValue,
};

/// A field in the config, use `.` to represent nested fields.
pub type ConfigField = String;
/// Compact node identifier used by the persisted/materialized graphs.
pub type Idx = u32;
/// Transient node identifier used while building inside the slab.
pub type NodeId = usize;

/// Upper bound on snapshot-tree depth, guarding the recursive materializer
/// against stack overflow when fed an untrusted (deserialized) graph.
const MAX_MATERIALIZE_DEPTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ConfigSnapshot {
    /// Materialized snapshot of the config.
    pub config: serde_json::Value,
    /// The changed fields compared to the previous snapshot.
    pub changed_fields: Option<IndexSet<ConfigField>>,
}

impl ConfigSnapshot {
    pub fn new(config: serde_json::Value, changed_fields: Option<IndexSet<ConfigField>>) -> Self {
        Self {
            config,
            changed_fields,
        }
    }

    pub fn new_unchanged(config: serde_json::Value) -> Self {
        Self {
            config,
            changed_fields: None,
        }
    }
}

/// Why a config pipeline is being executed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ConfigExecutionRole {
    /// The final config selected by `Profiles.current`.
    Selected,
    /// Built as the base member of a composition.
    CompositionBase { composition_id: ProfileId },
    /// Built as a proxies contributor of a composition.
    CompositionContributor {
        composition_id: ProfileId,
        contributor_index: u32,
    },
}

/// Built-in post-processing steps applied to the selected config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinStepKind {
    GuardOverrides,
    WhitelistFieldFilter,
    Finalizing,
}

/// The pipeline operator that produced a snapshot node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum OperatorTag {
    FileConfigRoot {
        profile_id: ProfileId,
        role: ConfigExecutionRole,
    },
    /// `base: None` is the clean seed (`proxies: []`).
    CompositionRoot {
        profile_id: ProfileId,
        base: Option<ProfileId>,
    },
    ExtendProxiesStep {
        composition_id: ProfileId,
        contributor_profile_id: ProfileId,
        contributor_index: u32,
    },
    ScopedTransform {
        host_profile_id: ProfileId,
        role: ConfigExecutionRole,
        transform_profile_id: ProfileId,
        transform_kind: TransformKind,
        step_index: u32,
    },
    /// `selected_profile_id: None` = bare 模式（current 为空，spec §2 目标 6）。
    GlobalTransform {
        selected_profile_id: Option<ProfileId>,
        transform_profile_id: ProfileId,
        transform_kind: TransformKind,
        step_index: u32,
    },
    BuiltinStep {
        selected_profile_id: Option<ProfileId>,
        step: BuiltinStepKind,
    },
    /// current = None 的裸配置管线根（spec §8.2）。
    BareRoot,
    /// 内建增强脚本步骤；`name` 为展示性字段，`node_key()` 丢弃。
    BuiltinTransform {
        selected_profile_id: Option<ProfileId>,
        name: String,
        step_index: u32,
    },
}

impl OperatorTag {
    /// Derives the semantic position key of this tag. Never stored; the
    /// materialized graph computes it on demand.
    pub fn node_key(&self) -> SnapshotNodeKey {
        match self {
            Self::FileConfigRoot { profile_id, role } => SnapshotNodeKey::FileRoot {
                profile_id: profile_id.clone(),
                role: role.clone(),
            },
            Self::CompositionRoot { profile_id, .. } => SnapshotNodeKey::CompositionRoot {
                profile_id: profile_id.clone(),
            },
            Self::ExtendProxiesStep {
                composition_id,
                contributor_index,
                ..
            } => SnapshotNodeKey::ExtendProxies {
                composition_id: composition_id.clone(),
                contributor_index: *contributor_index,
            },
            Self::ScopedTransform {
                host_profile_id,
                role,
                step_index,
                ..
            } => SnapshotNodeKey::ScopedTransform {
                host_profile_id: host_profile_id.clone(),
                role: role.clone(),
                step_index: *step_index,
            },
            Self::GlobalTransform {
                selected_profile_id,
                step_index,
                ..
            } => SnapshotNodeKey::GlobalTransform {
                selected_profile_id: selected_profile_id.clone(),
                step_index: *step_index,
            },
            Self::BuiltinStep {
                selected_profile_id,
                step,
            } => SnapshotNodeKey::Builtin {
                selected_profile_id: selected_profile_id.clone(),
                step: *step,
            },
            Self::BareRoot => SnapshotNodeKey::BareRoot,
            Self::BuiltinTransform {
                selected_profile_id,
                step_index,
                ..
            } => SnapshotNodeKey::BuiltinTransform {
                selected_profile_id: selected_profile_id.clone(),
                step_index: *step_index,
            },
        }
    }
}

/// Semantic position key: the tag stripped of display-only fields. Stable
/// across rebuilds, so the UI can anchor onto nodes; carries `Eq + Hash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum SnapshotNodeKey {
    FileRoot {
        profile_id: ProfileId,
        role: ConfigExecutionRole,
    },
    CompositionRoot {
        profile_id: ProfileId,
    },
    ExtendProxies {
        composition_id: ProfileId,
        contributor_index: u32,
    },
    ScopedTransform {
        host_profile_id: ProfileId,
        role: ConfigExecutionRole,
        step_index: u32,
    },
    GlobalTransform {
        selected_profile_id: Option<ProfileId>,
        step_index: u32,
    },
    Builtin {
        selected_profile_id: Option<ProfileId>,
        step: BuiltinStepKind,
    },
    BareRoot,
    BuiltinTransform {
        selected_profile_id: Option<ProfileId>,
        step_index: u32,
    },
}

/// Controls what a node's payload is encoded against during materialization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotBaseline {
    /// Keyframe/delta encoded against the parent node; materialization
    /// derives `changed_fields`.
    #[default]
    Parent,
    /// Independent branch root: forced `Full` keyframe, materialized
    /// `changed_fields = None`.
    Independent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ConfigSnapshotState<C> {
    pub snapshot: ConfigSnapshot,
    /// The operator that generated this snapshot.
    pub tag: OperatorTag,
    /// Semantic position key derived from `tag` at materialization time.
    pub key: SnapshotNodeKey,
    pub next: Option<Vec<C>>,
}

impl<C> ConfigSnapshotState<C> {
    pub fn new(snapshot: ConfigSnapshot, tag: OperatorTag, next: Option<Vec<C>>) -> Self {
        let key = tag.node_key();
        Self {
            snapshot,
            tag,
            key,
            next,
        }
    }
}

/// Front-end facing graph: every node carries a fully materialized config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ConfigSnapshotsGraph {
    pub nodes: Vec<ConfigSnapshotState<Idx>>,
    pub root_id: Idx,
}

/// Storage payload: keyframe (`Full`) or relative `Delta` against the parent node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum SnapshotPayload {
    Full(Arc<ConfigValue>),
    Delta(Patch),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredConfigSnapshot {
    pub payload: SnapshotPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredConfigSnapshotState<C> {
    pub snapshot: StoredConfigSnapshot,
    pub tag: OperatorTag,
    pub baseline: SnapshotBaseline,
    pub next: Option<Vec<C>>,
}

impl<C> StoredConfigSnapshotState<C> {
    pub fn new(
        snapshot: StoredConfigSnapshot,
        tag: OperatorTag,
        baseline: SnapshotBaseline,
        next: Option<Vec<C>>,
    ) -> Self {
        Self {
            snapshot,
            tag,
            baseline,
            next,
        }
    }
}

/// Storage graph: keyframe/delta encoded, no redundant edge list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredConfigSnapshotsGraph {
    pub nodes: Vec<StoredConfigSnapshotState<Idx>>,
    pub root_id: Idx,
}

pub struct ConfigSnapshotTreeNode {
    pub full: Arc<ConfigValue>,
    pub node: StoredConfigSnapshotState<ConfigSnapshotTreeNode>,
}

/// Controls when a node is stored as a keyframe instead of a delta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyframePolicy {
    /// Store a `Full` keyframe once the serialized patch exceeds this fraction
    /// of the serialized full value.
    pub delta_to_full_ratio: f32,
}

impl Default for KeyframePolicy {
    fn default() -> Self {
        Self {
            delta_to_full_ratio: 0.50,
        }
    }
}

impl KeyframePolicy {
    fn encode(&self, parent: &ConfigValue, current: Arc<ConfigValue>) -> SnapshotPayload {
        let parent_json = parent.to_json();
        let current_json = current.to_json();
        let patch = json_patch::diff(&parent_json, &current_json);
        let patch_len = serialized_len(&patch);
        let full_len = serialized_len(&current_json).max(1);

        if (patch_len as f32) > (full_len as f32 * self.delta_to_full_ratio) {
            SnapshotPayload::Full(current)
        } else {
            SnapshotPayload::Delta(patch)
        }
    }
}

#[derive(Debug, Error)]
pub enum SnapshotBuildError {
    #[error("root node {root_id} not found")]
    MissingRoot { root_id: usize },
    #[error("child node {child_id} referenced by {parent_id} is missing")]
    MissingChild { parent_id: usize, child_id: usize },
    #[error("node {node_id} has multiple parents")]
    MultipleParents { node_id: usize },
    #[error("cycle detected at node {node_id}")]
    Cycle { node_id: usize },
    #[error("unreachable nodes present: {node_ids:?}")]
    Unreachable { node_ids: Vec<usize> },
    #[error("node id {node_id} exceeds u32")]
    IdOverflow { node_id: usize },
    #[error("snapshot graph depth {depth} exceeds maximum {max}")]
    DepthLimitExceeded { depth: usize, max: usize },
    #[error("independent snapshot node {node_id} must carry a full payload")]
    IndependentDelta { node_id: usize },
    #[error("root node {root_id} must use the independent baseline")]
    RootNotIndependent { root_id: usize },
    #[error(transparent)]
    Patch(#[from] json_patch::PatchError),
}

#[derive(Debug, Clone)]
struct BuilderNode {
    /// Cached full value, kept for O(1) clone during keyframe/delta encoding.
    full: Arc<ConfigValue>,
    state: StoredConfigSnapshotState<NodeId>,
    parent_id: Option<NodeId>,
    children: Vec<NodeId>,
}

/// Pure recorder for the pipeline executor: appends main-line nodes with
/// [`Self::push`] and grafts pre-built branches with
/// [`Self::attach_independent_branch`]. Tags are constructed by the caller.
pub struct ConfigSnapshotsBuilder {
    slab: Slab<BuilderNode>,
    root_id: NodeId,
    /// The current processing snapshot id.
    current_id: NodeId,
    keyframe_policy: KeyframePolicy,
}

impl ConfigSnapshotsBuilder {
    pub fn new_root(root_value: Arc<ConfigValue>, tag: OperatorTag) -> Self {
        Self::new_root_with_keyframe_policy(root_value, tag, KeyframePolicy::default())
    }

    pub fn new_root_with_keyframe_policy(
        root_value: Arc<ConfigValue>,
        tag: OperatorTag,
        keyframe_policy: KeyframePolicy,
    ) -> Self {
        let mut slab = Slab::new();
        let root_state = StoredConfigSnapshotState {
            snapshot: StoredConfigSnapshot {
                payload: SnapshotPayload::Full(root_value.clone()),
            },
            tag,
            baseline: SnapshotBaseline::Independent,
            next: None,
        };

        let root_id = slab.insert(BuilderNode {
            full: root_value,
            state: root_state,
            parent_id: None,
            children: Vec::new(),
        });

        Self {
            slab,
            root_id,
            current_id: root_id,
            keyframe_policy,
        }
    }

    pub fn current_node_id(&self) -> NodeId {
        self.current_id
    }

    pub fn root_node_id(&self) -> NodeId {
        self.root_id
    }

    /// Appends a main-line node after the current node and advances the
    /// current position onto it.
    pub fn push(
        &mut self,
        tag: OperatorTag,
        value: Arc<ConfigValue>,
    ) -> Result<NodeId, SnapshotBuildError> {
        let parent_id = self.current_id;
        let parent = self
            .slab
            .get(parent_id)
            .ok_or(SnapshotBuildError::MissingRoot { root_id: parent_id })?;
        let payload = self.keyframe_policy.encode(&parent.full, value.clone());
        let id = self.insert_child(parent_id, tag, value, SnapshotBaseline::Parent, payload);
        self.current_id = id;
        Ok(id)
    }

    /// Grafts a whole branch builder under `parent`. The branch root becomes
    /// an [`SnapshotBaseline::Independent`] node with a forced `Full` payload
    /// (no delta against the new parent); inner branch nodes keep their
    /// encoding. The main-line current position is left untouched. Returns the
    /// grafted branch-root id.
    pub fn attach_independent_branch(
        &mut self,
        parent_id: NodeId,
        branch: ConfigSnapshotsBuilder,
    ) -> Result<NodeId, SnapshotBuildError> {
        if self.slab.get(parent_id).is_none() {
            return Err(SnapshotBuildError::MissingRoot { root_id: parent_id });
        }
        branch.validate_tree()?;

        let ordered_ids = branch.slab.iter().map(|(idx, _)| idx).collect::<Vec<_>>();
        let mut id_map = HashMap::with_capacity(ordered_ids.len());
        for &old_id in &ordered_ids {
            let node = &branch.slab[old_id];
            let mut state = node.state.clone();
            if old_id == branch.root_id {
                state.baseline = SnapshotBaseline::Independent;
                state.snapshot.payload = SnapshotPayload::Full(node.full.clone());
            }

            let new_id = self.slab.insert(BuilderNode {
                full: node.full.clone(),
                state,
                parent_id: None,
                children: Vec::new(),
            });
            id_map.insert(old_id, new_id);
        }

        // The branch passed `validate_tree`, so every child id resolves.
        for &old_id in &ordered_ids {
            let new_id = id_map[&old_id];
            for child in &branch.slab[old_id].children {
                let new_child = id_map[child];
                self.slab[new_id].children.push(new_child);
                self.slab[new_child].parent_id = Some(new_id);
            }
        }

        let branch_root = id_map[&branch.root_id];
        self.slab[parent_id].children.push(branch_root);
        self.slab[branch_root].parent_id = Some(parent_id);
        Ok(branch_root)
    }

    pub fn build_tree(mut self) -> Result<ConfigSnapshotTreeNode, SnapshotBuildError> {
        self.validate_tree()?;
        Ok(Self::build_tree_node(&mut self.slab, self.root_id))
    }

    pub fn build_stored(self) -> Result<StoredConfigSnapshotsGraph, SnapshotBuildError> {
        self.validate_tree()?;
        self.into_compact_u32_graph()
    }

    pub fn build(self) -> Result<ConfigSnapshotsGraph, SnapshotBuildError> {
        self.build_stored()?.materialize()
    }

    fn insert_child(
        &mut self,
        parent_id: NodeId,
        tag: OperatorTag,
        current: Arc<ConfigValue>,
        baseline: SnapshotBaseline,
        payload: SnapshotPayload,
    ) -> NodeId {
        let id = self.slab.insert(BuilderNode {
            full: current,
            state: StoredConfigSnapshotState {
                snapshot: StoredConfigSnapshot { payload },
                tag,
                baseline,
                next: None,
            },
            parent_id: Some(parent_id),
            children: Vec::new(),
        });
        self.slab[parent_id].children.push(id);
        id
    }

    fn build_tree_node(slab: &mut Slab<BuilderNode>, id: NodeId) -> ConfigSnapshotTreeNode {
        let BuilderNode {
            full,
            state,
            children,
            parent_id: _,
        } = slab.remove(id);

        let next = if children.is_empty() {
            None
        } else {
            Some(
                children
                    .into_iter()
                    .map(|child_id| Self::build_tree_node(slab, child_id))
                    .collect(),
            )
        };

        ConfigSnapshotTreeNode {
            full,
            node: StoredConfigSnapshotState {
                snapshot: state.snapshot,
                tag: state.tag,
                baseline: state.baseline,
                next,
            },
        }
    }

    fn validate_tree(&self) -> Result<(), SnapshotBuildError> {
        let node_ids = self.slab.iter().map(|(idx, _)| idx).collect::<Vec<_>>();
        if !node_ids.contains(&self.root_id) {
            return Err(SnapshotBuildError::MissingRoot {
                root_id: self.root_id,
            });
        }

        let mut indegree: HashMap<NodeId, usize> = HashMap::new();
        for (idx, node) in self.slab.iter() {
            for &child in &node.children {
                if self.slab.get(child).is_none() {
                    return Err(SnapshotBuildError::MissingChild {
                        parent_id: idx,
                        child_id: child,
                    });
                }

                if child == idx {
                    return Err(SnapshotBuildError::Cycle { node_id: idx });
                }

                let count = indegree.entry(child).or_insert(0);
                *count += 1;
                if *count > 1 {
                    return Err(SnapshotBuildError::MultipleParents { node_id: child });
                }
            }
        }

        self.validate_acyclic(&node_ids)?;

        let roots = node_ids
            .iter()
            .copied()
            .filter(|id| indegree.get(id).copied().unwrap_or(0) == 0)
            .collect::<Vec<_>>();
        if !roots.contains(&self.root_id) {
            return Err(SnapshotBuildError::MissingRoot {
                root_id: self.root_id,
            });
        }
        if roots.len() != 1 {
            return Err(SnapshotBuildError::Unreachable {
                node_ids: roots.into_iter().filter(|id| *id != self.root_id).collect(),
            });
        }

        let mut reachable = IndexSet::new();
        let mut queue = VecDeque::from_iter([self.root_id]);
        while let Some(id) = queue.pop_front() {
            if !reachable.insert(id) {
                continue;
            }

            queue.extend(self.slab[id].children.iter().copied());
        }

        if reachable.len() != self.slab.len() {
            let unreachable = node_ids
                .into_iter()
                .filter(|id| !reachable.contains(id))
                .collect::<Vec<_>>();
            return Err(SnapshotBuildError::Unreachable {
                node_ids: unreachable,
            });
        }

        Ok(())
    }

    fn validate_acyclic(&self, node_ids: &[NodeId]) -> Result<(), SnapshotBuildError> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum VisitState {
            Unvisited,
            Visiting,
            Done,
        }

        fn dfs(
            idx: NodeId,
            slab: &Slab<BuilderNode>,
            state: &mut HashMap<NodeId, VisitState>,
        ) -> Result<(), SnapshotBuildError> {
            match state.get(&idx).copied() {
                Some(VisitState::Visiting) => {
                    return Err(SnapshotBuildError::Cycle { node_id: idx });
                }
                Some(VisitState::Done) => return Ok(()),
                _ => {}
            }

            state.insert(idx, VisitState::Visiting);
            for &child in &slab[idx].children {
                dfs(child, slab, state)?;
            }
            state.insert(idx, VisitState::Done);
            Ok(())
        }

        let mut state = node_ids
            .iter()
            .map(|&idx| (idx, VisitState::Unvisited))
            .collect::<HashMap<_, _>>();
        for &id in node_ids {
            dfs(id, &self.slab, &mut state)?;
        }
        Ok(())
    }

    fn into_compact_u32_graph(self) -> Result<StoredConfigSnapshotsGraph, SnapshotBuildError> {
        let ordered_ids = self.slab.iter().map(|(idx, _)| idx).collect::<Vec<_>>();
        let mut id_map = HashMap::with_capacity(ordered_ids.len());
        for (compact, old_id) in ordered_ids.iter().copied().enumerate() {
            id_map.insert(old_id, usize_to_idx(compact)?);
        }

        let root_id = *id_map
            .get(&self.root_id)
            .ok_or(SnapshotBuildError::MissingRoot {
                root_id: self.root_id,
            })?;
        let mut nodes = Vec::with_capacity(self.slab.len());

        for old_id in ordered_ids {
            let builder_node = &self.slab[old_id];
            let next = if builder_node.children.is_empty() {
                None
            } else {
                Some(
                    builder_node
                        .children
                        .iter()
                        .map(|child| {
                            id_map
                                .get(child)
                                .copied()
                                .ok_or(SnapshotBuildError::MissingChild {
                                    parent_id: old_id,
                                    child_id: *child,
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )
            };

            nodes.push(StoredConfigSnapshotState {
                snapshot: builder_node.state.snapshot.clone(),
                tag: builder_node.state.tag.clone(),
                baseline: builder_node.state.baseline,
                next,
            });
        }

        Ok(StoredConfigSnapshotsGraph { nodes, root_id })
    }
}

impl StoredConfigSnapshotsGraph {
    /// Materializes every node into a fully expanded [`ConfigSnapshotsGraph`],
    /// applying deltas against their parents while walking the tree.
    pub fn materialize(&self) -> Result<ConfigSnapshotsGraph, SnapshotBuildError> {
        self.validate_tree_shape()?;
        let mut nodes = vec![None; self.nodes.len()];
        self.materialize_node(self.root_id, None, 0, &mut nodes)?;

        let nodes = nodes
            .into_iter()
            .enumerate()
            .map(|(idx, node)| {
                node.ok_or(SnapshotBuildError::Unreachable {
                    node_ids: vec![idx],
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ConfigSnapshotsGraph {
            nodes,
            root_id: self.root_id,
        })
    }

    fn materialize_node(
        &self,
        id: Idx,
        parent: Option<&serde_json::Value>,
        depth: usize,
        nodes: &mut [Option<ConfigSnapshotState<Idx>>],
    ) -> Result<serde_json::Value, SnapshotBuildError> {
        if depth > MAX_MATERIALIZE_DEPTH {
            return Err(SnapshotBuildError::DepthLimitExceeded {
                depth,
                max: MAX_MATERIALIZE_DEPTH,
            });
        }

        let index = id as usize;
        let state = self
            .nodes
            .get(index)
            .ok_or(SnapshotBuildError::MissingChild {
                parent_id: index,
                child_id: index,
            })?;

        let (config, changed_fields) = match (state.baseline, &state.snapshot.payload) {
            (SnapshotBaseline::Independent, SnapshotPayload::Full(value)) => {
                (value.to_json(), None)
            }
            (SnapshotBaseline::Independent, SnapshotPayload::Delta(_)) => {
                return Err(SnapshotBuildError::IndependentDelta { node_id: index });
            }
            (SnapshotBaseline::Parent, SnapshotPayload::Full(value)) => {
                let config = value.to_json();
                let changed_fields = parent
                    .map(|parent| json_patch::diff(parent, &config))
                    .and_then(|patch| changed_fields_from_patch(&patch));
                (config, changed_fields)
            }
            (SnapshotBaseline::Parent, SnapshotPayload::Delta(patch)) => {
                let mut config = parent
                    .cloned()
                    .ok_or(SnapshotBuildError::MissingRoot { root_id: index })?;
                json_patch::patch(&mut config, patch)?;
                (config, changed_fields_from_patch(patch))
            }
        };

        let next = state.next.clone();
        nodes[index] = Some(ConfigSnapshotState {
            snapshot: ConfigSnapshot {
                config: config.clone(),
                changed_fields,
            },
            tag: state.tag.clone(),
            key: state.tag.node_key(),
            next: next.clone(),
        });

        for child in next.unwrap_or_default() {
            self.materialize_node(child, Some(&config), depth + 1, nodes)?;
        }

        Ok(config)
    }

    /// Validates that a (possibly deserialized) stored graph is a single-rooted
    /// tree: valid child ids, no self-loops, no multi-parent edges, every node
    /// reachable from the root, depth within [`MAX_MATERIALIZE_DEPTH`], an
    /// independent root, and no independent node carrying a delta payload.
    pub(crate) fn validate_tree_shape(&self) -> Result<(), SnapshotBuildError> {
        let root = self.root_id as usize;
        if root >= self.nodes.len() {
            return Err(SnapshotBuildError::MissingRoot { root_id: root });
        }

        if self.nodes[root].baseline != SnapshotBaseline::Independent {
            return Err(SnapshotBuildError::RootNotIndependent { root_id: root });
        }

        for (node_id, node) in self.nodes.iter().enumerate() {
            if node.baseline == SnapshotBaseline::Independent
                && matches!(node.snapshot.payload, SnapshotPayload::Delta(_))
            {
                return Err(SnapshotBuildError::IndependentDelta { node_id });
            }
        }

        let mut indegree = vec![0usize; self.nodes.len()];
        for (parent_id, node) in self.nodes.iter().enumerate() {
            for child in node.next.as_deref().unwrap_or(&[]) {
                let child_id = *child as usize;
                if child_id >= self.nodes.len() {
                    return Err(SnapshotBuildError::MissingChild {
                        parent_id,
                        child_id,
                    });
                }
                if child_id == parent_id {
                    return Err(SnapshotBuildError::Cycle { node_id: parent_id });
                }

                indegree[child_id] += 1;
                if indegree[child_id] > 1 {
                    return Err(SnapshotBuildError::MultipleParents { node_id: child_id });
                }
            }
        }

        if indegree[root] != 0 {
            return Err(SnapshotBuildError::MissingRoot { root_id: root });
        }

        let mut reachable = vec![false; self.nodes.len()];
        let mut queue = VecDeque::from_iter([(root, 0usize)]);
        while let Some((node_id, depth)) = queue.pop_front() {
            if depth > MAX_MATERIALIZE_DEPTH {
                return Err(SnapshotBuildError::DepthLimitExceeded {
                    depth,
                    max: MAX_MATERIALIZE_DEPTH,
                });
            }
            if reachable[node_id] {
                continue;
            }
            reachable[node_id] = true;

            for child in self.nodes[node_id].next.as_deref().unwrap_or(&[]) {
                queue.push_back((*child as usize, depth + 1));
            }
        }

        let unreachable = reachable
            .iter()
            .enumerate()
            .filter_map(|(idx, seen)| (!*seen).then_some(idx))
            .collect::<Vec<_>>();
        if !unreachable.is_empty() {
            return Err(SnapshotBuildError::Unreachable {
                node_ids: unreachable,
            });
        }

        Ok(())
    }
}

/// Projects the set of changed dot-paths from a JSON patch.
pub fn changed_fields_from_patch(patch: &Patch) -> Option<IndexSet<ConfigField>> {
    let mut fields = IndexSet::new();
    for operation in &patch.0 {
        match operation {
            PatchOperation::Add(operation) => {
                insert_pointer_path(&mut fields, operation.path.as_str())
            }
            PatchOperation::Remove(operation) => {
                insert_pointer_path(&mut fields, operation.path.as_str())
            }
            PatchOperation::Replace(operation) => {
                insert_pointer_path(&mut fields, operation.path.as_str())
            }
            PatchOperation::Move(operation) => {
                insert_pointer_path(&mut fields, operation.from.as_str());
                insert_pointer_path(&mut fields, operation.path.as_str());
            }
            PatchOperation::Copy(operation) => {
                insert_pointer_path(&mut fields, operation.path.as_str())
            }
            PatchOperation::Test(_) => {}
        }
    }

    (!fields.is_empty()).then_some(fields)
}

/// Converts an RFC 6901 JSON pointer into a dot-separated field path.
pub fn json_pointer_to_dot_path(pointer: &str) -> ConfigField {
    if pointer.is_empty() {
        return String::new();
    }

    pointer
        .strip_prefix('/')
        .unwrap_or(pointer)
        .split('/')
        .map(decode_json_pointer_token)
        .collect::<Vec<_>>()
        .join(".")
}

fn insert_pointer_path(fields: &mut IndexSet<ConfigField>, pointer: &str) {
    fields.insert(json_pointer_to_dot_path(pointer));
}

fn decode_json_pointer_token(token: &str) -> String {
    let mut decoded = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') => decoded.push('~'),
                Some('1') => decoded.push('/'),
                Some(other) => {
                    decoded.push('~');
                    decoded.push(other);
                }
                None => decoded.push('~'),
            }
        } else {
            decoded.push(ch);
        }
    }
    decoded
}

fn serialized_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|value| value.len())
        .unwrap_or(usize::MAX)
}

fn usize_to_idx(node_id: usize) -> Result<Idx, SnapshotBuildError> {
    u32::try_from(node_id).map_err(|_| SnapshotBuildError::IdOverflow { node_id })
}

/// Snapshot archive encoding/decoding.
///
/// Decoding is strict: any version, shape, or serialization mismatch is
/// returned as an error. Cache owners MUST treat every decode error as a
/// cache miss and rebuild snapshots from source state:
///
/// ```ignore
/// match persistence::decode_archive(bytes) {
///     Ok(archive) => Some(archive.graph),
///     Err(error) => {
///         tracing::debug!(?error, "discarding runtime snapshot cache");
///         None
///     }
/// }
/// ```
#[cfg(feature = "snapshot-persistence")]
pub mod persistence {
    use std::{
        io::{Cursor, Read},
        sync::Arc,
    };

    use serde::{Deserialize, Serialize};
    use thiserror::Error;

    use super::StoredConfigSnapshotsGraph;

    pub const SNAPSHOT_ARCHIVE_VERSION: u16 = 2;
    /// Decompression ceiling, guarding against zstd decompression bombs.
    const MAX_ARCHIVE_DECODED_BYTES: u64 = 64 * 1024 * 1024;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct SnapshotArchive {
        pub format_version: u16,
        /// Diagnostic only; never used for compatibility decisions.
        pub crate_version: Arc<str>,
        pub graph: StoredConfigSnapshotsGraph,
    }

    #[derive(Debug, Error)]
    pub enum SnapshotPersistError {
        #[error(transparent)]
        Encode(#[from] rmp_serde::encode::Error),
        #[error(transparent)]
        Decode(#[from] rmp_serde::decode::Error),
        #[error(transparent)]
        Io(#[from] std::io::Error),
        #[error("unsupported snapshot archive version {found}, expected {expected}")]
        UnsupportedVersion { found: u16, expected: u16 },
        #[error("decoded snapshot archive exceeds {max_bytes} bytes")]
        DecodeLimitExceeded { max_bytes: u64 },
        #[error(transparent)]
        Graph(#[from] super::SnapshotBuildError),
    }

    pub fn encode_archive(
        graph: &StoredConfigSnapshotsGraph,
    ) -> Result<Vec<u8>, SnapshotPersistError> {
        let archive = SnapshotArchive {
            format_version: SNAPSHOT_ARCHIVE_VERSION,
            crate_version: Arc::from(env!("CARGO_PKG_VERSION")),
            graph: graph.clone(),
        };
        let body = rmp_serde::to_vec_named(&archive)?;
        Ok(zstd::stream::encode_all(body.as_slice(), 3)?)
    }

    pub fn decode_archive(bytes: &[u8]) -> Result<SnapshotArchive, SnapshotPersistError> {
        let decoder = zstd::stream::read::Decoder::new(Cursor::new(bytes))?;
        let mut limited = decoder.take(MAX_ARCHIVE_DECODED_BYTES + 1);
        let mut body = Vec::new();
        limited.read_to_end(&mut body)?;
        if body.len() as u64 > MAX_ARCHIVE_DECODED_BYTES {
            return Err(SnapshotPersistError::DecodeLimitExceeded {
                max_bytes: MAX_ARCHIVE_DECODED_BYTES,
            });
        }

        let archive: SnapshotArchive = rmp_serde::from_slice(&body)?;
        if archive.format_version != SNAPSHOT_ARCHIVE_VERSION {
            return Err(SnapshotPersistError::UnsupportedVersion {
                found: archive.format_version,
                expected: SNAPSHOT_ARCHIVE_VERSION,
            });
        }
        archive.graph.validate_tree_shape()?;
        Ok(archive)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::{
        profile::ScriptRuntime,
        runtime::value::{ConfigValue, PathSegment},
    };

    fn value(value: serde_json::Value) -> Arc<ConfigValue> {
        Arc::new(ConfigValue::try_from(value).unwrap())
    }

    fn pid(value: &str) -> ProfileId {
        ProfileId(value.to_owned())
    }

    fn file_root(profile_id: &str, role: ConfigExecutionRole) -> OperatorTag {
        OperatorTag::FileConfigRoot {
            profile_id: pid(profile_id),
            role,
        }
    }

    fn selected_file_root(profile_id: &str) -> OperatorTag {
        file_root(profile_id, ConfigExecutionRole::Selected)
    }

    fn composition_root(profile_id: &str, base: Option<&str>) -> OperatorTag {
        OperatorTag::CompositionRoot {
            profile_id: pid(profile_id),
            base: base.map(pid),
        }
    }

    fn extend_step(
        composition_id: &str,
        contributor_profile_id: &str,
        contributor_index: u32,
    ) -> OperatorTag {
        OperatorTag::ExtendProxiesStep {
            composition_id: pid(composition_id),
            contributor_profile_id: pid(contributor_profile_id),
            contributor_index,
        }
    }

    fn scoped_transform(
        host_profile_id: &str,
        role: ConfigExecutionRole,
        transform_profile_id: &str,
        transform_kind: TransformKind,
        step_index: u32,
    ) -> OperatorTag {
        OperatorTag::ScopedTransform {
            host_profile_id: pid(host_profile_id),
            role,
            transform_profile_id: pid(transform_profile_id),
            transform_kind,
            step_index,
        }
    }

    fn global_transform(
        selected_profile_id: &str,
        transform_profile_id: &str,
        transform_kind: TransformKind,
        step_index: u32,
    ) -> OperatorTag {
        OperatorTag::GlobalTransform {
            selected_profile_id: Some(pid(selected_profile_id)),
            transform_profile_id: pid(transform_profile_id),
            transform_kind,
            step_index,
        }
    }

    fn builtin_step(selected_profile_id: &str, step: BuiltinStepKind) -> OperatorTag {
        OperatorTag::BuiltinStep {
            selected_profile_id: Some(pid(selected_profile_id)),
            step,
        }
    }

    fn round_trip_tag(tag: OperatorTag) {
        let json = serde_json::to_value(&tag).unwrap();
        let back: OperatorTag = serde_json::from_value(json).unwrap();
        assert_eq!(tag, back);
    }

    fn full_node(
        tag: OperatorTag,
        baseline: SnapshotBaseline,
        next: Option<Vec<Idx>>,
    ) -> StoredConfigSnapshotState<Idx> {
        StoredConfigSnapshotState {
            snapshot: StoredConfigSnapshot {
                payload: SnapshotPayload::Full(value(json!({ "a": 1 }))),
            },
            tag,
            baseline,
            next,
        }
    }

    /// A member/base FileConfig branch: raw root followed by one scoped
    /// transform, mirroring design doc section 7.2.
    fn scoped_file_branch(
        profile_id: &str,
        role: ConfigExecutionRole,
        transform_profile_id: &str,
    ) -> ConfigSnapshotsBuilder {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "profile": profile_id, "stage": "raw" })),
            file_root(profile_id, role.clone()),
        );
        builder
            .push(
                scoped_transform(
                    profile_id,
                    role,
                    transform_profile_id,
                    TransformKind::Overlay,
                    0,
                ),
                value(json!({ "profile": profile_id, "stage": "scoped" })),
            )
            .unwrap();
        builder
    }

    #[test]
    fn diff_patch_apply_equality() {
        let before = json!({ "a": 1, "b": [1, 2] });
        let after = json!({ "a": 2, "b": [1, 2, 3] });
        let patch = json_patch::diff(&before, &after);
        let mut applied = before;
        json_patch::patch(&mut applied, &patch).unwrap();
        assert_eq!(applied, after);
    }

    #[test]
    fn keyframe_threshold_selects_full_when_patch_is_too_large() {
        let parent = value(json!({ "a": 1 }));
        let current = value(json!({ "a": 2 }));
        let policy = KeyframePolicy {
            delta_to_full_ratio: 0.0,
        };

        assert!(matches!(
            policy.encode(&parent, current),
            SnapshotPayload::Full(_)
        ));
    }

    #[test]
    fn rfc6901_decode_projects_changed_fields() {
        assert_eq!(json_pointer_to_dot_path("/a~1b/~01"), "a/b.~1");
        assert_eq!(json_pointer_to_dot_path("//foo"), ".foo");

        let before = json!({ "a/b": { "~1": 1 } });
        let after = json!({ "a/b": { "~1": 2 } });
        let patch = json_patch::diff(&before, &after);
        let fields = changed_fields_from_patch(&patch).unwrap();
        assert!(fields.contains("a/b.~1"));
    }

    #[test]
    fn builder_materializes_delta_graph() {
        let root = value(json!({ "a": 1, "stable": [1, 2] }));
        let child = value(json!({ "a": 2, "stable": [1, 2] }));
        let mut builder = ConfigSnapshotsBuilder::new_root(root, selected_file_root("primary"));

        builder
            .push(builtin_step("primary", BuiltinStepKind::Finalizing), child)
            .unwrap();
        let graph = builder.build().unwrap();

        assert_eq!(graph.root_id, 0);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(
            graph.nodes[1].key,
            SnapshotNodeKey::Builtin {
                selected_profile_id: Some(pid("primary")),
                step: BuiltinStepKind::Finalizing,
            }
        );
        assert_eq!(
            graph.nodes[1].snapshot.config,
            json!({ "a": 2, "stable": [1, 2] })
        );
        assert!(
            graph.nodes[1]
                .snapshot
                .changed_fields
                .as_ref()
                .unwrap()
                .contains("a")
        );
    }

    #[test]
    fn materialize_rejects_malformed_stored_graph() {
        // Self-cycle: node 0 references itself as a child.
        let cyclic = StoredConfigSnapshotsGraph {
            nodes: vec![full_node(
                selected_file_root("primary"),
                SnapshotBaseline::Independent,
                Some(vec![0]),
            )],
            root_id: 0,
        };
        assert!(matches!(
            cyclic.materialize(),
            Err(SnapshotBuildError::Cycle { .. })
        ));

        // Out-of-range child id.
        let dangling = StoredConfigSnapshotsGraph {
            nodes: vec![full_node(
                selected_file_root("primary"),
                SnapshotBaseline::Independent,
                Some(vec![7]),
            )],
            root_id: 0,
        };
        assert!(matches!(
            dangling.materialize(),
            Err(SnapshotBuildError::MissingChild { .. })
        ));

        // Multi-parent: nodes 0 and 1 both reference node 2.
        let multi_parent = StoredConfigSnapshotsGraph {
            nodes: vec![
                full_node(
                    selected_file_root("primary"),
                    SnapshotBaseline::Independent,
                    Some(vec![1, 2]),
                ),
                full_node(
                    builtin_step("primary", BuiltinStepKind::GuardOverrides),
                    SnapshotBaseline::Parent,
                    Some(vec![2]),
                ),
                full_node(
                    builtin_step("primary", BuiltinStepKind::Finalizing),
                    SnapshotBaseline::Parent,
                    None,
                ),
            ],
            root_id: 0,
        };
        assert!(matches!(
            multi_parent.materialize(),
            Err(SnapshotBuildError::MultipleParents { .. })
        ));

        // Root carrying a parent-relative baseline.
        let parent_root = StoredConfigSnapshotsGraph {
            nodes: vec![full_node(
                selected_file_root("primary"),
                SnapshotBaseline::Parent,
                None,
            )],
            root_id: 0,
        };
        assert!(matches!(
            parent_root.materialize(),
            Err(SnapshotBuildError::RootNotIndependent { .. })
        ));
    }

    #[test]
    fn detects_u32_overflow() {
        assert!(matches!(
            usize_to_idx(u32::MAX as usize + 1),
            Err(SnapshotBuildError::IdOverflow { .. })
        ));
    }

    #[test]
    fn value_path_update_shares_unchanged_subtrees() {
        let root = ConfigValue::try_from(json!({
            "changed": { "x": 1 },
            "stable": [1, 2, 3]
        }))
        .unwrap();
        let stable_key: Arc<str> = Arc::from("stable");
        let changed_key: Arc<str> = Arc::from("changed");
        let x_key: Arc<str> = Arc::from("x");

        let before_stable = root
            .as_object_arc()
            .unwrap()
            .get(&stable_key)
            .unwrap()
            .as_array_arc()
            .unwrap()
            .clone();
        let updated = root
            .set_path(
                &[PathSegment::Key(changed_key), PathSegment::Key(x_key)],
                ConfigValue::Number(2.into()),
            )
            .unwrap();
        let after_stable = updated
            .as_object_arc()
            .unwrap()
            .get(&stable_key)
            .unwrap()
            .as_array_arc()
            .unwrap()
            .clone();

        assert!(Arc::ptr_eq(&before_stable, &after_stable));
    }

    #[test]
    fn operator_tag_file_config_root_round_trips_all_roles() {
        round_trip_tag(selected_file_root("selected"));
        round_trip_tag(file_root(
            "base",
            ConfigExecutionRole::CompositionBase {
                composition_id: pid("composition"),
            },
        ));
        round_trip_tag(file_root(
            "member",
            ConfigExecutionRole::CompositionContributor {
                composition_id: pid("composition"),
                contributor_index: 2,
            },
        ));
    }

    #[test]
    fn operator_tag_composition_root_round_trips_base_some_and_none() {
        round_trip_tag(composition_root("composition", Some("base")));
        round_trip_tag(composition_root("composition", None));
        assert_eq!(
            composition_root("composition", Some("base")).node_key(),
            composition_root("composition", None).node_key()
        );
    }

    #[test]
    fn operator_tag_extend_proxies_step_round_trips() {
        round_trip_tag(extend_step("composition", "member", 3));
        assert_eq!(
            extend_step("composition", "member-a", 3).node_key(),
            extend_step("composition", "member-b", 3).node_key()
        );
    }

    #[test]
    fn operator_tag_scoped_transform_round_trips_and_key_drops_transform_identity() {
        let overlay = scoped_transform(
            "host",
            ConfigExecutionRole::Selected,
            "overlay-transform",
            TransformKind::Overlay,
            0,
        );
        let script = scoped_transform(
            "host",
            ConfigExecutionRole::Selected,
            "script-transform",
            TransformKind::Script {
                runtime: ScriptRuntime::Lua,
            },
            0,
        );

        round_trip_tag(overlay.clone());
        round_trip_tag(script.clone());
        assert_eq!(overlay.node_key(), script.node_key());
    }

    #[test]
    fn operator_tag_global_transform_round_trips_and_key_drops_transform_identity() {
        let overlay = global_transform("selected", "overlay-transform", TransformKind::Overlay, 1);
        let script = global_transform(
            "selected",
            "script-transform",
            TransformKind::Script {
                runtime: ScriptRuntime::Lua,
            },
            1,
        );

        round_trip_tag(overlay.clone());
        round_trip_tag(script.clone());
        assert_eq!(overlay.node_key(), script.node_key());
    }

    #[test]
    fn operator_tag_builtin_step_round_trips_all_steps() {
        round_trip_tag(builtin_step("selected", BuiltinStepKind::GuardOverrides));
        round_trip_tag(builtin_step(
            "selected",
            BuiltinStepKind::WhitelistFieldFilter,
        ));
        round_trip_tag(builtin_step("selected", BuiltinStepKind::Finalizing));
    }

    #[test]
    fn selected_file_config_processing_order_is_scoped_global_builtin() {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "step": "file" })),
            selected_file_root("selected"),
        );
        builder
            .push(
                scoped_transform(
                    "selected",
                    ConfigExecutionRole::Selected,
                    "scoped",
                    TransformKind::Overlay,
                    0,
                ),
                value(json!({ "step": "scoped" })),
            )
            .unwrap();
        builder
            .push(
                global_transform("selected", "global", TransformKind::Overlay, 0),
                value(json!({ "step": "global" })),
            )
            .unwrap();
        builder
            .push(
                builtin_step("selected", BuiltinStepKind::WhitelistFieldFilter),
                value(json!({ "step": "whitelist" })),
            )
            .unwrap();
        builder
            .push(
                builtin_step("selected", BuiltinStepKind::GuardOverrides),
                value(json!({ "step": "guard" })),
            )
            .unwrap();
        builder
            .push(
                OperatorTag::BuiltinTransform {
                    selected_profile_id: Some(pid("selected")),
                    name: "config_fixer".to_string(),
                    step_index: 0,
                },
                value(json!({ "step": "builtin_transform" })),
            )
            .unwrap();
        builder
            .push(
                builtin_step("selected", BuiltinStepKind::Finalizing),
                value(json!({ "step": "final" })),
            )
            .unwrap();

        let graph = builder.build().unwrap();
        assert!(matches!(
            &graph.nodes[0].tag,
            OperatorTag::FileConfigRoot {
                role: ConfigExecutionRole::Selected,
                ..
            }
        ));
        assert!(matches!(
            &graph.nodes[1].tag,
            OperatorTag::ScopedTransform { .. }
        ));
        assert!(matches!(
            &graph.nodes[2].tag,
            OperatorTag::GlobalTransform { .. }
        ));
        assert!(matches!(
            &graph.nodes[3].tag,
            OperatorTag::BuiltinStep {
                step: BuiltinStepKind::WhitelistFieldFilter,
                ..
            }
        ));
        assert!(matches!(
            &graph.nodes[4].tag,
            OperatorTag::BuiltinStep {
                step: BuiltinStepKind::GuardOverrides,
                ..
            }
        ));
        assert!(matches!(
            &graph.nodes[5].tag,
            OperatorTag::BuiltinTransform { .. }
        ));
        assert!(matches!(
            &graph.nodes[6].tag,
            OperatorTag::BuiltinStep {
                step: BuiltinStepKind::Finalizing,
                ..
            }
        ));
    }

    #[test]
    fn member_file_config_processing_order_is_scoped_only() {
        let builder = scoped_file_branch(
            "member",
            ConfigExecutionRole::CompositionContributor {
                composition_id: pid("composition"),
                contributor_index: 0,
            },
            "normalize",
        );
        let graph = builder.build().unwrap();

        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.iter().all(|node| !matches!(
            node.tag,
            OperatorTag::GlobalTransform { .. } | OperatorTag::BuiltinStep { .. }
        )));
    }

    #[test]
    fn composition_with_base_processing_order_attaches_independent_branches() {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "source": "base-scoped" })),
            composition_root("composition", Some("base")),
        );
        let root_id = builder.root_node_id();

        let before_base_attach = builder.current_node_id();
        let base_root = builder
            .attach_independent_branch(
                root_id,
                scoped_file_branch(
                    "base",
                    ConfigExecutionRole::CompositionBase {
                        composition_id: pid("composition"),
                    },
                    "base-transform",
                ),
            )
            .unwrap();
        assert_eq!(builder.current_node_id(), before_base_attach);

        let contributor_0_root = builder
            .attach_independent_branch(
                builder.current_node_id(),
                scoped_file_branch(
                    "member-a",
                    ConfigExecutionRole::CompositionContributor {
                        composition_id: pid("composition"),
                        contributor_index: 0,
                    },
                    "member-a-transform",
                ),
            )
            .unwrap();
        assert_eq!(builder.current_node_id(), before_base_attach);
        let extend_0 = builder
            .push(
                extend_step("composition", "member-a", 0),
                value(json!({ "source": "extend-a" })),
            )
            .unwrap();

        let before_contributor_1_attach = builder.current_node_id();
        let contributor_1_root = builder
            .attach_independent_branch(
                before_contributor_1_attach,
                scoped_file_branch(
                    "member-b",
                    ConfigExecutionRole::CompositionContributor {
                        composition_id: pid("composition"),
                        contributor_index: 1,
                    },
                    "member-b-transform",
                ),
            )
            .unwrap();
        assert_eq!(builder.current_node_id(), before_contributor_1_attach);
        let extend_1 = builder
            .push(
                extend_step("composition", "member-b", 1),
                value(json!({ "source": "extend-b" })),
            )
            .unwrap();
        builder
            .push(
                scoped_transform(
                    "composition",
                    ConfigExecutionRole::Selected,
                    "composition-transform",
                    TransformKind::Overlay,
                    0,
                ),
                value(json!({ "source": "composition-scoped" })),
            )
            .unwrap();
        builder
            .push(
                global_transform("composition", "global", TransformKind::Overlay, 0),
                value(json!({ "source": "global" })),
            )
            .unwrap();
        builder
            .push(
                builtin_step("composition", BuiltinStepKind::Finalizing),
                value(json!({ "source": "final" })),
            )
            .unwrap();

        let stored = builder.build_stored().unwrap();
        assert_eq!(
            stored.nodes[root_id].next.as_deref(),
            Some(&[base_root as Idx, contributor_0_root as Idx, extend_0 as Idx][..])
        );
        assert_eq!(
            stored.nodes[extend_0].next.as_deref(),
            Some(&[contributor_1_root as Idx, extend_1 as Idx][..])
        );
        assert_eq!(
            stored.nodes[base_root].baseline,
            SnapshotBaseline::Independent
        );
        assert_eq!(
            stored.nodes[contributor_0_root].baseline,
            SnapshotBaseline::Independent
        );
        assert_eq!(
            stored.nodes[contributor_1_root].baseline,
            SnapshotBaseline::Independent
        );
    }

    #[test]
    fn composition_without_base_processing_order_starts_from_clean_seed() {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "proxies": [] })),
            composition_root("composition", None),
        );
        let root_id = builder.root_node_id();
        let contributor_root = builder
            .attach_independent_branch(
                root_id,
                scoped_file_branch(
                    "member",
                    ConfigExecutionRole::CompositionContributor {
                        composition_id: pid("composition"),
                        contributor_index: 0,
                    },
                    "member-transform",
                ),
            )
            .unwrap();
        let extend = builder
            .push(
                extend_step("composition", "member", 0),
                value(json!({ "proxies": [{ "name": "a" }] })),
            )
            .unwrap();

        let stored = builder.build_stored().unwrap();
        assert!(matches!(
            &stored.nodes[root_id].tag,
            OperatorTag::CompositionRoot { base: None, .. }
        ));
        assert_eq!(
            stored.nodes[root_id].next.as_deref(),
            Some(&[contributor_root as Idx, extend as Idx][..])
        );
    }

    #[test]
    fn global_transforms_appear_once_on_final_selected_mainline() {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "proxies": [] })),
            composition_root("composition", None),
        );
        builder
            .attach_independent_branch(
                builder.root_node_id(),
                scoped_file_branch(
                    "member",
                    ConfigExecutionRole::CompositionContributor {
                        composition_id: pid("composition"),
                        contributor_index: 0,
                    },
                    "member-transform",
                ),
            )
            .unwrap();
        builder
            .push(
                extend_step("composition", "member", 0),
                value(json!({ "step": "extend" })),
            )
            .unwrap();
        builder
            .push(
                global_transform("composition", "global", TransformKind::Overlay, 0),
                value(json!({ "step": "global" })),
            )
            .unwrap();
        builder
            .push(
                builtin_step("composition", BuiltinStepKind::Finalizing),
                value(json!({ "step": "final" })),
            )
            .unwrap();

        let stored = builder.build_stored().unwrap();
        assert_eq!(
            stored
                .nodes
                .iter()
                .filter(|node| matches!(node.tag, OperatorTag::GlobalTransform { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn storage_contract_independent_branch_root_materializes_without_changed_fields() {
        let mut builder = ConfigSnapshotsBuilder::new_root(
            value(json!({ "a": 1 })),
            composition_root("composition", None),
        );
        let branch_root = builder
            .attach_independent_branch(
                builder.root_node_id(),
                scoped_file_branch(
                    "member",
                    ConfigExecutionRole::CompositionContributor {
                        composition_id: pid("composition"),
                        contributor_index: 0,
                    },
                    "member-transform",
                ),
            )
            .unwrap();

        let stored = builder.build_stored().unwrap();
        assert_eq!(
            stored.nodes[branch_root].baseline,
            SnapshotBaseline::Independent
        );
        assert!(matches!(
            stored.nodes[branch_root].snapshot.payload,
            SnapshotPayload::Full(_)
        ));

        let materialized = stored.materialize().unwrap();
        assert!(
            materialized.nodes[branch_root]
                .snapshot
                .changed_fields
                .is_none()
        );

        // An independent node carrying a delta payload is rejected.
        let invalid = StoredConfigSnapshotsGraph {
            nodes: vec![StoredConfigSnapshotState {
                snapshot: StoredConfigSnapshot {
                    payload: SnapshotPayload::Delta(json_patch::diff(
                        &json!({ "a": 1 }),
                        &json!({ "a": 2 }),
                    )),
                },
                tag: selected_file_root("primary"),
                baseline: SnapshotBaseline::Independent,
                next: None,
            }],
            root_id: 0,
        };
        assert!(matches!(
            invalid.validate_tree_shape(),
            Err(SnapshotBuildError::IndependentDelta { .. })
        ));
        assert!(matches!(
            invalid.materialize(),
            Err(SnapshotBuildError::IndependentDelta { .. })
        ));
    }

    #[cfg(feature = "snapshot-persistence")]
    #[test]
    fn storage_contract_archive_v2_round_trip_and_decode_failures() {
        let root = value(json!({ "a": 1 }));
        let graph = ConfigSnapshotsBuilder::new_root(root, selected_file_root("primary"))
            .build_stored()
            .unwrap();
        let bytes = persistence::encode_archive(&graph).unwrap();
        let archive = persistence::decode_archive(&bytes).unwrap();

        assert_eq!(
            archive.format_version,
            persistence::SNAPSHOT_ARCHIVE_VERSION
        );
        assert_eq!(archive.graph.root_id, graph.root_id);

        // A v1 archive is rejected by version.
        let v1_archive = persistence::SnapshotArchive {
            format_version: 1,
            crate_version: Arc::from("0.0.0"),
            graph: graph.clone(),
        };
        let v1_body = rmp_serde::to_vec_named(&v1_archive).unwrap();
        let v1_bytes = zstd::stream::encode_all(v1_body.as_slice(), 3).unwrap();
        assert!(matches!(
            persistence::decode_archive(&v1_bytes),
            Err(persistence::SnapshotPersistError::UnsupportedVersion {
                found: 1,
                expected: 2,
            })
        ));

        // A malformed graph is rejected by shape validation.
        let malformed = StoredConfigSnapshotsGraph {
            nodes: vec![full_node(
                selected_file_root("primary"),
                SnapshotBaseline::Independent,
                Some(vec![7]),
            )],
            root_id: 0,
        };
        let malformed_archive = persistence::SnapshotArchive {
            format_version: persistence::SNAPSHOT_ARCHIVE_VERSION,
            crate_version: Arc::from("0.0.0"),
            graph: malformed,
        };
        let malformed_body = rmp_serde::to_vec_named(&malformed_archive).unwrap();
        let malformed_bytes = zstd::stream::encode_all(malformed_body.as_slice(), 3).unwrap();
        assert!(matches!(
            persistence::decode_archive(&malformed_bytes),
            Err(persistence::SnapshotPersistError::Graph(
                SnapshotBuildError::MissingChild { .. }
            ))
        ));

        let cyclic = StoredConfigSnapshotsGraph {
            nodes: vec![full_node(
                selected_file_root("primary"),
                SnapshotBaseline::Independent,
                Some(vec![0]),
            )],
            root_id: 0,
        };
        let cyclic_archive = persistence::SnapshotArchive {
            format_version: persistence::SNAPSHOT_ARCHIVE_VERSION,
            crate_version: Arc::from("0.0.0"),
            graph: cyclic,
        };
        let cyclic_body = rmp_serde::to_vec_named(&cyclic_archive).unwrap();
        let cyclic_bytes = zstd::stream::encode_all(cyclic_body.as_slice(), 3).unwrap();
        assert!(matches!(
            persistence::decode_archive(&cyclic_bytes),
            Err(persistence::SnapshotPersistError::Graph(
                SnapshotBuildError::Cycle { .. }
            ))
        ));
    }

    #[test]
    fn operator_tag_bare_root_and_builtin_transform_round_trip() {
        round_trip_tag(OperatorTag::BareRoot);
        round_trip_tag(OperatorTag::BuiltinTransform {
            selected_profile_id: Some(pid("selected")),
            name: "verge_hy_alpn".to_string(),
            step_index: 0,
        });
        round_trip_tag(OperatorTag::BuiltinTransform {
            selected_profile_id: None,
            name: "config_fixer".to_string(),
            step_index: 1,
        });

        // node_key 丢弃展示性 name 字段。
        let a = OperatorTag::BuiltinTransform {
            selected_profile_id: None,
            name: "a".to_string(),
            step_index: 2,
        };
        let b = OperatorTag::BuiltinTransform {
            selected_profile_id: None,
            name: "b".to_string(),
            step_index: 2,
        };
        assert_eq!(a.node_key(), b.node_key());
        assert_eq!(OperatorTag::BareRoot.node_key(), SnapshotNodeKey::BareRoot);
    }

    #[test]
    fn operator_tag_optional_selected_is_forward_compatible_with_v2_wire() {
        // 旧 v2 wire 中 selected_profile_id 是裸字符串；Option 化后必须解码为 Some。
        let tag: OperatorTag = serde_json::from_value(json!({
            "kind": "builtin_step",
            "data": { "selected_profile_id": "sel", "step": "finalizing" }
        }))
        .unwrap();
        assert_eq!(
            tag,
            OperatorTag::BuiltinStep {
                selected_profile_id: Some(pid("sel")),
                step: BuiltinStepKind::Finalizing,
            }
        );

        let tag: OperatorTag = serde_json::from_value(json!({
            "kind": "global_transform",
            "data": {
                "selected_profile_id": "sel",
                "transform_profile_id": "t",
                "transform_kind": { "type": "overlay" },
                "step_index": 0
            }
        }))
        .unwrap();
        assert!(matches!(
            tag,
            OperatorTag::GlobalTransform {
                selected_profile_id: Some(_),
                ..
            }
        ));
    }
}
