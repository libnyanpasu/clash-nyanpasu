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
pub struct ConfigSnapshotState<C> {
    pub snapshot: ConfigSnapshot,
    /// The kind of process that generated this snapshot
    pub process_kind: ProcessKind,
    pub next: Option<Vec<C>>,
}

impl<C> ConfigSnapshotState<C> {
    pub fn new(snapshot: ConfigSnapshot, process_kind: ProcessKind, next: Option<Vec<C>>) -> Self {
        Self {
            snapshot,
            process_kind,
            next,
        }
    }
}

pub type Idx = usize;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigSnapshotsGraph {
    pub nodes: Vec<ConfigSnapshotState<Idx>>,
    /// edges as (from, to)
    pub edges: Vec<(Idx, Idx)>,
    pub root_id: Idx,
}

pub struct ConfigSnapshotTreeNode {
    pub node: ConfigSnapshotState<ConfigSnapshotTreeNode>,
}

type NodeId = usize;

#[derive(Debug, Clone)]
struct BuilderNode {
    state: ConfigSnapshotState<NodeId>,
    parent_id: Option<NodeId>,
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
            parent_id: None,
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

    pub fn add_node(&mut self, parent_id: NodeId, node: ConfigSnapshotState<NodeId>) -> NodeId {
        let id = self.slab.insert(BuilderNode {
            state: node,
            parent_id: Some(parent_id),
            children: Vec::new(),
        });

        self.slab[parent_id].children.push(id);

        id
    }

    pub fn add_node_to_current(&mut self, node: ConfigSnapshotState<NodeId>) -> NodeId {
        let parent = self.current_id;
        self.add_node(parent, node)
    }

    pub fn push_node(&mut self, node: ConfigSnapshotState<NodeId>) -> NodeId {
        let id = self.add_node_to_current(node);
        self.set_current(id);
        id
    }

    pub fn add_leaf_from_subtree(
        &mut self,
        parent_id: NodeId,
        subtree: ConfigSnapshotsBuilder,
    ) -> Vec<NodeId> {
        let subtree = subtree.build_tree();
        self.add_leaf(parent_id, subtree.node.next.unwrap_or_default())
    }

    pub fn add_leaf(
        &mut self,
        parent_id: NodeId,
        children: Vec<ConfigSnapshotTreeNode>,
    ) -> Vec<NodeId> {
        let mut ids = Vec::with_capacity(children.len());
        let mut queue = VecDeque::from_iter([(parent_id, children)]);

        while let Some((parent_id, children)) = queue.pop_front() {
            let mut children_ids = Vec::new();
            for mut child in children {
                let grand_children = child.node.next.take();

                let id = self.slab.insert(BuilderNode {
                    state: ConfigSnapshotState {
                        snapshot: child.node.snapshot,
                        process_kind: child.node.process_kind,
                        next: None,
                    },
                    parent_id: Some(parent_id),
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

    pub fn add_edge(&mut self, from: NodeId, to: NodeId) {
        self.slab[from].children.push(to);
        self.slab[to].parent_id = Some(from);
    }

    pub fn add_leaf_to_current(&mut self, children: Vec<ConfigSnapshotTreeNode>) -> Vec<NodeId> {
        let parent = self.current_id;
        self.add_leaf(parent, children)
    }

    pub fn set_current(&mut self, id: NodeId) {
        self.current_id = id;
    }

    fn build_tree_node(slab: &mut Slab<BuilderNode>, id: NodeId) -> ConfigSnapshotTreeNode {
        let BuilderNode {
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
            node: ConfigSnapshotState {
                snapshot: state.snapshot,
                process_kind: state.process_kind,
                next,
            },
        }
    }

    pub fn build_tree(mut self) -> ConfigSnapshotTreeNode {
        use std::collections::HashMap;

        let node_ids: Vec<NodeId> = self.slab.iter().map(|(idx, _)| idx).collect();
        if !node_ids.contains(&self.root_id) {
            panic!(
                "cannot build tree: root {root} not found",
                root = self.root_id
            );
        }

        // Validate it is a proper tree: single root, no multi-parent, no cycles, all reachable.
        let mut indegree: HashMap<NodeId, usize> = HashMap::new();
        for (idx, node) in self.slab.iter() {
            for &child in &node.children {
                if self.slab.get(child).is_none() {
                    panic!("cannot build tree: child node {child} is missing");
                }

                if child == idx {
                    panic!("cannot build tree: self-cycle at node {idx}");
                }

                let count = indegree.entry(child).or_insert(0);
                *count += 1;
                if *count > 1 {
                    panic!("cannot build tree: node {child} has multiple parents");
                }
            }
        }

        let roots: Vec<NodeId> = node_ids
            .iter()
            .copied()
            .filter(|id| indegree.get(id).copied().unwrap_or(0) == 0)
            .collect();
        if roots.len() != 1 || roots[0] != self.root_id {
            panic!("cannot build tree: multiple roots detected");
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum VisitState {
            Unvisited,
            Visiting,
            Done,
        }

        let mut visit_state: HashMap<NodeId, VisitState> = node_ids
            .iter()
            .map(|&idx| (idx, VisitState::Unvisited))
            .collect();

        fn dfs(idx: NodeId, slab: &Slab<BuilderNode>, state: &mut HashMap<NodeId, VisitState>) {
            match state.get(&idx).copied() {
                Some(VisitState::Visiting) => {
                    panic!("cannot build tree: cycle detected at node {idx}")
                }
                Some(VisitState::Done) => return,
                _ => {}
            }

            state.insert(idx, VisitState::Visiting);
            for &child in &slab[idx].children {
                dfs(child, slab, state);
            }
            state.insert(idx, VisitState::Done);
        }

        dfs(self.root_id, &self.slab, &mut visit_state);
        if visit_state.values().any(|&s| s != VisitState::Done) {
            panic!("cannot build tree: unreachable nodes present");
        }

        Self::build_tree_node(&mut self.slab, self.root_id)
    }

    pub fn build(self) -> ConfigSnapshotsGraph {
        let max_idx = self.slab.iter().map(|(idx, _)| idx).max().unwrap_or(0);
        let mut nodes: Vec<Option<ConfigSnapshotState<NodeId>>> = vec![None; max_idx + 1];
        let mut edges = Vec::with_capacity(self.slab.len().saturating_sub(1));

        for (idx, builder_node) in self.slab.into_iter() {
            for &child in &builder_node.children {
                edges.push((idx, child));
            }

            let next = if builder_node.children.is_empty() {
                None
            } else {
                Some(builder_node.children.clone())
            };

            nodes[idx] = Some(ConfigSnapshotState {
                snapshot: builder_node.state.snapshot,
                process_kind: builder_node.state.process_kind,
                next,
            });
        }

        if nodes.iter().any(|node| node.is_none()) {
            panic!("cannot build graph: sparse node ids present");
        }

        ConfigSnapshotsGraph {
            nodes: nodes
                .into_iter()
                .map(|node| node.expect("node is present"))
                .collect(),
            edges,
            root_id: self.root_id,
        }
    }
}
