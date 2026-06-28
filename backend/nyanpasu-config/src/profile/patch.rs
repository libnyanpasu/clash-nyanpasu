//! Specialized mutators for the profile model.
//!
//! Enum-variant transitions (`Local <-> Remote`, `File <-> Composition`,
//! `Overlay <-> Script`) are done by atomic replacement, never field-level
//! patch, so the model never holds an illegal intermediate state. Leaf structs
//! (`ProfileMetadata`, `RemoteProfileOptions`) keep their struct-patch types.
use struct_patch::Patch;

use super::*;

/// Append `uid` unless already present. Returns whether it was added.
pub fn list_add(list: &mut Vec<ProfileId>, uid: ProfileId) -> bool {
    if list.contains(&uid) {
        false
    } else {
        list.push(uid);
        true
    }
}

/// Remove the first occurrence of `uid`. Returns whether anything was removed.
pub fn list_remove(list: &mut Vec<ProfileId>, uid: &ProfileId) -> bool {
    if let Some(pos) = list.iter().position(|existing| existing == uid) {
        list.remove(pos);
        true
    } else {
        false
    }
}

/// Move the element at `from` to index `to`. No-op (`false`) when either index
/// is out of range or they are equal.
pub fn list_move(list: &mut Vec<ProfileId>, from: usize, to: usize) -> bool {
    if from >= list.len() || to >= list.len() || from == to {
        return false;
    }
    let item = list.remove(from);
    list.insert(to, item);
    true
}

impl Profiles {
    pub fn set_current(&mut self, uid: Option<ProfileId>) {
        self.current = uid;
    }

    pub fn clear_current(&mut self) {
        self.current = None;
    }

    pub fn set_valid(&mut self, valid: Vec<String>) {
        self.valid = valid;
    }

    pub fn add_global_transform(&mut self, uid: ProfileId) -> bool {
        list_add(&mut self.global_transforms, uid)
    }

    pub fn remove_global_transform(&mut self, uid: &ProfileId) -> bool {
        list_remove(&mut self.global_transforms, uid)
    }

    pub fn move_global_transform(&mut self, from: usize, to: usize) -> bool {
        list_move(&mut self.global_transforms, from, to)
    }
}

impl ProfileItem {
    pub fn apply_metadata_patch(&mut self, patch: ProfileMetadataPatch) {
        self.metadata.apply(patch);
    }

    /// Atomically replace the whole definition (kind / source / binding switch).
    pub fn set_definition(&mut self, definition: ProfileDefinition) {
        self.definition = definition;
    }

    /// Replace the source in place. Returns `false` for definitions without a
    /// source (a `CompositionConfig` has no materialized source).
    pub fn set_source(&mut self, source: ProfileSource) -> bool {
        match self.definition.source_mut() {
            Some(slot) => {
                *slot = source;
                true
            }
            None => false,
        }
    }
}

impl CompositionConfig {
    pub fn set_base(&mut self, base: Option<ProfileId>) {
        self.base = base;
    }

    pub fn add_contributor(&mut self, uid: ProfileId) -> bool {
        list_add(&mut self.extend_proxies_from, uid)
    }

    pub fn remove_contributor(&mut self, uid: &ProfileId) -> bool {
        list_remove(&mut self.extend_proxies_from, uid)
    }

    pub fn move_contributor(&mut self, from: usize, to: usize) -> bool {
        list_move(&mut self.extend_proxies_from, from, to)
    }
}
