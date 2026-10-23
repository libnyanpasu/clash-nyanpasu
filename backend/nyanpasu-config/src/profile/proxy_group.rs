use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;

/// A user-named group of proxy groups in the Proxies sidebar. Clash identifies a
/// proxy group only by its name, so members are names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyGroupFolder {
    pub id: FolderId,
    pub name: String,
    /// Display order inside the folder.
    pub members: Vec<String>,
}

/// How a Config profile's proxy groups are arranged in the sidebar: the groups
/// in no folder, then the folders.
///
/// The groups come from the running core, not from the profile document, so a
/// layout may name groups that no longer exist and may not name new ones. It is
/// presentation only: [`ProxyGroupLayout::resolve`] reconciles it with the live
/// groups, and nothing here touches the runtime config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyGroupLayout {
    pub folders: Vec<ProxyGroupFolder>,
    pub ungrouped: Vec<String>,
}

/// A [`ProxyGroupLayout`] reconciled with the live groups: every live group
/// appears exactly once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedProxyGroupLayout {
    pub folders: Vec<ProxyGroupFolder>,
    pub ungrouped: Vec<String>,
}

impl ResolvedProxyGroupLayout {
    /// The groups in sidebar order, top to bottom: the groups in no folder, then
    /// each folder's members.
    pub fn flat_order(&self) -> Vec<String> {
        self.ungrouped
            .iter()
            .chain(self.folders.iter().flat_map(|folder| &folder.members))
            .cloned()
            .collect()
    }
}

/// What [`Profiles::repair_proxy_group_layouts`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyGroupRepairReport {
    /// Layouts removed because the profile is missing or is not a Config.
    pub dropped_layouts: Vec<ProfileId>,
    /// Folders removed because an earlier folder of the layout used their id.
    pub dropped_folders: Vec<(ProfileId, FolderId)>,
    /// Names removed because they are empty or already placed earlier.
    pub dropped_members: Vec<(ProfileId, String)>,
    /// Folders whose empty name was replaced.
    pub renamed: Vec<(ProfileId, FolderId)>,
}

impl ProxyGroupRepairReport {
    pub fn is_empty(&self) -> bool {
        self.dropped_layouts.is_empty()
            && self.dropped_folders.is_empty()
            && self.dropped_members.is_empty()
            && self.renamed.is_empty()
    }
}

/// Name given to a proxy group folder whose stored name is empty.
pub const REPAIRED_PROXY_GROUP_FOLDER_NAME: &str = "Folder";

impl ProxyGroupLayout {
    /// Whether the layout places nothing, which is the same as having none.
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.ungrouped.is_empty()
    }

    /// Reconcile the layout with the groups the core reports, in its order.
    ///
    /// - A stored name that is not live is ignored, not forgotten: a group that
    ///   disappears for one refresh keeps its place when it returns.
    /// - A live name the layout does not place is appended to the ungrouped
    ///   groups, in the core's order.
    /// - A name placed twice keeps its first place, ungrouped before folders.
    pub fn resolve(&self, live: &[String]) -> ResolvedProxyGroupLayout {
        let live_names: IndexSet<&str> = live.iter().map(String::as_str).collect();
        let mut placed: IndexSet<String> = IndexSet::new();
        let mut place = |name: &String| -> bool {
            live_names.contains(name.as_str()) && placed.insert(name.clone())
        };

        let mut ungrouped: Vec<String> = self
            .ungrouped
            .iter()
            .filter(|name| place(name))
            .cloned()
            .collect();

        let folders = self
            .folders
            .iter()
            .map(|folder| ProxyGroupFolder {
                id: folder.id.clone(),
                name: folder.name.clone(),
                members: folder
                    .members
                    .iter()
                    .filter(|name| place(name))
                    .cloned()
                    .collect(),
            })
            .collect();

        for name in live {
            if place(name) {
                ungrouped.push(name.clone());
            }
        }

        ResolvedProxyGroupLayout { folders, ungrouped }
    }

    /// Make the layout valid: keep the first folder of a duplicated id, drop
    /// empty and repeated names, and name unnamed folders.
    fn repair(&mut self, profile: &ProfileId, report: &mut ProxyGroupRepairReport) {
        let mut folder_ids = IndexSet::new();
        let mut placed: IndexSet<String> = IndexSet::new();
        let mut keep = |name: &String, report: &mut ProxyGroupRepairReport| -> bool {
            let keep = !name.trim().is_empty() && placed.insert(name.clone());
            if !keep {
                report.dropped_members.push((profile.clone(), name.clone()));
            }
            keep
        };

        self.ungrouped.retain(|name| keep(name, report));

        let mut folders = Vec::with_capacity(self.folders.len());
        for mut folder in std::mem::take(&mut self.folders) {
            if !folder_ids.insert(folder.id.clone()) {
                report.dropped_folders.push((profile.clone(), folder.id));
                continue;
            }
            folder.members.retain(|name| keep(name, report));
            if folder.name.trim().is_empty() {
                folder.name = REPAIRED_PROXY_GROUP_FOLDER_NAME.to_owned();
                report.renamed.push((profile.clone(), folder.id.clone()));
            }
            folders.push(folder);
        }
        self.folders = folders;
    }
}

impl Profiles {
    /// Make `proxy_group_layouts` valid: drop layouts of profiles that are not
    /// Configs, then repair each layout (see [`ProxyGroupLayout`]).
    pub fn repair_proxy_group_layouts(&mut self) -> ProxyGroupRepairReport {
        let mut report = ProxyGroupRepairReport::default();

        let mut layouts = std::mem::take(&mut self.proxy_group_layouts);
        layouts.retain(|profile, _| {
            let keep = self
                .items
                .get(profile)
                .is_some_and(|item| item.definition.is_config());
            if !keep {
                report.dropped_layouts.push(profile.clone());
            }
            keep
        });
        for (profile, layout) in &mut layouts {
            layout.repair(profile, &mut report);
        }
        self.proxy_group_layouts = layouts;

        report
    }
}

pub(super) fn validate_proxy_group_layouts(
    items: &IndexMap<ProfileId, ProfileItem>,
    layouts: &IndexMap<ProfileId, ProxyGroupLayout>,
    errors: &mut Vec<ProfileValidationError>,
) {
    for (profile, layout) in layouts {
        if !items
            .get(profile)
            .is_some_and(|item| item.definition.is_config())
        {
            errors.push(ProfileValidationError::ProxyGroupLayoutProfileNotConfig {
                profile: profile.clone(),
            });
        }

        let mut folder_ids = IndexSet::new();
        let mut placed: IndexSet<String> = IndexSet::new();
        let mut check_name = |name: &String, errors: &mut Vec<ProfileValidationError>| {
            if name.trim().is_empty() {
                errors.push(ProfileValidationError::ProxyGroupNameEmpty {
                    profile: profile.clone(),
                });
            }
            if !placed.insert(name.clone()) {
                errors.push(ProfileValidationError::ProxyGroupDuplicate {
                    profile: profile.clone(),
                    name: name.clone(),
                });
            }
        };

        for name in &layout.ungrouped {
            check_name(name, errors);
        }
        for folder in &layout.folders {
            if !folder_ids.insert(&folder.id) {
                errors.push(ProfileValidationError::ProxyGroupFolderDuplicateId {
                    profile: profile.clone(),
                    id: folder.id.clone(),
                });
            }
            if folder.name.trim().is_empty() {
                errors.push(ProfileValidationError::ProxyGroupFolderNameEmpty {
                    profile: profile.clone(),
                    id: folder.id.clone(),
                });
            }
            for name in &folder.members {
                check_name(name, errors);
            }
        }
    }
}
