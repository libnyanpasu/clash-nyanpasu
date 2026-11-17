//! The one classification of every application path, and the syncable items it
//! produces.
//!
//! Nothing is synced because of where it sits: only a [`PathClass::Portable`]
//! row with a [`SyncRule`] can produce a [`SyncItem`], and a file no row names
//! never leaves the device. The table is a constant and this module performs no
//! platform-conditional code; enumeration is the only IO and reads metadata
//! through [`std::fs::symlink_metadata`].

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
};

use camino::{Utf8Path, Utf8PathBuf};
use snafu::Snafu;

use crate::LogicalPath;

/// What an application path is for.
///
/// Exactly one class describes a path. Only [`PathClass::Portable`] entries can
/// sync; the others exist so backup, diagnostics and sync share one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathClass {
    /// Syncable configuration or user asset.
    Portable,
    /// Configuration or bookkeeping bound to this device.
    Device,
    /// Durable local state: stores, history, the sync baseline and backups.
    State,
    /// Rebuilt from configuration: runtime documents, sockets and the PID.
    Derived,
    /// Regenerable downloads and script runtime directories.
    Cache,
    Logs,
    /// Credentials. Never synced, backed up or archived for diagnostics.
    Secret,
    /// Core binaries, geodata and what ships beside a core.
    Binary,
    /// Inputs that only migrations read.
    Legacy,
}

/// The root a [`PathEntry`] is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Config,
    Data,
}

/// Which configuration document a portable projection encodes.
///
/// The projection itself is implemented by `nyanpasu-config::sync`, which owns
/// the matching [`DocumentKind`](https://docs.rs/nyanpasu-config) value. The
/// paths crate names the kind here so the manifest stays free of config's
/// dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDocumentKind {
    Application,
    Clash,
    Profiles,
}

/// How an item's portable bytes are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemCodec {
    /// Portable projection of a configuration document, canonical JSON.
    Document(SyncDocumentKind),
    /// UTF-8 text, LF line endings, no BOM.
    Text,
    /// Opaque bytes.
    Binary,
}

/// How many files a row contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    /// The entry is one file.
    File,
    /// Every regular file under the directory.
    Tree,
    /// Only the files the caller selects, such as non-remote profile files.
    SelectedTree,
}

/// How a portable row becomes sync items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncRule {
    /// The logical key of a [`Membership::File`] item, or the logical prefix a
    /// tree's files are joined to.
    pub base: &'static str,
    pub codec: ItemCodec,
    pub membership: Membership,
}

/// One row of the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathEntry {
    /// A stable name for tests and diagnostics, such as `application`.
    pub id: &'static str,
    pub class: PathClass,
    pub root: Root,
    /// Forward-slash relative path; an empty string names the root itself.
    pub relative: &'static str,
    /// `Some` exactly when `class == PathClass::Portable`.
    pub sync: Option<SyncRule>,
}

impl PathEntry {
    const fn device(id: &'static str, root: Root, relative: &'static str) -> Self {
        Self {
            id,
            class: PathClass::Device,
            root,
            relative,
            sync: None,
        }
    }

    const fn other(id: &'static str, class: PathClass, root: Root, relative: &'static str) -> Self {
        Self {
            id,
            class,
            root,
            relative,
            sync: None,
        }
    }

    const fn portable(
        id: &'static str,
        root: Root,
        relative: &'static str,
        sync: SyncRule,
    ) -> Self {
        Self {
            id,
            class: PathClass::Portable,
            root,
            relative,
            sync: Some(sync),
        }
    }
}

const fn document(base: &'static str, kind: SyncDocumentKind) -> SyncRule {
    SyncRule {
        base,
        codec: ItemCodec::Document(kind),
        membership: Membership::File,
    }
}

const fn tree(base: &'static str, codec: ItemCodec) -> SyncRule {
    SyncRule {
        base,
        codec,
        membership: Membership::Tree,
    }
}

const fn selected_tree(base: &'static str, codec: ItemCodec) -> SyncRule {
    SyncRule {
        base,
        codec,
        membership: Membership::SelectedTree,
    }
}

/// Every path the application knows, with its class.
///
/// The runtime directory is still the pre-move `<config>/runtime` location;
/// relocating it to `<data>/runtime` is a separate layout migration that does
/// not change its class. Paths discovered by name, such as core binaries and
/// geodata, are [`PathClass::Binary`] by convention rather than by row.
pub const MANIFEST: &[PathEntry] = &[
    PathEntry::portable(
        "application",
        Root::Config,
        "application.yaml",
        document("app/application", SyncDocumentKind::Application),
    ),
    PathEntry::portable(
        "clash",
        Root::Config,
        "clash-config.yaml",
        document("app/clash", SyncDocumentKind::Clash),
    ),
    PathEntry::portable(
        "profiles_catalog",
        Root::Config,
        "profiles.yaml",
        document("app/profiles", SyncDocumentKind::Profiles),
    ),
    PathEntry::portable(
        "profile_files",
        Root::Config,
        "profiles",
        selected_tree("profiles", ItemCodec::Text),
    ),
    PathEntry::other(
        "profile_materialization",
        PathClass::Device,
        Root::Config,
        "profiles/.profile-materialization-v1",
    ),
    PathEntry::portable(
        "icons",
        Root::Config,
        "icons",
        tree("icons", ItemCodec::Binary),
    ),
    PathEntry::device("sync_settings", Root::Config, "local/sync.yaml"),
    // Relocating this under `local/` is a layout migration of its own; the
    // class does not depend on where the file sits.
    PathEntry::device("session_state", Root::Config, "session-state.yaml"),
    PathEntry::device("migration_state", Root::Config, "migration-state.yaml"),
    PathEntry::other("runtime", PathClass::Derived, Root::Config, "runtime"),
    PathEntry::other(
        "legacy_nyanpasu_config",
        PathClass::Legacy,
        Root::Config,
        "nyanpasu-config.yaml",
    ),
    PathEntry::other(
        "legacy_clash_guard_overrides",
        PathClass::Legacy,
        Root::Config,
        "clash-guard-overrides.yaml",
    ),
    PathEntry::other(
        "legacy_typed_clash",
        PathClass::Legacy,
        Root::Config,
        "clash.yaml",
    ),
    PathEntry::other("storage", PathClass::State, Root::Data, "storage.db"),
    PathEntry::other("jobs", PathClass::State, Root::Data, "jobs.redb"),
    PathEntry::other("backups", PathClass::State, Root::Data, "backups"),
    PathEntry::other("logs", PathClass::Logs, Root::Data, "logs"),
    PathEntry::other("core_logs", PathClass::Logs, Root::Data, "logs/core"),
    PathEntry::other(
        "migration_log",
        PathClass::Logs,
        Root::Data,
        "migration.log",
    ),
    PathEntry::other("cache", PathClass::Cache, Root::Data, "cache"),
    PathEntry::other("icon_cache", PathClass::Cache, Root::Data, "cache/icons"),
    PathEntry::other(
        "geodata_cache",
        PathClass::Cache,
        Root::Data,
        "cache/geodata",
    ),
    PathEntry::other("pac_script", PathClass::Cache, Root::Data, "cache/pac.js"),
    PathEntry::other("traffic", PathClass::State, Root::Data, "traffic"),
    PathEntry::other("scripts", PathClass::Cache, Root::Data, "scripts"),
    PathEntry::other("cores", PathClass::Binary, Root::Data, "cores"),
    PathEntry::other("clash_pid", PathClass::Derived, Root::Data, "clash.pid"),
    PathEntry::other(
        "remote_access",
        PathClass::Secret,
        Root::Data,
        "remote-access",
    ),
    PathEntry::other(
        "sync_state",
        PathClass::State,
        Root::Data,
        "sync/state.redb",
    ),
    PathEntry::other(
        "sync_credentials",
        PathClass::Secret,
        Root::Data,
        "sync/credentials.json",
    ),
    PathEntry::other("sync_staging", PathClass::Cache, Root::Data, "sync/staging"),
    PathEntry::other(
        "sync_snapshots",
        PathClass::State,
        Root::Data,
        "sync/snapshots",
    ),
];

/// One file that leaves the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncItem {
    pub logical: LogicalPath,
    pub physical: Utf8PathBuf,
    pub codec: ItemCodec,
}

/// A file or name that was present but cannot sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// A name that is not a valid logical segment on every platform.
    UnportableName { physical: Utf8PathBuf },
    /// Two names equal under ASCII case folding: a case-insensitive volume
    /// cannot hold both, so neither is synced.
    CaseCollision {
        logical: LogicalPath,
        physical: Vec<Utf8PathBuf>,
    },
    /// A symlink or other non-regular entry.
    Link { physical: Utf8PathBuf },
    /// A [`Membership::SelectedTree`] file the caller did not select.
    NotSelected { physical: Utf8PathBuf },
}

/// The portable items present on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncEnumeration {
    pub items: Vec<SyncItem>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum EnumerateError {
    #[snafu(display("failed to read {path}"))]
    ReadDir {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("failed to read the metadata of {path}"))]
    Metadata {
        path: Utf8PathBuf,
        source: io::Error,
    },
}

impl crate::PathResolver {
    /// The manifest row for `id`, or `None` when no row has that name.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&'static PathEntry> {
        MANIFEST.iter().find(|entry| entry.id == id)
    }

    /// The physical path of a manifest row.
    #[must_use]
    pub fn resolve(&self, entry: &PathEntry) -> Utf8PathBuf {
        let root = match entry.root {
            Root::Config => self.app_config_dir(),
            Root::Data => self.app_data_dir(),
        };
        join_relative(root, entry.relative)
    }

    /// The syncable items present on disk, sorted by logical path.
    ///
    /// `.`-prefixed components are skipped silently: they cover materialization
    /// state, atomic-write temp dirs and editor droppings without a list.
    ///
    /// # Errors
    ///
    /// Fails when a portable tree cannot be read. A missing tree is empty.
    pub fn enumerate_sync_items(
        &self,
        selected: &BTreeSet<LogicalPath>,
    ) -> Result<SyncEnumeration, EnumerateError> {
        let mut enumeration = SyncEnumeration::default();
        for entry in MANIFEST.iter().filter(|entry| entry.sync.is_some()) {
            let Some(rule) = entry.sync else {
                continue;
            };
            let path = self.resolve(entry);
            match rule.membership {
                Membership::File => self.collect_file(rule, &path, &mut enumeration),
                Membership::Tree | Membership::SelectedTree => {
                    self.collect_tree(rule, &path, selected, &mut enumeration)?;
                }
            }
        }
        drop_collisions(&mut enumeration);
        enumeration
            .items
            .sort_by(|left, right| left.logical.cmp(&right.logical));
        enumeration.skipped.sort_by_key(skip_key);
        Ok(enumeration)
    }

    fn collect_file(&self, rule: SyncRule, path: &Utf8Path, enumeration: &mut SyncEnumeration) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if metadata.file_type().is_symlink() {
            enumeration.skipped.push(Skipped::Link {
                physical: path.to_owned(),
            });
            return;
        }
        if !metadata.is_file() {
            return;
        }
        let Ok(logical) = LogicalPath::parse(rule.base) else {
            enumeration.skipped.push(Skipped::UnportableName {
                physical: path.to_owned(),
            });
            return;
        };
        enumeration.items.push(SyncItem {
            logical,
            physical: path.to_owned(),
            codec: rule.codec,
        });
    }

    fn collect_tree(
        &self,
        rule: SyncRule,
        root: &Utf8Path,
        selected: &BTreeSet<LogicalPath>,
        enumeration: &mut SyncEnumeration,
    ) -> Result<(), EnumerateError> {
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(EnumerateError::ReadDir {
                        path: directory,
                        source: error,
                    });
                }
            };
            for entry in entries {
                let entry = entry.map_err(|source| EnumerateError::ReadDir {
                    path: directory.clone(),
                    source,
                })?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    let physical = entry.path();
                    enumeration.skipped.push(Skipped::UnportableName {
                        physical: Utf8PathBuf::from_path_buf(physical)
                            .unwrap_or_else(|_| directory.clone()),
                    });
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let physical = directory.join(name);
                let metadata =
                    fs::symlink_metadata(&physical).map_err(|source| EnumerateError::Metadata {
                        path: physical.clone(),
                        source,
                    })?;
                if metadata.file_type().is_symlink() {
                    enumeration.skipped.push(Skipped::Link { physical });
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(physical);
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }
                let Ok(logical) = LogicalPath::parse(rule.base).and_then(|base| base.join(name))
                else {
                    enumeration
                        .skipped
                        .push(Skipped::UnportableName { physical });
                    continue;
                };
                if rule.membership == Membership::SelectedTree && !selected.contains(&logical) {
                    enumeration.skipped.push(Skipped::NotSelected { physical });
                    continue;
                }
                enumeration.items.push(SyncItem {
                    logical,
                    physical,
                    codec: rule.codec,
                });
            }
        }
        Ok(())
    }
}

fn join_relative(root: &Utf8Path, relative: &str) -> Utf8PathBuf {
    let mut path = root.to_owned();
    for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
        path.push(segment);
    }
    path
}

/// Removes every item whose logical path collides under case folding.
fn drop_collisions(enumeration: &mut SyncEnumeration) {
    let mut by_fold: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, item) in enumeration.items.iter().enumerate() {
        by_fold
            .entry(item.logical.case_folded())
            .or_default()
            .push(index);
    }
    let mut removed: BTreeSet<usize> = BTreeSet::new();
    for indices in by_fold.values().filter(|indices| indices.len() > 1) {
        let mut physical: Vec<_> = indices
            .iter()
            .map(|index| enumeration.items[*index].physical.clone())
            .collect();
        physical.sort();
        enumeration.skipped.push(Skipped::CaseCollision {
            logical: enumeration.items[indices[0]].logical.clone(),
            physical,
        });
        removed.extend(indices);
    }
    if removed.is_empty() {
        return;
    }
    let mut kept = Vec::with_capacity(enumeration.items.len() - removed.len());
    for (index, item) in enumeration.items.drain(..).enumerate() {
        if !removed.contains(&index) {
            kept.push(item);
        }
    }
    enumeration.items = kept;
}

fn skip_key(skipped: &Skipped) -> String {
    match skipped {
        Skipped::UnportableName { physical } | Skipped::Link { physical } => physical.to_string(),
        Skipped::NotSelected { physical } => physical.to_string(),
        Skipped::CaseCollision { logical, .. } => logical.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::PathResolver;

    fn resolver(root: &Utf8Path) -> PathResolver {
        PathResolver::with_base_dirs(root.join("config"), root.join("data"))
    }

    fn write(path: &Utf8Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn portable_rows_are_the_only_rows_with_a_sync_rule() {
        for entry in MANIFEST {
            assert_eq!(
                entry.sync.is_some(),
                entry.class == PathClass::Portable,
                "{}",
                entry.id
            );
        }
    }

    #[test]
    fn row_ids_are_unique() {
        let mut ids: Vec<_> = MANIFEST.iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn every_public_path_method_is_covered_by_exactly_one_row() {
        let r = PathResolver::with_base_dirs("/cfg".into(), "/data".into());
        let paths = [
            ("application", r.application_config_path()),
            ("clash", r.clash_config_path()),
            ("profiles_catalog", r.profiles_path()),
            ("profile_files", r.app_profiles_dir()),
            ("session_state", r.session_state_path()),
            (
                "migration_state",
                r.resolve(r.entry("migration_state").unwrap()),
            ),
            ("runtime", r.resolve(r.entry("runtime").unwrap())),
            ("legacy_nyanpasu_config", r.nyanpasu_config_path()),
            (
                "legacy_clash_guard_overrides",
                r.clash_guard_overrides_path(),
            ),
            ("storage", r.storage_path()),
            ("jobs", r.jobs_path()),
            ("backups", r.backups_dir()),
            ("logs", r.app_logs_dir()),
            ("cache", r.cache_dir()),
            ("scripts", r.scripts_dir()),
            ("clash_pid", r.clash_pid_path()),
            ("remote_access", r.remote_access_dir()),
            ("cores", r.resolve(r.entry("cores").unwrap())),
        ];
        for (id, path) in paths {
            let matching: Vec<_> = MANIFEST
                .iter()
                .filter(|entry| r.resolve(entry) == path)
                .collect();
            assert_eq!(matching.len(), 1, "{id} resolves to {path}");
            assert_eq!(matching[0].id, id);
        }
    }

    #[test]
    fn enumerates_files_and_trees_with_logical_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).unwrap();
        let r = resolver(&root);
        write(&root.join("config/application.yaml"), "a: 1");
        write(&root.join("config/profiles/pK3mA9xZq2Lw.yaml"), "b: 2");
        write(&root.join("config/icons/tun.png"), "png");

        let selected = BTreeSet::from([LogicalPath::parse("profiles/pK3mA9xZq2Lw.yaml").unwrap()]);
        let enumeration = r.enumerate_sync_items(&selected).unwrap();
        let keys: Vec<_> = enumeration
            .items
            .iter()
            .map(|item| (item.logical.as_str(), item.codec))
            .collect();
        assert_eq!(
            keys,
            [
                (
                    "app/application",
                    ItemCodec::Document(SyncDocumentKind::Application)
                ),
                ("icons/tun.png", ItemCodec::Binary),
                ("profiles/pK3mA9xZq2Lw.yaml", ItemCodec::Text),
            ]
        );
    }

    #[test]
    fn dot_entries_links_unselected_and_unportable_names_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).unwrap();
        let r = resolver(&root);
        write(&root.join("config/profiles/.atomicwrite-1/x"), "temp");
        write(&root.join("config/profiles/a b.yaml"), "space");
        write(&root.join("config/profiles/remote.yaml"), "not selected");
        let link = root.join("config/profiles/link.yaml");
        symlink_file(&root.join("config/profiles/remote.yaml"), &link);

        let enumeration = r.enumerate_sync_items(&BTreeSet::new()).unwrap();
        assert!(enumeration.items.is_empty());
        assert!(enumeration.skipped.iter().any(
            |skipped| matches!(skipped, Skipped::UnportableName { physical }
                if physical.ends_with("a b.yaml"))
        ));
        assert!(
            enumeration
                .skipped
                .iter()
                .any(|skipped| matches!(skipped, Skipped::Link { .. }))
        );
        assert!(enumeration.skipped.iter().any(
            |skipped| matches!(skipped, Skipped::NotSelected { physical }
                if physical.ends_with("remote.yaml"))
        ));
    }

    #[cfg(unix)]
    fn symlink_file(target: &Utf8Path, link: &Utf8Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn symlink_file(target: &Utf8Path, link: &Utf8Path) {
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }

    #[test]
    fn names_colliding_under_case_folding_are_both_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).unwrap();
        let r = resolver(&root);
        write(&root.join("config/icons/A.png"), "a");
        write(&root.join("config/icons/a.png"), "b");
        write(&root.join("config/icons/b.png"), "c");

        let enumeration = r.enumerate_sync_items(&BTreeSet::new()).unwrap();
        let keys: Vec<_> = enumeration
            .items
            .iter()
            .map(|item| item.logical.as_str())
            .collect();
        assert_eq!(keys, ["icons/b.png"]);
        assert!(enumeration.skipped.iter().any(
            |skipped| matches!(skipped, Skipped::CaseCollision { physical, .. }
                if physical.len() == 2)
        ));
    }
}
