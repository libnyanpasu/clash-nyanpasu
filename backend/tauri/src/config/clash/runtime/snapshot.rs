//! A snapshot of the clash config processing

use serde_yaml::Mapping;
use slab::Slab;
use std::collections::BTreeSet;

use crate::config::profile::item_type::ProfileItemType;

/// A field in the config, use `.` to represent nested fields
pub type ConfigField = String;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigSnapshot {
    /// Current snapshot of the config
    pub config: Mapping,
    /// The changed fields compared to the previous snapshot
    pub changed_fields: Option<BTreeSet<ConfigField>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ProcessKind {
    Root {
        primary_profile_id: String,
    },
    SecondarySelectedProxiesMerge {
        primary_profile_id: String,
        other_profiles_ids: Vec<String>,
    },
    ChainNode {
        profile_id: String,
        profile_kind: ProfileItemType,
    },
    BuiltinChain {
        name: String,
    },
    GuardOverrides,
    WhitelistFieldFilter,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigSnapshotState {
    pub snapshot: ConfigSnapshot,
    /// The kind of process that generated this snapshot
    pub process_kind: ProcessKind,
    pub next: Option<Vec<ConfigSnapshotState>>,
}

pub struct ConfigSnapshotsBuilder {
    pub slab: Slab<ConfigSnapshotState>,
    pub root_id: usize,
    /// The current processing snapshot id
    pub current_id: usize,
    /// Relations between snapshots in the slab, (parent, child),
    pub relations: Vec<(usize, Vec<usize>)>,
}

impl ConfigSnapshotsBuilder {
    pub fn new(root_snapshot: ConfigSnapshot, primary_profile_id: String) -> Self {
        let mut slab = Slab::new();
        let root_state = ConfigSnapshotState {
            snapshot: root_snapshot,
            process_kind: ProcessKind::Root { primary_profile_id },
            next: None,
        };
        let root_id = slab.insert(root_state);
        Self {
            slab,
            root_id,
            current_id: root_id,
            relations: Vec::with_capacity(32),
        }
    }

    /// Add leaf snapshots to the current snapshot node
    pub fn add_leaf(&mut self, parent_id: usize, children: Vec<ConfigSnapshotState>) {
        let mut child_ids = Vec::with_capacity(children.len());
        for child in children {
            let child_id = self.slab.insert(child);
            child_ids.push(child_id);
        }
        self.relations.push((parent_id, child_ids.clone()));
    }

    pub fn build(mut self) -> ConfigSnapshotState {
        // Build the tree structure from the relations
        for (parent_id, child_ids) in self.relations.into_iter().rev() {
            let mut children = Vec::with_capacity(child_ids.len());
            for child_id in child_ids {
                let child = self.slab.remove(child_id);
                children.push(child);
            }
            let parent = self
                .slab
                .get_mut(parent_id)
                .expect("parent node is missing, possible data corruption?");
            parent.next = Some(children);
        }
        self.slab.remove(self.root_id)
    }
}
