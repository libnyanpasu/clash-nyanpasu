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

use crate::{profile::item::kind::ProfileItemType, runtime::value::ConfigValue};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ChainNodeKind {
    Scoped {
        #[specta(type = String)]
        parent_profile_id: Arc<str>,
    },
    Global,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum OperatorTag {
    Root {
        #[specta(type = String)]
        primary_profile_id: Arc<str>,
    },
    SecondaryProcessing {
        #[specta(type = String)]
        profile_id: Arc<str>,
    },
    SelectedProfilesProxiesMerge {
        #[specta(type = String)]
        primary_profile_id: Arc<str>,
        #[specta(type = Vec<String>)]
        other_profiles_ids: Vec<Arc<str>>,
    },
    ChainNode {
        kind: ChainNodeKind,
        #[specta(type = String)]
        profile_id: Arc<str>,
        profile_kind: ProfileItemType,
    },
    BuiltinChain {
        #[specta(type = String)]
        name: Arc<str>,
    },
    GuardOverrides,
    WhitelistFieldFilter,
    Finalizing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ConfigSnapshotState<C> {
    pub snapshot: ConfigSnapshot,
    /// The operator that generated this snapshot.
    pub tag: OperatorTag,
    pub next: Option<Vec<C>>,
}

impl<C> ConfigSnapshotState<C> {
    pub fn new(snapshot: ConfigSnapshot, tag: OperatorTag, next: Option<Vec<C>>) -> Self {
        Self {
            snapshot,
            tag,
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
    pub next: Option<Vec<C>>,
}

impl<C> StoredConfigSnapshotState<C> {
    pub fn new(snapshot: StoredConfigSnapshot, tag: OperatorTag, next: Option<Vec<C>>) -> Self {
        Self {
            snapshot,
            tag,
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

pub struct ConfigSnapshotsBuilder {
    slab: Slab<BuilderNode>,
    root_id: NodeId,
    /// The current processing snapshot id.
    current_id: NodeId,
    keyframe_policy: KeyframePolicy,
}

impl ConfigSnapshotsBuilder {
    pub fn new(root_config: Arc<ConfigValue>, primary_profile_id: impl Into<Arc<str>>) -> Self {
        Self::with_keyframe_policy(root_config, primary_profile_id, KeyframePolicy::default())
    }

    pub fn with_keyframe_policy(
        root_config: Arc<ConfigValue>,
        primary_profile_id: impl Into<Arc<str>>,
        keyframe_policy: KeyframePolicy,
    ) -> Self {
        let mut slab = Slab::new();
        let root_state = StoredConfigSnapshotState {
            snapshot: StoredConfigSnapshot {
                payload: SnapshotPayload::Full(root_config.clone()),
            },
            tag: OperatorTag::Root {
                primary_profile_id: primary_profile_id.into(),
            },
            next: None,
        };

        let root_id = slab.insert(BuilderNode {
            full: root_config,
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

    pub fn new_subtree(&self, node_id: NodeId) -> Result<Self, SnapshotBuildError> {
        let node = self
            .slab
            .get(node_id)
            .ok_or(SnapshotBuildError::MissingRoot { root_id: node_id })?;

        let mut slab = Slab::new();
        let root_state = StoredConfigSnapshotState {
            snapshot: StoredConfigSnapshot {
                payload: SnapshotPayload::Full(node.full.clone()),
            },
            tag: node.state.tag.clone(),
            next: None,
        };
        let root_id = slab.insert(BuilderNode {
            full: node.full.clone(),
            state: root_state,
            parent_id: None,
            children: Vec::new(),
        });

        Ok(Self {
            slab,
            root_id,
            current_id: root_id,
            keyframe_policy: self.keyframe_policy,
        })
    }

    pub fn add_node(
        &mut self,
        parent_id: NodeId,
        tag: OperatorTag,
        current: Arc<ConfigValue>,
    ) -> Result<NodeId, SnapshotBuildError> {
        let parent = self
            .slab
            .get(parent_id)
            .ok_or(SnapshotBuildError::MissingRoot { root_id: parent_id })?;
        let payload = self.keyframe_policy.encode(&parent.full, current.clone());
        Ok(self.insert_child(parent_id, tag, current, payload))
    }

    pub fn add_node_to_current(
        &mut self,
        tag: OperatorTag,
        current: Arc<ConfigValue>,
    ) -> Result<NodeId, SnapshotBuildError> {
        self.add_node(self.current_id, tag, current)
    }

    pub fn push_value(
        &mut self,
        tag: OperatorTag,
        current: Arc<ConfigValue>,
    ) -> Result<Idx, SnapshotBuildError> {
        let id = self.add_node_to_current(tag, current)?;
        self.current_id = id;
        usize_to_idx(id)
    }

    pub fn push_node(
        &mut self,
        tag: OperatorTag,
        current: Arc<ConfigValue>,
    ) -> Result<NodeId, SnapshotBuildError> {
        let id = self.add_node_to_current(tag, current)?;
        self.current_id = id;
        Ok(id)
    }

    pub fn add_leaf_from_subtree(
        &mut self,
        parent_id: NodeId,
        subtree: ConfigSnapshotsBuilder,
    ) -> Result<Vec<NodeId>, SnapshotBuildError> {
        let mut subtree = subtree.build_tree()?;
        self.add_leaf(parent_id, subtree.node.next.take().unwrap_or_default())
    }

    pub fn add_leaf(
        &mut self,
        parent_id: NodeId,
        children: Vec<ConfigSnapshotTreeNode>,
    ) -> Result<Vec<NodeId>, SnapshotBuildError> {
        if self.slab.get(parent_id).is_none() {
            return Err(SnapshotBuildError::MissingRoot { root_id: parent_id });
        }

        let mut ids = Vec::with_capacity(children.len());
        let mut queue = VecDeque::from_iter([(parent_id, children)]);

        while let Some((parent_id, children)) = queue.pop_front() {
            for mut child in children {
                let grand_children = child.node.next.take();
                let id = self.add_node(parent_id, child.node.tag, child.full)?;

                if let Some(grand_children) = grand_children {
                    queue.push_back((id, grand_children));
                }

                ids.push(id);
            }
        }

        Ok(ids)
    }

    pub fn add_leaf_to_current(
        &mut self,
        children: Vec<ConfigSnapshotTreeNode>,
    ) -> Result<Vec<NodeId>, SnapshotBuildError> {
        self.add_leaf(self.current_id, children)
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId) -> Result<(), SnapshotBuildError> {
        if self.slab.get(from).is_none() {
            return Err(SnapshotBuildError::MissingRoot { root_id: from });
        }
        if self.slab.get(to).is_none() {
            return Err(SnapshotBuildError::MissingChild {
                parent_id: from,
                child_id: to,
            });
        }

        self.slab[from].children.push(to);
        self.slab[to].parent_id = Some(from);
        Ok(())
    }

    pub fn set_current(&mut self, id: NodeId) -> Result<(), SnapshotBuildError> {
        if self.slab.get(id).is_none() {
            return Err(SnapshotBuildError::MissingRoot { root_id: id });
        }

        self.current_id = id;
        Ok(())
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
        payload: SnapshotPayload,
    ) -> NodeId {
        let id = self.slab.insert(BuilderNode {
            full: current,
            state: StoredConfigSnapshotState {
                snapshot: StoredConfigSnapshot { payload },
                tag,
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

        let (config, changed_fields) = match &state.snapshot.payload {
            SnapshotPayload::Full(value) => {
                let config = value.to_json();
                let changed_fields = parent
                    .map(|parent| json_patch::diff(parent, &config))
                    .and_then(|patch| changed_fields_from_patch(&patch));
                (config, changed_fields)
            }
            SnapshotPayload::Delta(patch) => {
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
            next: next.clone(),
        });

        for child in next.unwrap_or_default() {
            self.materialize_node(child, Some(&config), depth + 1, nodes)?;
        }

        Ok(config)
    }

    /// Validates that a (possibly deserialized) stored graph is a single-rooted
    /// tree: valid child ids, no self-loops, no multi-parent edges, every node
    /// reachable from the root, and depth within [`MAX_MATERIALIZE_DEPTH`].
    pub(crate) fn validate_tree_shape(&self) -> Result<(), SnapshotBuildError> {
        let root = self.root_id as usize;
        if root >= self.nodes.len() {
            return Err(SnapshotBuildError::MissingRoot { root_id: root });
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

#[cfg(feature = "snapshot-persistence")]
pub mod persistence {
    use std::{
        io::{Cursor, Read},
        sync::Arc,
    };

    use serde::{Deserialize, Serialize};
    use thiserror::Error;

    use super::StoredConfigSnapshotsGraph;

    pub const SNAPSHOT_ARCHIVE_VERSION: u16 = 1;
    /// Decompression ceiling, guarding against zstd decompression bombs.
    const MAX_ARCHIVE_DECODED_BYTES: u64 = 64 * 1024 * 1024;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct SnapshotArchive {
        pub format_version: u16,
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
    use crate::runtime::value::{ConfigValue, PathSegment};

    fn value(value: serde_json::Value) -> Arc<ConfigValue> {
        Arc::new(ConfigValue::try_from(value).unwrap())
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
        let mut builder = ConfigSnapshotsBuilder::new(root, "primary");

        builder.push_node(OperatorTag::Finalizing, child).unwrap();
        let graph = builder.build().unwrap();

        assert_eq!(graph.root_id, 0);
        assert_eq!(graph.nodes.len(), 2);
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
    fn builder_reports_cycle() {
        let root = value(json!({ "a": 1 }));
        let mut builder = ConfigSnapshotsBuilder::new(root, "primary");

        builder
            .add_edge(builder.root_node_id(), builder.root_node_id())
            .unwrap();
        assert!(matches!(
            builder.build_stored(),
            Err(SnapshotBuildError::Cycle { .. })
        ));
    }

    #[test]
    fn builder_reports_multiple_parents() {
        let root = value(json!({ "a": 1 }));
        let child = value(json!({ "a": 2 }));
        let mut builder = ConfigSnapshotsBuilder::new(root, "primary");
        let child_id = builder
            .add_node_to_current(OperatorTag::Finalizing, child)
            .unwrap();

        builder.add_edge(builder.root_node_id(), child_id).unwrap();
        assert!(matches!(
            builder.build_stored(),
            Err(SnapshotBuildError::MultipleParents { .. })
        ));
    }

    #[test]
    fn builder_reports_missing_child() {
        let root = value(json!({ "a": 1 }));
        let mut builder = ConfigSnapshotsBuilder::new(root, "primary");

        assert!(matches!(
            builder.add_edge(builder.root_node_id(), 999),
            Err(SnapshotBuildError::MissingChild { .. })
        ));
    }

    #[test]
    fn materialize_rejects_malformed_stored_graph() {
        let stored_node = |next: Option<Vec<Idx>>| StoredConfigSnapshotState {
            snapshot: StoredConfigSnapshot {
                payload: SnapshotPayload::Full(value(json!({ "a": 1 }))),
            },
            tag: OperatorTag::Finalizing,
            next,
        };

        // Self-cycle: node 0 references itself as a child.
        let cyclic = StoredConfigSnapshotsGraph {
            nodes: vec![stored_node(Some(vec![0]))],
            root_id: 0,
        };
        assert!(matches!(
            cyclic.materialize(),
            Err(SnapshotBuildError::Cycle { .. })
        ));

        // Out-of-range child id.
        let dangling = StoredConfigSnapshotsGraph {
            nodes: vec![stored_node(Some(vec![7]))],
            root_id: 0,
        };
        assert!(matches!(
            dangling.materialize(),
            Err(SnapshotBuildError::MissingChild { .. })
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

    #[cfg(feature = "snapshot-persistence")]
    #[test]
    fn archive_round_trip() {
        let root = value(json!({ "a": 1 }));
        let graph = ConfigSnapshotsBuilder::new(root, "primary")
            .build_stored()
            .unwrap();
        let bytes = persistence::encode_archive(&graph).unwrap();
        let archive = persistence::decode_archive(&bytes).unwrap();

        assert_eq!(
            archive.format_version,
            persistence::SNAPSHOT_ARCHIVE_VERSION
        );
        assert_eq!(archive.graph.root_id, graph.root_id);
    }
}
