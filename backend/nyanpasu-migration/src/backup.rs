//! Full snapshots of the config dir and `storage.db` under `<data>/backups/`.
//!
//! A backup is a stateless filesystem operation: every input arrives through
//! the request, so migrations and the application client share it.

use camino::Utf8PathBuf;
use nyanpasu_contracts::{BoxedCause, bundle_metadata::BundleMetadata};
use nyanpasu_paths::PathResolver;

use jiff::Timestamp;
use semver::Version;
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const MIGRATION_PREFIX: &str = "migration-";
pub const MANUAL_PREFIX: &str = "manual-";
pub const KEEP_MIGRATION_BACKUPS: usize = 3;
pub const KEEP_MANUAL_BACKUPS: usize = 3;

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
    Storage(#[source] BoxedCause),
    #[error("failed to encode the backup manifest")]
    Manifest(#[source] serde_json::Error),
    #[error(
        "config directory cannot be located at or inside the remote-access secrets directory: {}",
        path.display()
    )]
    InvalidConfigSource { path: PathBuf },
}

pub enum BackupKind<'a> {
    Migration {
        from: Option<&'a Version>,
        target: &'a Version,
    },
    Manual,
}

/// Exports a consistent snapshot of an open store without copying its locked file.
pub trait StorageSnapshot: Send + Sync {
    /// Writes the snapshot to `destination`.
    ///
    /// # Errors
    /// Returns the original storage error if exporting the snapshot fails.
    fn export_to(&self, destination: &Path) -> Result<(), BoxedCause>;
}

/// Where `storage.db` comes from.
pub enum StorageSource<'a> {
    /// No process has the file open (the migration subprocess): copy it.
    File(&'a Path),
    /// The app is running and redb holds the file: export a snapshot from one
    /// read transaction.
    Live(&'a dyn StorageSnapshot),
}

pub struct BackupRequest<'a> {
    pub paths: &'a PathResolver,
    pub bundle_metadata: BundleMetadata,
    pub storage: StorageSource<'a>,
    pub kind: BackupKind<'a>,
    pub now: Timestamp,
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
    validate_config_source(req.paths)?;
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

struct CopyExclusions {
    skipped: Vec<PathBuf>,
    secret_dir: PathBuf,
}

fn canonicalize_if_exists(path: &Path) -> Result<Option<PathBuf>, BackupError> {
    match fs::canonicalize(path) {
        Ok(canon) => Ok(Some(canon)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path)(error)),
    }
}

fn validate_config_source(paths: &PathResolver) -> Result<(), BackupError> {
    let config_dir = paths.app_config_dir().as_std_path();
    let Some(canon_secret) = canonicalize_if_exists(paths.remote_access_dir().as_std_path())?
    else {
        return Ok(());
    };
    let inside_secret = if let Some(canon_config) = canonicalize_if_exists(config_dir)? {
        is_same_or_descendant(&canon_config, &canon_secret)
    } else {
        let mut matched = false;
        for anc in config_dir.ancestors() {
            if let Some(canon_anc) = canonicalize_if_exists(anc)? {
                matched = is_same_or_descendant(&canon_anc, &canon_secret);
                break;
            }
        }
        matched
    };
    if inside_secret {
        return Err(BackupError::InvalidConfigSource {
            path: config_dir.to_owned(),
        });
    }
    Ok(())
}

/// Computes canonical exclusion paths after `backups_dir` (and thus `app_data_dir`) exists.
/// Reserves `remote-access` (and other known `app_data_dir` children) even when absent on disk so
/// a concurrent first secret write cannot race the directory walk.
fn compute_copy_exclusions(paths: &PathResolver) -> Result<CopyExclusions, BackupError> {
    let raw_data_dir = paths.app_data_dir().as_std_path();
    let canon_data_dir = fs::canonicalize(raw_data_dir).map_err(io_error(raw_data_dir))?;
    let raw_secret_buf = paths.remote_access_dir();
    let raw_secret_dir = raw_secret_buf.as_std_path();
    let secret_rel = raw_secret_dir
        .strip_prefix(raw_data_dir)
        .unwrap_or_else(|_| Path::new("remote-access"));
    let secret_dir =
        canonicalize_if_exists(raw_secret_dir)?.unwrap_or_else(|| canon_data_dir.join(secret_rel));

    let mut skipped = Vec::new();
    for path in skipped_paths(paths) {
        if let Ok(rel) = path.strip_prefix(raw_data_dir) {
            let derived = canon_data_dir.join(rel);
            if !skipped.contains(&derived) {
                skipped.push(derived);
            }
        }
        if let Some(canon) = canonicalize_if_exists(&path)?
            && !skipped.contains(&canon)
        {
            skipped.push(canon);
        }
    }
    Ok(CopyExclusions {
        skipped,
        secret_dir,
    })
}

fn write_backup(req: &BackupRequest<'_>, partial: &Path) -> Result<(), BackupError> {
    let config_dir = req.paths.app_config_dir().as_std_path();
    let canon_config = fs::canonicalize(config_dir).map_err(io_error(config_dir))?;
    let exclusions = compute_copy_exclusions(req.paths)?;
    let mut symlinks = Vec::new();
    copy_dir(
        &canon_config,
        &partial.join(CONFIG_DIR),
        Path::new(CONFIG_DIR),
        &exclusions.skipped,
        &exclusions.secret_dir,
        &mut symlinks,
    )?;

    backup_storage(req, &partial.join(DATA_DIR))?;

    let manifest = manifest(req, symlinks);
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(BackupError::Manifest)?;
    let manifest_path = partial.join(MANIFEST_FILE);
    fs::write(&manifest_path, manifest).map_err(io_error(&manifest_path))
}

fn is_same_or_descendant(candidate: &Path, root: &Path) -> bool {
    candidate == root || candidate.starts_with(root)
}

/// Data-dir paths that sit inside the config dir when both are the same tree.
/// None of them is config: the backups themselves, the stores redb keeps
/// locked, and runtime data the backup does not cover.
fn skipped_paths(paths: &PathResolver) -> Vec<PathBuf> {
    [
        paths.backups_dir(),
        paths.storage_path(),
        paths.jobs_path(),
        paths.app_data_dir().join("sync"),
        paths.app_logs_dir(),
        paths.cache_dir(),
        paths.scripts_dir(),
        paths.clash_pid_path(),
        paths.remote_access_dir(),
    ]
    .into_iter()
    .map(Utf8PathBuf::into_std_path_buf)
    .collect()
}

/// Copies `src` into `dst`. Symlinks are neither followed nor recreated; they
/// are recorded under `rel`, their path relative to the backup root. Sockets,
/// FIFOs and devices, such as the core's control socket under `runtime/`,
/// hold no config and cannot be copied, so they are left out.
fn copy_dir(
    src: &Path,
    dst: &Path,
    rel: &Path,
    skipped: &[PathBuf],
    secret_dir: &Path,
    symlinks: &mut Vec<ManifestSymlink>,
) -> Result<(), BackupError> {
    fs::create_dir_all(dst).map_err(io_error(dst))?;
    for entry in fs::read_dir(src).map_err(io_error(src))? {
        let entry = entry.map_err(io_error(src))?;
        let path = entry.path();
        if skipped.contains(&path) || is_same_or_descendant(&path, secret_dir) {
            continue;
        }
        let file_type = entry.file_type().map_err(io_error(&path))?;
        let is_symlink = file_type.is_symlink();

        let dst = dst.join(entry.file_name());
        let rel = rel.join(entry.file_name());
        if is_symlink {
            symlinks.push(ManifestSymlink {
                path: rel.to_string_lossy().replace('\\', "/"),
                target: fs::read_link(&path).map_err(io_error(&path))?,
            });
        } else if file_type.is_dir() {
            copy_dir(&path, &dst, &rel, skipped, secret_dir, symlinks)?;
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
            storage.export_to(&dest).map_err(BackupError::Storage)?;
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
        app_version: req.bundle_metadata.app_version,
        from_version,
        target_version,
        sources: ManifestSources {
            config: req.paths.app_config_dir().as_std_path().to_owned(),
            data: req.paths.app_data_dir().as_std_path().to_owned(),
        },
        symlinks,
    }
}

fn file_stamp(now: Timestamp) -> String {
    now.strftime("%Y%m%dT%H%M%SZ").to_string()
}

fn rfc3339(now: Timestamp) -> String {
    now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-09-21T14:13:20Z
    const NOW: i64 = 1_790_000_000;
    const STAMP: &str = "20260921T141320Z";

    fn now() -> Timestamp {
        Timestamp::from_second(NOW).unwrap()
    }

    fn paths(root: &Path) -> PathResolver {
        PathResolver::with_base_dirs(
            root.join("config").try_into().unwrap(),
            root.join("data").try_into().unwrap(),
        )
    }

    fn write(path: &(impl AsRef<Path> + ?Sized), content: &str) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn manual<'a>(paths: &'a PathResolver, storage: StorageSource<'a>) -> BackupRequest<'a> {
        BackupRequest {
            bundle_metadata: nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
            paths,
            storage,
            kind: BackupKind::Manual,
            now: now(),
        }
    }

    fn assert_backup_excludes_remote_access_secrets(config_dir: &Path, data_dir: &Path) {
        let resolver = PathResolver::with_base_dirs(
            config_dir.to_path_buf().try_into().unwrap(),
            data_dir.to_path_buf().try_into().unwrap(),
        );

        let sentinel_plain = "PLAIN_REMOTE_AUTH_SECRET_SENTINEL";
        let sentinel_encrypted = "ENCRYPTED_REMOTE_AUTH_SECRET_SENTINEL";
        write(
            resolver.remote_access_secrets_path().as_std_path(),
            sentinel_plain,
        );
        write(
            &resolver
                .remote_access_dir()
                .join("auth-secrets.json.tmp")
                .into_std_path_buf(),
            sentinel_encrypted,
        );
        write(&config_dir.join("clash-nyanpasu.yaml"), "port: 7890\n");

        let storage_file = config_dir.join("_unused_storage");
        let backup = create_backup(&manual(&resolver, StorageSource::File(&storage_file))).unwrap();
        let backup_root = backup.path.as_path();

        assert!(backup_root.join("config/clash-nyanpasu.yaml").is_file());
        assert!(!backup_root.join("data/remote-access").exists());
        assert!(!backup_root.join("config/remote-access").exists());

        let scanned = walk_assert_no_sentinel(backup_root, &[sentinel_plain, sentinel_encrypted]);
        assert!(
            scanned >= 1,
            "expected at least clash-nyanpasu.yaml in backup"
        );
    }

    fn walk_assert_no_sentinel(dir: &Path, sentinels: &[&str]) -> usize {
        if !dir.exists() {
            return 0;
        }
        let mut scanned = 0usize;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_ne!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("remote-access")
                );
                scanned += walk_assert_no_sentinel(&path, sentinels);
            } else if path.is_file() {
                scanned += 1;
                let bytes = fs::read(&path).unwrap();
                let text = String::from_utf8_lossy(&bytes);
                for s in sentinels {
                    assert!(
                        !text.contains(s),
                        "backup file {} leaked remote-access secret",
                        path.display()
                    );
                }
            }
        }
        scanned
    }

    #[test]
    fn remote_access_secrets_excluded_across_separate_same_nested_and_alias_dirs() {
        let root = tempfile::tempdir().unwrap();

        let sep = root.path().join("separate");
        assert_backup_excludes_remote_access_secrets(&sep.join("config"), &sep.join("data"));

        let shared = root.path().join("shared");
        assert_backup_excludes_remote_access_secrets(&shared, &shared);

        let data_in_config = root.path().join("data_in_config");
        assert_backup_excludes_remote_access_secrets(&data_in_config, &data_in_config.join("data"));

        let config_in_data = root.path().join("config_in_data");
        assert_backup_excludes_remote_access_secrets(
            &config_in_data.join("config"),
            &config_in_data,
        );

        // `data_dir` containing existing or non-existing `some/../data` alias inside `config_dir`
        // must still normalize and exclude `remote-access`.
        let alias_existing = root.path().join("alias_existing");
        fs::create_dir_all(alias_existing.join("some")).unwrap();
        assert_backup_excludes_remote_access_secrets(
            &alias_existing,
            &alias_existing.join("some/../data"),
        );

        let alias_missing_hop = root.path().join("alias_missing");
        fs::create_dir_all(&alias_missing_hop).unwrap();
        assert_backup_excludes_remote_access_secrets(
            &alias_missing_hop.join("sub/../config"),
            &alias_missing_hop.join("config/missing_hop/../data"),
        );
    }

    #[test]
    fn backup_rejects_config_dir_at_or_inside_remote_access_dir() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        let secret_dir = data_dir.join("remote-access");
        write(&secret_dir.join("auth-secrets.json"), "SECRET_SENTINEL");
        fs::create_dir_all(secret_dir.join("nested")).unwrap();

        for bad_config in [
            secret_dir.clone(),
            secret_dir.join("nested"),
            secret_dir.join("nested/missing"),
        ] {
            let resolver = PathResolver::with_base_dirs(
                bad_config.clone().try_into().unwrap(),
                data_dir.clone().try_into().unwrap(),
            );
            let storage = data_dir.join("_unused_storage");
            let err = create_backup(&manual(&resolver, StorageSource::File(&storage))).unwrap_err();
            assert!(
                matches!(err, BackupError::InvalidConfigSource { .. }),
                "expected InvalidConfigSource for {}, got {err:?}",
                bad_config.display()
            );
            assert!(
                !resolver.backups_dir().as_std_path().exists(),
                "rejected backup must not leave partial/complete backup directories"
            );
        }
    }

    #[test]
    fn backup_exclusion_captured_before_first_secret_creation_still_excludes_concurrent_secret() {
        let root = tempfile::tempdir().unwrap();
        let config_dir = root.path().join("shared");
        let data_alias = config_dir.join("some/../.");
        fs::create_dir_all(config_dir.join("some")).unwrap();

        let resolver = PathResolver::with_base_dirs(
            config_dir.clone().try_into().unwrap(),
            data_alias.try_into().unwrap(),
        );
        write(&resolver.clash_config_path(), "port: 7890\n");
        fs::create_dir_all(resolver.backups_dir()).unwrap();
        assert!(
            !resolver.remote_access_dir().as_std_path().exists(),
            "remote-access dir must be absent when exclusions are computed"
        );

        let exclusions = compute_copy_exclusions(&resolver).unwrap();

        // Simulate concurrent first secret creation after exclusions are captured, before walk.
        let sentinel = "CONCURRENT_FIRST_SECRET_SENTINEL";
        write(&resolver.remote_access_secrets_path(), sentinel);

        let out_config = root.path().join("captured-backup/config");
        let canon_config = fs::canonicalize(&config_dir).unwrap();
        let mut symlinks = Vec::new();
        copy_dir(
            &canon_config,
            &out_config,
            Path::new(CONFIG_DIR),
            &exclusions.skipped,
            &exclusions.secret_dir,
            &mut symlinks,
        )
        .unwrap();

        let scanned = walk_assert_no_sentinel(&out_config, &[sentinel]);
        assert!(
            scanned >= 1,
            "expected real config file clash-nyanpasu.yaml to be copied"
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_if_exists_propagates_permission_denied_instead_of_treating_as_missing() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        let child = locked.join("remote-access");
        fs::create_dir_all(&child).unwrap();
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&locked, perms).unwrap();

        struct RestoreMode<'a>(&'a Path);
        impl Drop for RestoreMode<'_> {
            fn drop(&mut self) {
                if let Ok(m) = fs::metadata(self.0) {
                    let mut p = m.permissions();
                    p.set_mode(0o700);
                    let _ = fs::set_permissions(self.0, p);
                }
            }
        }
        let _restore = RestoreMode(&locked);

        if let Err(err) = fs::canonicalize(&child)
            && err.kind() == io::ErrorKind::PermissionDenied
        {
            let err = canonicalize_if_exists(&child).unwrap_err();
            assert!(matches!(err, BackupError::Io { .. }));
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
    fn copies_the_config_dir_and_storage_with_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        write(&paths.app_profiles_dir().join("a.yaml"), "a");
        write(&paths.storage_path(), "db");
        let from = Version::new(1, 6, 1);
        let target = Version::new(2, 0, 0);

        let backup = create_backup(&BackupRequest {
            bundle_metadata: nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
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
        assert_eq!(manifest["app_version"], "2.0.0-test+host");
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
            bundle_metadata: nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
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
        let paths = PathResolver::with_base_dirs(
            config.clone().try_into().unwrap(),
            config.try_into().unwrap(),
        );
        write(&paths.profiles_path(), "profiles");
        write(&paths.storage_path(), "db");
        write(&paths.app_logs_dir().join("app.log"), "log");
        write(&paths.jobs_path(), "jobs");
        write(&paths.app_data_dir().join("sync/state.redb"), "sync state");
        write(
            &paths.app_data_dir().join("sync/credentials.json"),
            "secret",
        );

        let storage = paths.storage_path();
        let first =
            create_backup(&manual(&paths, StorageSource::File(storage.as_std_path()))).unwrap();
        let mut request = manual(&paths, StorageSource::File(storage.as_std_path()));
        request.now = now() + jiff::SignedDuration::from_secs(1);
        let second = create_backup(&request).unwrap();

        assert_eq!(names(&second.path.join(CONFIG_DIR)), ["profiles.yaml"]);
        assert_eq!(names(&second.path.join("data")), ["storage.db"]);
        assert!(first.path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_in_the_config_dir_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let runtime = paths.app_config_dir().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        write(&runtime.join("state.yaml"), "kept");
        // A live core leaves its control socket here.
        let _socket = std::os::unix::net::UnixListener::bind(runtime.join("core.sock")).unwrap();

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_std_path()),
        ))
        .unwrap();

        let copied = backup.path.join(CONFIG_DIR).join("runtime");
        assert_eq!(names(&copied), ["state.yaml"]);
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
