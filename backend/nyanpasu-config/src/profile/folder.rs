use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;

/// A user-named group of Config profiles. A Config profile belongs to at most
/// one folder; transforms are never members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProfileFolder {
    pub id: FolderId,
    pub name: String,
    /// Display order inside the folder.
    pub members: Vec<ProfileId>,
}

/// The complete arrangement of Config profiles: every Config profile appears
/// exactly once, either inside a folder or in `ungrouped`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProfileLayout {
    pub folders: Vec<ProfileFolder>,
    pub ungrouped: Vec<ProfileId>,
}

/// What [`Profiles::repair_folders`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderRepairReport {
    /// Memberships removed because the profile is missing, is not a Config, or
    /// was already placed in an earlier folder.
    pub dropped_members: Vec<(FolderId, ProfileId)>,
    /// Folders removed because an earlier folder already used their id.
    pub dropped_folders: Vec<FolderId>,
    /// Folders whose empty name was replaced.
    pub renamed: Vec<FolderId>,
}

impl FolderRepairReport {
    pub fn is_empty(&self) -> bool {
        self.dropped_members.is_empty()
            && self.dropped_folders.is_empty()
            && self.renamed.is_empty()
    }
}

/// Name given to a folder whose stored name is empty.
pub const REPAIRED_FOLDER_NAME: &str = "Folder";

impl Profiles {
    /// Replace the folders and rewrite `items` to the order the user sees:
    /// each folder's members in folder order, the ungrouped Configs, then the
    /// transforms in their existing relative order.
    ///
    /// The layout must place every Config profile exactly once. The document is
    /// left unchanged when it does not.
    pub fn apply_layout(
        &mut self,
        layout: ProfileLayout,
    ) -> Result<(), Vec<ProfileValidationError>> {
        let mut errors = Vec::new();
        validate_folders(&self.items, &layout.folders, &mut errors);

        let mut placed: IndexSet<&ProfileId> = layout
            .folders
            .iter()
            .flat_map(|folder| &folder.members)
            .collect();
        for profile in &layout.ungrouped {
            match self.items.get(profile) {
                Some(item) if item.definition.is_config() => {}
                _ => {
                    errors.push(ProfileValidationError::LayoutUnknownProfile {
                        profile: profile.clone(),
                    });
                    continue;
                }
            }
            if !placed.insert(profile) {
                errors.push(ProfileValidationError::FolderMemberDuplicate {
                    profile: profile.clone(),
                });
            }
        }
        for (uid, item) in &self.items {
            if item.definition.is_config() && !placed.contains(uid) {
                errors.push(ProfileValidationError::LayoutMissingProfile {
                    profile: uid.clone(),
                });
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut previous = std::mem::take(&mut self.items);
        let mut items = IndexMap::with_capacity(previous.len());
        let ordered = layout
            .folders
            .iter()
            .flat_map(|folder| &folder.members)
            .chain(&layout.ungrouped);
        for uid in ordered {
            if let Some(item) = previous.shift_remove(uid) {
                items.insert(uid.clone(), item);
            }
        }
        items.extend(previous);

        self.items = items;
        self.folders = layout.folders;
        Ok(())
    }

    /// Make `folders` valid without touching `items`: keep the first folder of
    /// a duplicated id, drop members that are missing, not Config profiles, or
    /// already placed earlier, and name unnamed folders.
    pub fn repair_folders(&mut self) -> FolderRepairReport {
        let mut report = FolderRepairReport::default();
        let mut folder_ids = IndexSet::new();
        let mut placed = IndexSet::new();
        let mut repaired = Vec::with_capacity(self.folders.len());

        for mut folder in std::mem::take(&mut self.folders) {
            if !folder_ids.insert(folder.id.clone()) {
                report.dropped_folders.push(folder.id);
                continue;
            }

            folder.members.retain(|member| {
                let is_config = self
                    .items
                    .get(member)
                    .is_some_and(|item| item.definition.is_config());
                let keep = is_config && placed.insert(member.clone());
                if !keep {
                    report
                        .dropped_members
                        .push((folder.id.clone(), member.clone()));
                }
                keep
            });

            if folder.name.trim().is_empty() {
                folder.name = REPAIRED_FOLDER_NAME.to_owned();
                report.renamed.push(folder.id.clone());
            }
            repaired.push(folder);
        }

        self.folders = repaired;
        report
    }
}

pub(super) fn validate_folders(
    items: &IndexMap<ProfileId, ProfileItem>,
    folders: &[ProfileFolder],
    errors: &mut Vec<ProfileValidationError>,
) {
    let mut folder_ids = IndexSet::new();
    let mut placed = IndexSet::new();

    for folder in folders {
        if !folder_ids.insert(&folder.id) {
            errors.push(ProfileValidationError::FolderDuplicateId {
                id: folder.id.clone(),
            });
        }
        if folder.name.trim().is_empty() {
            errors.push(ProfileValidationError::FolderNameEmpty {
                id: folder.id.clone(),
            });
        }

        for member in &folder.members {
            match items.get(member) {
                None => errors.push(ProfileValidationError::FolderMemberNotFound {
                    folder: folder.id.clone(),
                    profile: member.clone(),
                }),
                Some(item) if !item.definition.is_config() => {
                    errors.push(ProfileValidationError::FolderMemberNotConfig {
                        folder: folder.id.clone(),
                        profile: member.clone(),
                    })
                }
                Some(_) => {}
            }
            if !placed.insert(member) {
                errors.push(ProfileValidationError::FolderMemberDuplicate {
                    profile: member.clone(),
                });
            }
        }
    }
}
