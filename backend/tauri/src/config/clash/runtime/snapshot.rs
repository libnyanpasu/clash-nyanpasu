//! A snapshot of the clash config processing

use std::collections::VecDeque;

use indexmap::IndexSet;
use serde_yaml::Mapping;
use slab::Slab;

use crate::config::profile::item_type::ProfileItemType;

/// A field in the config, use `.` to represent nested fields
pub type ConfigField = String;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigSnapshot {
    /// Current snapshot of the config
    pub config: Mapping,
    /// The changed fields compared to the previous snapshot
    pub changed_fields: Option<IndexSet<ConfigField>>,
}

impl ConfigSnapshot {
    pub fn new_with_diff(previous: &Mapping, current: Mapping) -> Self {
        let changed_fields = crate::utils::yaml::diff_fields(previous, &current);
        Self {
            config: current,
            changed_fields: if changed_fields.is_empty() {
                None
            } else {
                Some(changed_fields)
            },
        }
    }

    pub fn new_unchanged(config: Mapping) -> Self {
        Self {
            config,
            changed_fields: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ChainNodeKind {
    Scoped { parent_profile_id: String },
    Global,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ProcessKind {
    Root {
        primary_profile_id: String,
    },
    SecondaryProcessing {
        profile_id: String,
    },
    SelectedProfilesProxiesMerge {
        primary_profile_id: String,
        other_profiles_ids: Vec<String>,
    },
    ChainNode {
        kind: ChainNodeKind,
        profile_id: String,
        profile_kind: ProfileItemType,
    },
    BuiltinChain {
        name: String,
    },
    GuardOverrides,
    WhitelistFieldFilter,
    Finalizing,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigSnapshotState {
    pub snapshot: ConfigSnapshot,
    /// The kind of process that generated this snapshot
    pub process_kind: ProcessKind,
    pub next: Option<Vec<ConfigSnapshotState>>,
}

impl ConfigSnapshotState {
    pub fn new(
        snapshot: ConfigSnapshot,
        process_kind: ProcessKind,
        next: Option<Vec<ConfigSnapshotState>>,
    ) -> Self {
        Self {
            snapshot,
            process_kind,
            next,
        }
    }
}

type NodeId = usize;

#[derive(Debug, Clone)]
struct BuilderNode {
    state: ConfigSnapshotState,
    children: Vec<NodeId>,
}

pub struct ConfigSnapshotsBuilder {
    pub slab: Slab<BuilderNode>,
    pub root_id: NodeId,
    /// The current processing snapshot id
    pub current_id: NodeId,
}

impl ConfigSnapshotsBuilder {
    pub fn new(root_snapshot: ConfigSnapshot, primary_profile_id: String) -> Self {
        let mut slab = Slab::new();

        let root_state = ConfigSnapshotState {
            snapshot: root_snapshot,
            process_kind: ProcessKind::Root { primary_profile_id },
            next: None,
        };

        let root_id = slab.insert(BuilderNode {
            state: root_state,
            children: Vec::new(),
        });

        Self {
            slab,
            root_id,
            current_id: root_id,
        }
    }

    pub fn current_node_id(&self) -> NodeId {
        self.current_id
    }

    pub fn root_node_id(&self) -> NodeId {
        self.root_id
    }

    pub fn new_subtree(&self, node_id: NodeId) -> Self {
        let mut slab = Slab::new();
        let mut node = self.slab[node_id].clone();
        node.children.clear();
        let root_id = slab.insert(node);

        Self {
            slab,
            root_id,
            current_id: root_id,
        }
    }

    pub fn add_node(&mut self, parent_id: NodeId, node: ConfigSnapshotState) -> NodeId {
        let id = self.slab.insert(BuilderNode {
            state: node,
            children: Vec::new(),
        });

        self.slab[parent_id].children.push(id);

        id
    }

    pub fn add_node_to_current(&mut self, node: ConfigSnapshotState) -> NodeId {
        let parent = self.current_id;
        self.add_node(parent, node)
    }

    pub fn push_node(&mut self, node: ConfigSnapshotState) -> NodeId {
        let id = self.add_node_to_current(node);
        self.set_current(id);
        id
    }

    pub fn add_leaf_from_subtree(
        &mut self,
        parent_id: NodeId,
        subtree: ConfigSnapshotsBuilder,
    ) -> Vec<NodeId> {
        let subtree = subtree.build();
        self.add_leaf(parent_id, subtree.next.unwrap_or_default())
    }

    pub fn add_leaf(
        &mut self,
        parent_id: NodeId,
        children: Vec<ConfigSnapshotState>,
    ) -> Vec<NodeId> {
        let mut ids = Vec::with_capacity(children.len());
        let mut queue = VecDeque::from_iter([(parent_id, children)]);

        while let Some((parent_id, children)) = queue.pop_front() {
            let mut children_ids = Vec::new();
            for mut child in children {
                let grand_children = child.next.take();

                let id = self.slab.insert(BuilderNode {
                    state: child,
                    children: Vec::new(),
                });

                if let Some(grand_children) = grand_children {
                    queue.push_back((id, grand_children));
                }

                children_ids.push(id);
            }

            self.slab[parent_id]
                .children
                .extend(children_ids.iter().copied());
            ids.extend(children_ids);
        }

        ids
    }

    pub fn add_leaf_to_current(&mut self, children: Vec<ConfigSnapshotState>) -> Vec<NodeId> {
        let parent = self.current_id;
        self.add_leaf(parent, children)
    }

    pub fn set_current(&mut self, id: NodeId) {
        self.current_id = id;
    }

    fn build_node(&mut self, id: NodeId) -> ConfigSnapshotState {
        let BuilderNode {
            mut state,
            children,
        } = self.slab.remove(id);

        if children.is_empty() {
            state.next = None;
        } else {
            let next_children = children
                .into_iter()
                .map(|child_id| self.build_node(child_id))
                .collect();
            state.next = Some(next_children);
        }

        state
    }

    pub fn build(mut self) -> ConfigSnapshotState {
        self.build_node(self.root_id)
    }
}
