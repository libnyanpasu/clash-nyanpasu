//! Full snapshots of the config dir and `storage.db` under `<data>/backups/`.
//!
//! A backup is a stateless filesystem operation: every input arrives through
//! the request, so migrations and [`NyanpasuClient`](crate::client::NyanpasuClient)
//! share it.

use camino::Utf8PathBuf;
use nyanpasu_paths::PathResolver;

use crate::core::storage::{Storage, StorageOperationError};
use semver::Version;
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, UtcOffset};

pub const MIGRATION_PREFIX: &str = "migration-";
pub const MANUAL_PREFIX: &str = "manual-";
pub const KEEP_MIGRATION_BACKUPS: usize = 3;
pub const KEEP_MANUAL_BACKUPS: usize = 3;

/// Exit code of `clash-nyanpasu migrate` when the pre-migration backup failed,
/// which tells the parent that no config file was touched.
pub const BACKUP_FAILED_EXIT_CODE: i32 = 2;

const MANIFEST_FILE: &str = "manifest.json";
const CONFIG_DIR: &str = "config";
const DATA_DIR: &str = "data";
const PARTIAL_SUFFIX: &str = ".partial";
/// `from` of a migration backup whose state file records no earlier version.
const UNKNOWN_VERSION: &str = "unknown";
/// Length of the `YYYYMMDDTHHMMSSZ` stamp that follows a backup name's prefix.
const STAMP_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("failed to access {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to snapshot the storage database")]
    Storage(#[from] StorageOperationError),
    #[error("failed to encode the backup manifest")]
    Manifest(#[source] serde_json::Error),
}

pub enum BackupKind<'a> {
    Migration {
        from: Option<&'a Version>,
        target: &'a Version,
    },
    Manual,
}

/// Where `storage.db` comes from.
pub enum StorageSource<'a> {
    /// No process has the file open (the migration subprocess): copy it.
    File(&'a Path),
    /// The app is running and redb holds the file: export a snapshot from one
    /// read transaction.
    Live(&'a Storage),
}

pub struct BackupRequest<'a> {
    pub paths: &'a PathResolver,
    pub storage: StorageSource<'a>,
    pub kind: BackupKind<'a>,
    pub now: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Serialize)]
struct Manifest {
    kind: &'static str,
    created_at: String,
    app_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_version: Option<String>,
    sources: ManifestSources,
    symlinks: Vec<ManifestSymlink>,
}

#[derive(Serialize)]
struct ManifestSources {
    config: PathBuf,
    data: PathBuf,
}

#[derive(Serialize)]
struct ManifestSymlink {
    path: String,
    target: PathBuf,
}

/// Creates a full backup. A failure leaves neither a backup directory nor a
/// `.partial` behind.
pub fn create_backup(req: &BackupRequest<'_>) -> Result<BackupInfo, BackupError> {
    let backups_dir = req.paths.backups_dir().into_std_path_buf();
    fs::create_dir_all(&backups_dir).map_err(io_error(&backups_dir))?;
    remove_partials(&backups_dir)?;

    let (name, partial) = reserve_partial(&backups_dir, &backup_name(req))?;
    let target = backups_dir.join(&name);

    let result = write_backup(req, &partial)
        .and_then(|()| fs::rename(&partial, &target).map_err(io_error(&target)));
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&partial);
        return Err(error);
    }

    Ok(BackupInfo { name, path: target })
}

/// Removes leftover `.partial` directories, then all but the newest `keep`
/// backups whose name starts with `prefix`.
pub fn prune_backups(backups_dir: &Path, prefix: &str, keep: usize) -> Result<(), BackupError> {
    let entries = match fs::read_dir(backups_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(backups_dir)(error)),
    };

    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error(backups_dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !entry.file_type().map_err(io_error(&path))?.is_dir() {
            continue;
        }

        if is_partial(&name) {
            fs::remove_dir_all(&path).map_err(io_error(&path))?;
        } else if let Some(rest) = name.strip_prefix(prefix) {
            // Names carry a fixed-width UTC stamp, so it orders by time; the
            // modification time then orders the backups of one second.
            let stamp = rest.get(..STAMP_LEN).unwrap_or_default().to_owned();
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            backups.push(((stamp, modified, name), path));
        }
    }

    backups.sort_by(|a, b| a.0.cmp(&b.0));
    let stale = backups.len().saturating_sub(keep);
    for (_, path) in backups.into_iter().take(stale) {
        fs::remove_dir_all(&path).map_err(io_error(&path))?;
    }
    Ok(())
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> BackupError + '_ {
    move |source| BackupError::Io {
        path: path.to_owned(),
        source,
    }
}

fn is_partial(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(PARTIAL_SUFFIX)
}

fn remove_partials(backups_dir: &Path) -> Result<(), BackupError> {
    for entry in fs::read_dir(backups_dir).map_err(io_error(backups_dir))? {
        let entry = entry.map_err(io_error(backups_dir))?;
        if is_partial(&entry.file_name().to_string_lossy()) {
            let path = entry.path();
            fs::remove_dir_all(&path).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

fn backup_name(req: &BackupRequest<'_>) -> String {
    let stamp = file_stamp(req.now);
    match &req.kind {
        BackupKind::Migration { from, target } => {
            let from = from.map_or_else(|| UNKNOWN_VERSION.to_owned(), ToString::to_string);
            format!("{MIGRATION_PREFIX}{stamp}-{from}-to-{target}")
        }
        BackupKind::Manual => format!("{MANUAL_PREFIX}{stamp}"),
    }
}

/// Claims the first free name (`base`, `base-1`, `base-2`, ...) by creating its
/// `.partial` directory, so concurrent backups cannot share one.
fn reserve_partial(backups_dir: &Path, base: &str) -> Result<(String, PathBuf), BackupError> {
    for attempt in 0_u32.. {
        let name = match attempt {
            0 => base.to_owned(),
            n => format!("{base}-{n}"),
        };
        if backups_dir.join(&name).exists() {
            continue;
        }

        let partial = backups_dir.join(format!(".{name}{PARTIAL_SUFFIX}"));
        match fs::create_dir(&partial) {
            Ok(()) => return Ok((name, partial)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(&partial)(error)),
        }
    }
    unreachable!("the attempt counter does not run out")
}

fn write_backup(req: &BackupRequest<'_>, partial: &Path) -> Result<(), BackupError> {
    let skipped = skipped_paths(req.paths);
    let mut symlinks = Vec::new();
    copy_dir(
        req.paths.app_config_dir().as_std_path(),
        &partial.join(CONFIG_DIR),
        Path::new(CONFIG_DIR),
        &skipped,
        &mut symlinks,
    )?;

    backup_storage(req, &partial.join(DATA_DIR))?;

    let manifest = manifest(req, symlinks);
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(BackupError::Manifest)?;
    let manifest_path = partial.join(MANIFEST_FILE);
    fs::write(&manifest_path, manifest).map_err(io_error(&manifest_path))
}

/// Excludes generated runtime state and data-dir paths that sit inside the
/// config dir when both are the same tree, including backups and locked stores.
fn skipped_paths(paths: &PathResolver) -> Vec<PathBuf> {
    [
        paths.app_config_dir().join("runtime"),
        paths.backups_dir(),
        paths.storage_path(),
        paths.jobs_path(),
        paths.app_logs_dir(),
        paths.cache_dir(),
        paths.scripts_dir(),
        paths.clash_pid_path(),
    ]
    .into_iter()
    .map(Utf8PathBuf::into_std_path_buf)
    .collect()
}

/// Copies `src` into `dst`. Symlinks are neither followed nor recreated; they
/// are recorded under `rel`, their path relative to the backup root.
/// Sockets, FIFOs, and devices are runtime resources and are not copied.
fn copy_dir(
    src: &Path,
    dst: &Path,
    rel: &Path,
    skipped: &[PathBuf],
    symlinks: &mut Vec<ManifestSymlink>,
) -> Result<(), BackupError> {
    fs::create_dir_all(dst).map_err(io_error(dst))?;
    for entry in fs::read_dir(src).map_err(io_error(src))? {
        let entry = entry.map_err(io_error(src))?;
        let path = entry.path();
        if skipped.contains(&path) {
            continue;
        }

        let dst = dst.join(entry.file_name());
        let rel = rel.join(entry.file_name());
        let file_type = entry.file_type().map_err(io_error(&path))?;
        if file_type.is_symlink() {
            symlinks.push(ManifestSymlink {
                path: rel.to_string_lossy().replace('\\', "/"),
                target: fs::read_link(&path).map_err(io_error(&path))?,
            });
        } else if file_type.is_dir() {
            copy_dir(&path, &dst, &rel, skipped, symlinks)?;
        } else if file_type.is_file() {
            fs::copy(&path, &dst).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

fn backup_storage(req: &BackupRequest<'_>, data_dir: &Path) -> Result<(), BackupError> {
    let storage_path = req.paths.storage_path();
    let dest = data_dir.join(storage_path.file_name().unwrap_or_default());

    match &req.storage {
        StorageSource::File(source) => {
            match fs::metadata(source) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(io_error(source)(error)),
            }
            fs::create_dir_all(data_dir).map_err(io_error(data_dir))?;
            fs::copy(source, &dest).map_err(io_error(source))?;
        }
        StorageSource::Live(storage) => {
            fs::create_dir_all(data_dir).map_err(io_error(data_dir))?;
            storage.export_to(&dest)?;
        }
    }
    Ok(())
}

fn manifest(req: &BackupRequest<'_>, symlinks: Vec<ManifestSymlink>) -> Manifest {
    let (kind, from_version, target_version) = match &req.kind {
        BackupKind::Migration { from, target } => (
            "migration",
            Some(from.map_or_else(|| UNKNOWN_VERSION.to_owned(), ToString::to_string)),
            Some(target.to_string()),
        ),
        BackupKind::Manual => ("manual", None, None),
    };
    Manifest {
        kind,
        created_at: rfc3339(req.now),
        app_version: crate::consts::BUILD_INFO.pkg_version,
        from_version,
        target_version,
        sources: ManifestSources {
            config: req.paths.app_config_dir().as_std_path().to_owned(),
            data: req.paths.app_data_dir().as_std_path().to_owned(),
        },
        symlinks,
    }
}

fn file_stamp(now: OffsetDateTime) -> String {
    let now = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn rfc3339(now: OffsetDateTime) -> String {
    let now = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::WebStorage as _;

    // 2026-09-21T14:13:20Z
    const NOW: i64 = 1_790_000_000;
    const STAMP: &str = "20260921T141320Z";

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(NOW).unwrap()
    }

    fn paths(root: &Path) -> PathResolver {
        crate::client::tests::test_paths(root.join("config"), root.join("data"))
    }

    fn write(path: &(impl AsRef<Path> + ?Sized), content: &str) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn manual<'a>(paths: &'a PathResolver, storage: StorageSource<'a>) -> BackupRequest<'a> {
        BackupRequest {
            paths,
            storage,
            kind: BackupKind::Manual,
            now: now(),
        }
    }

    fn read_manifest(backup: &BackupInfo) -> serde_json::Value {
        serde_json::from_slice(&fs::read(backup.path.join(MANIFEST_FILE)).unwrap()).unwrap()
    }

    fn names(dir: &(impl AsRef<Path> + ?Sized)) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir.as_ref())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    #[cfg(unix)]
    fn backups_skip_runtime_and_special_files() {
        use std::os::unix::{fs::FileTypeExt, net::UnixListener};

        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let paths = paths(dir.path());
        let runtime = paths.app_config_dir().join("runtime");
        write(&runtime.join("control/generated.yaml"), "runtime config");
        write(&paths.profiles_path(), "profiles");
        let runtime_socket = runtime.join("control/core-22.sock");
        let _runtime_listener = UnixListener::bind(&runtime_socket).unwrap();
        let socket = paths.app_config_dir().join("other.sock");
        let _listener = UnixListener::bind(&socket).unwrap();

        let target = Version::new(2, 0, 0);
        let backup = create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(paths.storage_path().as_std_path()),
            kind: BackupKind::Migration {
                from: None,
                target: &target,
            },
            now: now(),
        })
        .unwrap();

        assert_eq!(names(&backup.path.join(CONFIG_DIR)), ["profiles.yaml"]);
        assert_eq!(
            fs::read_to_string(backup.path.join(CONFIG_DIR).join("profiles.yaml")).unwrap(),
            "profiles"
        );
        assert!(
            fs::symlink_metadata(runtime_socket)
                .unwrap()
                .file_type()
                .is_socket()
        );
        assert!(
            fs::symlink_metadata(socket)
                .unwrap()
                .file_type()
                .is_socket()
        );
        assert_eq!(names(&paths.backups_dir()), [backup.name]);
    }

    #[test]
    fn copies_the_config_dir_and_storage_with_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        write(&paths.app_profiles_dir().join("a.yaml"), "a");
        write(&paths.storage_path(), "db");
        let from = Version::new(1, 6, 1);
        let target = Version::new(2, 0, 0);

        let backup = create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(paths.storage_path().as_std_path()),
            kind: BackupKind::Migration {
                from: Some(&from),
                target: &target,
            },
            now: now(),
        })
        .unwrap();

        assert_eq!(backup.name, format!("migration-{STAMP}-1.6.1-to-2.0.0"));
        assert_eq!(backup.path, paths.backups_dir().join(&backup.name));
        let config = backup.path.join(CONFIG_DIR);
        assert_eq!(
            fs::read_to_string(config.join("profiles.yaml")).unwrap(),
            "profiles"
        );
        assert_eq!(
            fs::read_to_string(config.join("profiles").join("a.yaml")).unwrap(),
            "a"
        );
        assert_eq!(
            fs::read_to_string(backup.path.join("data").join("storage.db")).unwrap(),
            "db"
        );

        let manifest = read_manifest(&backup);
        assert_eq!(manifest["kind"], "migration");
        assert_eq!(manifest["created_at"], "2026-09-21T14:13:20Z");
        assert_eq!(manifest["from_version"], "1.6.1");
        assert_eq!(manifest["target_version"], "2.0.0");
        assert_eq!(
            manifest["sources"]["config"],
            paths.app_config_dir().as_str()
        );
        assert_eq!(manifest["sources"]["data"], paths.app_data_dir().as_str());
        assert_eq!(manifest["symlinks"], serde_json::json!([]));
        assert_eq!(names(&paths.backups_dir()), [backup.name]);
    }

    #[test]
    fn a_manual_backup_without_storage_records_no_versions() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_std_path()),
        ))
        .unwrap();

        assert_eq!(backup.name, format!("manual-{STAMP}"));
        assert!(!backup.path.join("data").exists());
        let manifest = read_manifest(&backup);
        assert_eq!(manifest["kind"], "manual");
        assert!(manifest.get("from_version").is_none());
        assert!(manifest.get("target_version").is_none());
    }

    #[test]
    fn a_migration_without_a_recorded_version_is_named_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let target = Version::new(2, 0, 0);

        let backup = create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(paths.storage_path().as_std_path()),
            kind: BackupKind::Migration {
                from: None,
                target: &target,
            },
            now: now(),
        })
        .unwrap();

        assert_eq!(backup.name, format!("migration-{STAMP}-unknown-to-2.0.0"));
        assert_eq!(read_manifest(&backup)["from_version"], "unknown");
    }

    #[test]
    fn a_data_dir_inside_the_config_dir_does_not_copy_the_backups() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let paths = crate::client::tests::test_paths(config.clone(), config.clone());
        write(&paths.profiles_path(), "profiles");
        write(&paths.storage_path(), "db");
        write(&paths.app_logs_dir().join("app.log"), "log");
        write(&paths.jobs_path(), "jobs");

        let storage = paths.storage_path();
        let first =
            create_backup(&manual(&paths, StorageSource::File(storage.as_std_path()))).unwrap();
        let mut request = manual(&paths, StorageSource::File(storage.as_std_path()));
        request.now = now() + time::Duration::seconds(1);
        let second = create_backup(&request).unwrap();

        assert_eq!(names(&second.path.join(CONFIG_DIR)), ["profiles.yaml"]);
        assert_eq!(names(&second.path.join("data")), ["storage.db"]);
        assert!(first.path.exists());
    }

    #[test]
    fn symlinks_are_recorded_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let external = dir.path().join("external.yaml");
        write(&external, "external");
        fs::create_dir_all(paths.app_profiles_dir()).unwrap();
        let link = paths.app_profiles_dir().join("local.yaml");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&external, &link).is_err() {
            // Creating symlinks needs developer mode or elevation.
            return;
        }

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_std_path()),
        ))
        .unwrap();

        assert!(
            !backup
                .path
                .join(CONFIG_DIR)
                .join("profiles")
                .join("local.yaml")
                .exists()
        );
        let manifest = read_manifest(&backup);
        assert_eq!(
            manifest["symlinks"],
            serde_json::json!([{
                "path": "config/profiles/local.yaml",
                "target": external.to_str().unwrap(),
            }])
        );
    }

    #[test]
    fn a_failure_midway_leaves_neither_a_backup_nor_a_partial() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        // A directory cannot be copied as the storage file.
        let unreadable = dir.path().join("unreadable");
        fs::create_dir_all(&unreadable).unwrap();

        let error = create_backup(&manual(&paths, StorageSource::File(&unreadable))).unwrap_err();

        assert!(matches!(error, BackupError::Io { .. }), "{error:?}");
        assert!(names(&paths.backups_dir()).is_empty());
    }

    #[test]
    fn a_second_backup_in_the_same_second_gets_a_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let source = paths.storage_path();

        let first =
            create_backup(&manual(&paths, StorageSource::File(source.as_std_path()))).unwrap();
        let second =
            create_backup(&manual(&paths, StorageSource::File(source.as_std_path()))).unwrap();

        assert_eq!(first.name, format!("manual-{STAMP}"));
        assert_eq!(second.name, format!("manual-{STAMP}-1"));
        assert!(first.path.join(MANIFEST_FILE).exists());
    }

    #[test]
    fn a_live_storage_is_exported_as_a_consistent_copy() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let storage = Storage::try_new(paths.storage_path().as_std_path()).unwrap();
        storage.set_item("a", &1).unwrap();
        storage.set_item("b", &"two").unwrap();

        let backup = create_backup(&manual(&paths, StorageSource::Live(&storage))).unwrap();

        let exported = Storage::try_new(&backup.path.join("data").join("storage.db")).unwrap();
        assert_eq!(exported.get_all().unwrap(), storage.get_all().unwrap());
        assert_eq!(exported.get_all().unwrap().len(), 2);
        storage.set_item("c", &3).unwrap();
        assert_eq!(storage.get_all().unwrap().len(), 3);
        assert_eq!(exported.get_all().unwrap().len(), 2);
    }

    #[test]
    fn pruning_keeps_the_newest_of_one_prefix_and_clears_partials() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        for name in [
            "migration-20260101T000000Z-1.0.0-to-2.0.0",
            "migration-20260301T000000Z-1.0.0-to-2.0.0",
            "migration-20260201T000000Z-1.0.0-to-2.0.0",
            "migration-20260401T000000Z-1.0.0-to-2.0.0",
            "manual-20250101T000000Z",
            ".migration-20260501T000000Z-1.0.0-to-2.0.0.partial",
            "unrelated",
        ] {
            fs::create_dir_all(backups.join(name)).unwrap();
        }
        fs::write(backups.join("migration-note.txt"), "keep").unwrap();

        prune_backups(&backups, MIGRATION_PREFIX, 2).unwrap();

        assert_eq!(
            names(&backups),
            [
                "manual-20250101T000000Z",
                "migration-20260301T000000Z-1.0.0-to-2.0.0",
                "migration-20260401T000000Z-1.0.0-to-2.0.0",
                "migration-note.txt",
                "unrelated",
            ]
        );
    }

    #[test]
    fn pruning_a_missing_dir_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();

        prune_backups(&dir.path().join("backups"), MANUAL_PREFIX, 3).unwrap();
    }

    #[test]
    fn creating_a_backup_clears_a_leftover_partial() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let leftover = paths.backups_dir().join(".manual-old.partial");
        fs::create_dir_all(&leftover).unwrap();

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_std_path()),
        ))
        .unwrap();

        assert_eq!(names(&paths.backups_dir()), [backup.name]);
    }
}
