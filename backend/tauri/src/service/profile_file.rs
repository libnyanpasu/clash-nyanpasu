//! Profile filesystem, materialization transactions, and subscription fetches
//! over injected paths/network state. Tauri-free and legacy-Config-free.

use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior, replace_atomic};
use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, Profiles, RemoteProfileOptions, SubscriptionInfo,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    state::profiles::ports::{
        CleanupOutcome, FetchedSubscription, MaterializationReconcileReport,
        MaterializationResource, PreparedCleanup, PreparedMaterialization, ProfileDegradation,
        ProfileDegradationCode, ProfileDegradationPhase, ProfileFsPort, ProfileMaterializationPort,
        SubscriptionFetcher,
    },
    utils::path::PathResolver,
};

const MATERIALIZATION_ROOT: &str = ".profile-materialization-v1";
const ABSENT_HASH: &str = "b7c03610089b9f660990ee7db3290cc8d8564161b079319c9c7ba1f19dc2e190";

/// Where the fetcher looks up the app's own mixed port when `self_proxy` is
/// requested. Wired at the composition root (T07); tests inject a constant.
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}

pub struct ProfileFileService {
    paths: PathResolver,
    self_proxy_port: Arc<dyn SelfProxyPortSource>,
    http_timeout: Duration,
}

impl ProfileFileService {
    pub fn new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>) -> Self {
        Self {
            paths,
            self_proxy_port,
            http_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    fn resolve(&self, path: &ManagedProfilePath) -> anyhow::Result<PathBuf> {
        if path
            .as_path()
            .components()
            .any(|component| is_materialization_root_name(component.as_os_str()))
        {
            bail!("managed profile path uses reserved private storage");
        }

        let full = self.paths.app_profiles_dir().join(path.as_path());
        self.validate_existing_parent_chain(&full)?;
        Ok(full)
    }

    fn validate_existing_parent_chain(&self, full: &Path) -> anyhow::Result<()> {
        let root = self.paths.app_profiles_dir();
        let relative = full.strip_prefix(&root).with_context(|| {
            format!(
                "profile path containment violation: {} escapes {}",
                full.display(),
                root.display()
            )
        })?;

        match std::fs::symlink_metadata(&root) {
            Ok(metadata) => ensure_real_directory(&root, &metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("inspect profiles directory"),
        }

        #[cfg(windows)]
        let canonical_private_root = {
            let private_root = root.join(MATERIALIZATION_ROOT);
            match std::fs::symlink_metadata(&private_root) {
                Ok(metadata) => {
                    ensure_real_directory(&private_root, &metadata)?;
                    Some(canonicalize_for_compare(&private_root).with_context(|| {
                        format!(
                            "canonicalize private materialization root {}",
                            private_root.display()
                        )
                    })?)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("inspect private root {}", private_root.display())
                    });
                }
            }
        };

        let Some(parent) = relative.parent() else {
            return Ok(());
        };
        let mut current = root;
        for component in parent.components() {
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) => ensure_real_directory(&current, &metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("inspect profile parent {}", current.display()));
                }
            }
            #[cfg(windows)]
            if let Some(private_root) = &canonical_private_root {
                let canonical_current = canonicalize_for_compare(&current).with_context(|| {
                    format!("canonicalize profile parent {}", current.display())
                })?;
                if canonical_current.starts_with(private_root) {
                    bail!("managed profile path uses reserved private storage");
                }
            }
        }
        #[cfg(windows)]
        if let Some(private_root) = &canonical_private_root {
            match std::fs::symlink_metadata(full) {
                Ok(_) => {
                    let canonical_full = canonicalize_for_compare(full).with_context(|| {
                        format!("canonicalize profile target {}", full.display())
                    })?;
                    if canonical_full.starts_with(private_root) {
                        bail!("managed profile path uses reserved private storage");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("inspect profile target {}", full.display()));
                }
            }
        }
        Ok(())
    }

    fn ensure_managed_parent(&self, full: &Path) -> anyhow::Result<()> {
        let root = self.ensure_profiles_root()?;
        let parent = full
            .parent()
            .context("managed profile target has no parent")?;
        self.ensure_directory_chain(&root, parent, false)
    }

    fn ensure_profiles_root(&self) -> anyhow::Result<PathBuf> {
        let root = self.paths.app_profiles_dir();
        let config_dir = root.parent().context("profiles directory has no parent")?;
        ensure_real_directory_tree(config_dir)?;

        match std::fs::symlink_metadata(&root) {
            Ok(metadata) => ensure_real_directory(&root, &metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&root)
                    .with_context(|| format!("create profiles directory {}", root.display()))?;
                let metadata = std::fs::symlink_metadata(&root)
                    .with_context(|| format!("inspect profiles directory {}", root.display()))?;
                ensure_real_directory(&root, &metadata)?;
                sync_directory(config_dir)?;
                sync_directory(&root)?;
            }
            Err(error) => return Err(error).context("inspect profiles directory"),
        }
        Ok(root)
    }

    fn ensure_directory_chain(
        &self,
        root: &Path,
        directory: &Path,
        private: bool,
    ) -> anyhow::Result<()> {
        let relative = directory.strip_prefix(root).with_context(|| {
            format!(
                "profile path containment violation: {} escapes {}",
                directory.display(),
                root.display()
            )
        })?;
        let mut current = root.to_path_buf();
        for component in relative.components() {
            current.push(component.as_os_str());
            let created = match std::fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    ensure_real_directory(&current, &metadata)?;
                    false
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&current).with_context(|| {
                        format!("create profile directory {}", current.display())
                    })?;
                    let metadata = std::fs::symlink_metadata(&current).with_context(|| {
                        format!("inspect profile directory {}", current.display())
                    })?;
                    ensure_real_directory(&current, &metadata)?;
                    true
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("inspect profile directory {}", current.display())
                    });
                }
            };
            if created {
                sync_directory(current.parent().expect("profile directory has parent"))?;
            }
            if private {
                set_private_directory_permissions(&current)?;
                sync_directory(&current)?;
            }
        }
        Ok(())
    }

    fn ensure_materialization_layout(&self) -> anyhow::Result<PathBuf> {
        let profiles_root = self.ensure_profiles_root()?;
        let root = profiles_root.join(MATERIALIZATION_ROOT);
        for relative in PRIVATE_DIRECTORIES {
            self.ensure_directory_chain(&profiles_root, &root.join(relative), true)?;
        }
        Ok(root)
    }
}

fn canonicalize_for_compare(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

fn symlink_points_to(link: &Path, target: &Path) -> anyhow::Result<bool> {
    let existing = std::fs::read_link(link)?;
    let existing = if existing.is_absolute() {
        existing
    } else {
        link.parent()
            .map(|parent| parent.join(&existing))
            .unwrap_or(existing)
    };
    let Ok(existing) = canonicalize_for_compare(&existing) else {
        return Ok(false);
    };
    let Ok(target) = canonicalize_for_compare(target) else {
        return Ok(false);
    };
    Ok(existing == target)
}

const PRIVATE_DIRECTORIES: &[&str] = &[
    "",
    "staging/files",
    "staging/links",
    "staging/ready-links",
    "backup/files",
    "backup/links",
    "journal/state-first/prepared",
    "journal/state-first/promoting",
    "journal/state-first/compensating",
    "journal/file-first/prepared",
    "journal/file-first/promoting",
    "journal/file-first/promoted",
    "journal/file-first/compensating",
    "cleanup/pending",
    "cleanup/ready",
    "cleanup/tombstones",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializationJournal {
    managed_path: ManagedProfilePath,
    operation_id: String,
    revision: u64,
    hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalLocation {
    StatePrepared,
    StatePromoting,
    StateCompensating,
    FilePrepared,
    FilePromoting,
    FilePromoted,
    FileCompensating,
}

impl JournalLocation {
    const ALL: [Self; 7] = [
        Self::StatePrepared,
        Self::StatePromoting,
        Self::StateCompensating,
        Self::FilePrepared,
        Self::FilePromoting,
        Self::FilePromoted,
        Self::FileCompensating,
    ];

    fn directory(self) -> &'static str {
        match self {
            Self::StatePrepared => "journal/state-first/prepared",
            Self::StatePromoting => "journal/state-first/promoting",
            Self::StateCompensating => "journal/state-first/compensating",
            Self::FilePrepared => "journal/file-first/prepared",
            Self::FilePromoting => "journal/file-first/promoting",
            Self::FilePromoted => "journal/file-first/promoted",
            Self::FileCompensating => "journal/file-first/compensating",
        }
    }

    fn promoting(self) -> Option<Self> {
        match self {
            Self::StatePrepared => Some(Self::StatePromoting),
            Self::FilePrepared => Some(Self::FilePromoting),
            _ => None,
        }
    }

    fn compensating(self) -> Self {
        match self {
            Self::StatePrepared | Self::StatePromoting | Self::StateCompensating => {
                Self::StateCompensating
            }
            Self::FilePrepared
            | Self::FilePromoting
            | Self::FilePromoted
            | Self::FileCompensating => Self::FileCompensating,
        }
    }

    fn family(self) -> u8 {
        match self {
            Self::StatePrepared | Self::StatePromoting | Self::StateCompensating => 0,
            Self::FilePrepared
            | Self::FilePromoting
            | Self::FilePromoted
            | Self::FileCompensating => 1,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::StatePrepared | Self::FilePrepared => 0,
            Self::StatePromoting | Self::FilePromoting => 1,
            Self::StateCompensating | Self::FilePromoted => 2,
            Self::FileCompensating => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupPhase {
    Pending,
    Ready,
}

impl CleanupPhase {
    fn directory(self) -> &'static str {
        match self {
            Self::Pending => "cleanup/pending",
            Self::Ready => "cleanup/ready",
        }
    }
}

#[derive(Debug)]
enum StoredResource {
    File { path: PathBuf },
    Symlink { target: ExternalProfilePath },
}

fn ensure_real_directory(path: &Path, metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    if is_symlink_or_reparse(metadata) || !metadata.is_dir() {
        bail!(
            "profile directory is a symlink, reparse point, or non-directory: {}",
            path.display()
        );
    }
    Ok(())
}

fn ensure_real_directory_tree(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => ensure_real_directory(path, &metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .context("directory creation path has no parent")?;
            ensure_real_directory_tree(parent)?;
            let created = match std::fs::create_dir(path) {
                Ok(()) => true,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("create directory {}", path.display()));
                }
            };
            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("inspect directory {}", path.display()))?;
            ensure_real_directory(path, &metadata)?;
            if created {
                sync_directory(parent)?;
                sync_directory(path)?;
            }
            Ok(())
        }
        Err(error) => Err(error).with_context(|| format!("inspect directory {}", path.display())),
    }
}

fn set_private_directory_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("set private directory permissions for {}", path.display()))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set private file permissions for {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn is_materialization_root_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    #[cfg(windows)]
    {
        name.trim_end_matches(['.', ' '])
            .eq_ignore_ascii_case(MATERIALIZATION_ROOT)
    }
    #[cfg(not(windows))]
    {
        name == MATERIALIZATION_ROOT
    }
}

fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "file symlinks are unsupported on this platform",
        ))
    }
}

fn hash_tagged(tag: &[u8], content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(tag);
    digest.update([0]);
    digest.update(content);
    hex::encode(digest.finalize())
}

fn valid_operation_id(operation_id: &str) -> bool {
    operation_id.len() == 16
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> anyhow::Result<()> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("sync directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

impl ProfileFileService {
    fn materialization_root(&self) -> PathBuf {
        self.paths.app_profiles_dir().join(MATERIALIZATION_ROOT)
    }

    fn stage_file_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("staging/files").join(operation_id)
    }

    fn stage_link_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("staging/links").join(operation_id)
    }

    fn restore_file_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("staging/files")
            .join(format!("{operation_id}.restore"))
    }

    fn ready_link_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("staging/ready-links").join(operation_id)
    }

    fn backup_file_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("backup/files").join(operation_id)
    }

    fn backup_link_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("backup/links").join(operation_id)
    }

    fn journal_path(root: &Path, location: JournalLocation, operation_id: &str) -> PathBuf {
        root.join(location.directory())
            .join(format!("{operation_id}.yaml"))
    }

    fn cleanup_path(root: &Path, phase: CleanupPhase, operation_id: &str) -> PathBuf {
        root.join(phase.directory())
            .join(format!("{operation_id}.yaml"))
    }

    fn cleanup_tombstone_path(root: &Path, operation_id: &str) -> PathBuf {
        root.join("cleanup/tombstones").join(operation_id)
    }

    fn write_private_file_new(path: &Path, content: &[u8]) -> anyhow::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .with_context(|| format!("create private file {}", path.display()))?;
        file.write_all(content)
            .with_context(|| format!("write private file {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync private file {}", path.display()))?;
        set_private_file_permissions(path)?;
        if let Some(parent) = path.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    }

    fn write_journal_new(path: &Path, journal: &MaterializationJournal) -> anyhow::Result<()> {
        let content =
            serde_yaml::to_string(journal).context("serialize materialization journal")?;
        AtomicFile::new(path, OverwriteBehavior::DisallowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("write materialization journal {}", path.display()))?;
        set_private_file_permissions(path)?;
        sync_directory(path.parent().expect("journal has parent"))
    }

    fn read_journal(path: &Path, operation_id: &str) -> anyhow::Result<MaterializationJournal> {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("inspect materialization journal {}", path.display()))?;
        if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
            bail!(
                "materialization journal is not a regular file: {}",
                path.display()
            );
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("read materialization journal {}", path.display()))?;
        let journal: MaterializationJournal = serde_yaml::from_str(&content)
            .with_context(|| format!("parse materialization journal {}", path.display()))?;
        if journal.operation_id != operation_id || !valid_operation_id(&journal.operation_id) {
            bail!("materialization journal operation id mismatch");
        }
        if journal
            .managed_path
            .as_path()
            .components()
            .any(|component| is_materialization_root_name(component.as_os_str()))
        {
            bail!("materialization journal targets reserved private storage");
        }
        if journal.hash.len() != 64 || !journal.hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("materialization journal hash is invalid");
        }
        Ok(journal)
    }

    fn remove_nofollow(path: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !is_symlink_or_reparse(&metadata) => {
                bail!(
                    "refusing to remove directory as a profile resource: {}",
                    path.display()
                )
            }
            Ok(_) => {
                std::fs::remove_file(path)
                    .with_context(|| format!("remove profile resource {}", path.display()))?;
                sync_directory(path.parent().expect("profile resource has parent"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("inspect profile resource {}", path.display()))
            }
        }
    }

    fn remove_private_regular(path: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if is_symlink_or_reparse(&metadata) || !metadata.is_file() => {
                bail!(
                    "private materialization artifact is not a regular file: {}",
                    path.display()
                )
            }
            Ok(_) => {
                std::fs::remove_file(path)
                    .with_context(|| format!("remove private artifact {}", path.display()))?;
                sync_directory(path.parent().expect("private artifact has parent"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("inspect private artifact {}", path.display()))
            }
        }
    }

    fn resource_hash(resource: &MaterializationResource) -> String {
        match resource {
            MaterializationResource::File { content } => hash_tagged(b"file", content.as_bytes()),
            MaterializationResource::Symlink { target } => {
                hash_tagged(b"symlink", target.as_str().as_bytes())
            }
        }
    }

    fn path_hash(path: &Path) -> anyhow::Result<String> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = std::fs::read_link(path)
                    .with_context(|| format!("read managed symlink {}", path.display()))?;
                let target = target
                    .to_str()
                    .context("managed symlink target is not valid UTF-8")?;
                Ok(hash_tagged(b"symlink", target.as_bytes()))
            }
            Ok(metadata) if is_symlink_or_reparse(&metadata) => {
                bail!(
                    "managed target is an unsupported reparse point: {}",
                    path.display()
                )
            }
            Ok(metadata) if metadata.is_file() => {
                let content = std::fs::read(path)
                    .with_context(|| format!("read managed profile {}", path.display()))?;
                Ok(hash_tagged(b"file", &content))
            }
            Ok(_) => bail!("managed target is not a file: {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ABSENT_HASH.to_owned())
            }
            Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
        }
    }

    fn ensure_replaceable_target(path: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => Ok(()),
            Ok(metadata) if is_symlink_or_reparse(&metadata) => {
                bail!(
                    "managed target is an unsupported reparse point: {}",
                    path.display()
                )
            }
            Ok(_) => bail!("managed target is not replaceable: {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
        }
    }

    fn stage_resource(
        root: &Path,
        operation_id: &str,
        resource: &MaterializationResource,
    ) -> anyhow::Result<()> {
        match resource {
            MaterializationResource::File { content } => Self::write_private_file_new(
                &Self::stage_file_path(root, operation_id),
                content.as_bytes(),
            ),
            MaterializationResource::Symlink { target } => Self::write_private_file_new(
                &Self::stage_link_path(root, operation_id),
                target.as_str().as_bytes(),
            ),
        }
    }

    fn read_staged_resource(
        root: &Path,
        operation_id: &str,
        expected_hash: &str,
    ) -> anyhow::Result<Option<StoredResource>> {
        let file_path = Self::stage_file_path(root, operation_id);
        let link_path = Self::stage_link_path(root, operation_id);
        let file_metadata = match std::fs::symlink_metadata(&file_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect staged file {}", file_path.display()));
            }
        };
        let link_metadata = match std::fs::symlink_metadata(&link_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "inspect staged symlink specification {}",
                        link_path.display()
                    )
                });
            }
        };
        if file_metadata.is_some() && link_metadata.is_some() {
            bail!("materialization has multiple staged resources");
        }
        if let Some(metadata) = file_metadata {
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                bail!("staged file is not a regular file");
            }
            let content = std::fs::read(&file_path)?;
            if hash_tagged(b"file", &content) != expected_hash {
                bail!("staged file hash mismatch");
            }
            return Ok(Some(StoredResource::File { path: file_path }));
        }
        if let Some(metadata) = link_metadata {
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                bail!("staged symlink specification is not a regular file");
            }
            let target = std::fs::read_to_string(&link_path)?;
            if hash_tagged(b"symlink", target.as_bytes()) != expected_hash {
                bail!("staged symlink hash mismatch");
            }
            return Ok(Some(StoredResource::Symlink {
                target: ExternalProfilePath::new(target)?,
            }));
        }
        Ok(None)
    }

    fn capture_backup(root: &Path, operation_id: &str, target: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link_target = std::fs::read_link(target)?;
                let link_target = link_target
                    .to_str()
                    .context("managed symlink target is not valid UTF-8")?;
                Self::write_private_file_new(
                    &Self::backup_link_path(root, operation_id),
                    link_target.as_bytes(),
                )
            }
            Ok(metadata) if is_symlink_or_reparse(&metadata) => {
                bail!(
                    "managed target is an unsupported reparse point: {}",
                    target.display()
                )
            }
            Ok(metadata) if metadata.is_file() => {
                let content = std::fs::read(target)?;
                Self::write_private_file_new(&Self::backup_file_path(root, operation_id), &content)
            }
            Ok(_) => bail!("managed target is not a file: {}", target.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("inspect {}", target.display())),
        }
    }

    fn read_backup_resource(
        root: &Path,
        operation_id: &str,
    ) -> anyhow::Result<Option<StoredResource>> {
        let file_path = Self::backup_file_path(root, operation_id);
        let link_path = Self::backup_link_path(root, operation_id);
        let file_metadata = match std::fs::symlink_metadata(&file_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect backup file {}", file_path.display()));
            }
        };
        let link_metadata = match std::fs::symlink_metadata(&link_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect backup link {}", link_path.display()));
            }
        };
        if file_metadata.is_some() && link_metadata.is_some() {
            bail!("materialization has multiple backups");
        }
        if let Some(metadata) = file_metadata {
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                bail!("backup file is not a regular file");
            }
            return Ok(Some(StoredResource::File { path: file_path }));
        }
        if let Some(metadata) = link_metadata {
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                bail!("backup symlink specification is not a regular file");
            }
            let target = std::fs::read_to_string(&link_path)?;
            return Ok(Some(StoredResource::Symlink {
                target: ExternalProfilePath::new(target)?,
            }));
        }
        Ok(None)
    }

    fn create_ready_link(
        root: &Path,
        operation_id: &str,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<PathBuf> {
        let ready = Self::ready_link_path(root, operation_id);
        match std::fs::symlink_metadata(&ready) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if std::fs::read_link(&ready)? != target.as_path() {
                    bail!("ready symlink target mismatch");
                }
            }
            Ok(_) => bail!("ready symlink path is occupied by an unexpected node"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_file_symlink(target.as_path(), &ready)
                    .with_context(|| format!("create staged symlink {}", ready.display()))?;
                sync_directory(ready.parent().expect("ready symlink has parent"))?;
            }
            Err(error) => return Err(error).context("inspect ready symlink"),
        }
        Ok(ready)
    }

    fn promote_resource(
        &self,
        root: &Path,
        operation_id: &str,
        target: &Path,
        expected_hash: &str,
    ) -> anyhow::Result<()> {
        self.ensure_managed_parent(target)?;
        Self::ensure_replaceable_target(target)?;
        let Some(resource) = Self::read_staged_resource(root, operation_id, expected_hash)? else {
            if Self::path_hash(target)? == expected_hash {
                return Ok(());
            }
            bail!("staged resource is missing and target hash does not match");
        };

        // Static symlink/reparse validation is mandatory. A same-user parent
        // swap after this recheck is outside this desktop app's local trust
        // boundary; descriptor-based confinement is intentionally not added.
        match resource {
            StoredResource::File { path } => {
                self.ensure_managed_parent(target)?;
                Self::ensure_replaceable_target(target)?;
                replace_atomic(&path, target).with_context(|| {
                    format!(
                        "promote staged file {} -> {}",
                        path.display(),
                        target.display()
                    )
                })?;
            }
            StoredResource::Symlink {
                target: link_target,
            } => {
                let ready = Self::create_ready_link(root, operation_id, &link_target)?;
                self.ensure_managed_parent(target)?;
                Self::ensure_replaceable_target(target)?;
                replace_atomic(&ready, target).with_context(|| {
                    format!(
                        "promote staged symlink {} -> {}",
                        ready.display(),
                        target.display()
                    )
                })?;
            }
        }
        if Self::path_hash(target)? != expected_hash {
            bail!("promoted target hash mismatch");
        }
        Ok(())
    }

    fn backup_hash(root: &Path, operation_id: &str) -> anyhow::Result<String> {
        match Self::read_backup_resource(root, operation_id)? {
            Some(StoredResource::File { path }) => {
                let content = std::fs::read(&path)
                    .with_context(|| format!("read backup file {}", path.display()))?;
                Ok(hash_tagged(b"file", &content))
            }
            Some(StoredResource::Symlink { target }) => {
                Ok(hash_tagged(b"symlink", target.as_str().as_bytes()))
            }
            None => Ok(ABSENT_HASH.to_owned()),
        }
    }

    fn target_is_pre_promote(
        root: &Path,
        operation_id: &str,
        target: &Path,
    ) -> anyhow::Result<bool> {
        Ok(Self::path_hash(target)? == Self::backup_hash(root, operation_id)?)
    }

    fn restore_backup(
        &self,
        root: &Path,
        operation_id: &str,
        target: &Path,
        promoted_hash: &str,
    ) -> anyhow::Result<bool> {
        let current_hash = Self::path_hash(target)?;
        if current_hash == Self::backup_hash(root, operation_id)? {
            return Ok(true);
        }
        if current_hash != promoted_hash {
            return Ok(false);
        }
        self.ensure_managed_parent(target)?;
        Self::ensure_replaceable_target(target)?;

        match Self::read_backup_resource(root, operation_id)? {
            Some(StoredResource::File { path }) => {
                let content = std::fs::read(&path)
                    .with_context(|| format!("read backup file {}", path.display()))?;
                let restore = Self::restore_file_path(root, operation_id);
                Self::remove_private_regular(&restore)?;
                Self::write_private_file_new(&restore, &content)?;
                self.ensure_managed_parent(target)?;
                Self::ensure_replaceable_target(target)?;
                replace_atomic(&restore, target).with_context(|| {
                    format!("restore backup {} -> {}", path.display(), target.display())
                })?;
            }
            Some(StoredResource::Symlink {
                target: link_target,
            }) => {
                let ready = Self::create_ready_link(root, operation_id, &link_target)?;
                self.ensure_managed_parent(target)?;
                Self::ensure_replaceable_target(target)?;
                replace_atomic(&ready, target).with_context(|| {
                    format!(
                        "restore backup symlink {} -> {}",
                        ready.display(),
                        target.display()
                    )
                })?;
            }
            None if current_hash == promoted_hash => Self::remove_nofollow(target)?,
            None => {}
        }
        Ok(true)
    }

    fn remove_private_artifacts(root: &Path, operation_id: &str) -> anyhow::Result<()> {
        for path in [
            Self::stage_file_path(root, operation_id),
            Self::restore_file_path(root, operation_id),
            Self::stage_link_path(root, operation_id),
            Self::backup_file_path(root, operation_id),
            Self::backup_link_path(root, operation_id),
        ] {
            Self::remove_private_regular(&path)?;
        }
        Self::remove_nofollow(&Self::ready_link_path(root, operation_id))
    }

    fn remove_operation_artifacts(
        root: &Path,
        operation_id: &str,
        journal: &Path,
    ) -> anyhow::Result<()> {
        // Retire the journal first. A crash after this point leaves only safe,
        // unreferenced private files, which reconcile can reclaim; it never
        // leaves a journal whose only backup was already consumed.
        Self::remove_private_regular(journal)?;
        Self::remove_private_artifacts(root, operation_id)
    }

    fn operation_id_in_use(root: &Path, operation_id: &str) -> bool {
        let artifact_paths = [
            Self::stage_file_path(root, operation_id),
            Self::stage_link_path(root, operation_id),
            Self::ready_link_path(root, operation_id),
            Self::backup_file_path(root, operation_id),
            Self::backup_link_path(root, operation_id),
            Self::cleanup_path(root, CleanupPhase::Pending, operation_id),
            Self::cleanup_path(root, CleanupPhase::Ready, operation_id),
        ];
        if artifact_paths
            .iter()
            .any(|path| std::fs::symlink_metadata(path).is_ok())
        {
            return true;
        }
        JournalLocation::ALL.iter().any(|location| {
            std::fs::symlink_metadata(Self::journal_path(root, *location, operation_id)).is_ok()
        })
    }

    fn allocate_operation_id(root: &Path) -> anyhow::Result<String> {
        for _ in 0..16 {
            let operation_id = nanoid::nanoid!(16, &nanoid::alphabet::SAFE);
            if !Self::operation_id_in_use(root, &operation_id) {
                return Ok(operation_id);
            }
        }
        bail!("failed to allocate a unique profile materialization operation id")
    }

    fn prepare_materialization(
        &self,
        path: &ManagedProfilePath,
        resource: &MaterializationResource,
        expected_revision: u64,
        location: JournalLocation,
    ) -> anyhow::Result<PreparedMaterialization> {
        let root = self.ensure_materialization_layout()?;
        let target = self.resolve(path)?;
        self.ensure_managed_parent(&target)?;
        Self::ensure_replaceable_target(&target)?;
        let operation_id = Self::allocate_operation_id(&root)?;

        if let Err(error) = (|| {
            Self::stage_resource(&root, &operation_id, resource)?;
            Self::capture_backup(&root, &operation_id, &target)?;
            let journal = MaterializationJournal {
                managed_path: path.clone(),
                operation_id: operation_id.clone(),
                revision: expected_revision,
                hash: Self::resource_hash(resource),
            };
            Self::write_journal_new(
                &Self::journal_path(&root, location, &operation_id),
                &journal,
            )
        })() {
            let _ = Self::remove_private_regular(&Self::stage_file_path(&root, &operation_id));
            let _ = Self::remove_private_regular(&Self::stage_link_path(&root, &operation_id));
            let _ = Self::remove_private_regular(&Self::backup_file_path(&root, &operation_id));
            let _ = Self::remove_private_regular(&Self::backup_link_path(&root, &operation_id));
            return Err(error);
        }
        Ok(PreparedMaterialization::new(operation_id))
    }

    fn locate_materialization(
        &self,
        root: &Path,
        operation_id: &str,
    ) -> anyhow::Result<Option<(JournalLocation, MaterializationJournal)>> {
        if !valid_operation_id(operation_id) {
            bail!("invalid profile materialization operation id");
        }
        let mut found = Vec::new();
        for location in JournalLocation::ALL {
            let path = Self::journal_path(root, location, operation_id);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                        bail!(
                            "materialization journal is not a regular file: {}",
                            path.display()
                        );
                    }
                    found.push((location, Self::read_journal(&path, operation_id)?, path));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("inspect journal {}", path.display()));
                }
            }
        }
        Self::converge_materialization_journals(found)
    }

    fn converge_materialization_journals(
        mut found: Vec<(JournalLocation, MaterializationJournal, PathBuf)>,
    ) -> anyhow::Result<Option<(JournalLocation, MaterializationJournal)>> {
        let Some(family) = found.first().map(|(location, _, _)| location.family()) else {
            return Ok(None);
        };
        if found
            .iter()
            .any(|(location, _, _)| location.family() != family)
        {
            bail!("materialization operation has mixed transaction families");
        }
        found.sort_by_key(|(location, _, _)| location.rank());
        let (location, journal, _) = found
            .pop()
            .expect("non-empty materialization journal set has a preferred phase");
        for (duplicate_location, duplicate_journal, duplicate_path) in found {
            if duplicate_journal != journal {
                bail!(
                    "materialization operation has conflicting journal payloads in {duplicate_location:?}"
                );
            }
            Self::remove_private_regular(&duplicate_path)?;
        }
        Ok(Some((location, journal)))
    }

    /// The journal locations share one private root, so Unix `rename` is an
    /// atomic same-filesystem phase transition. Do not use `move_atomic`: its
    /// hard-link/unlink fallback can leave duplicate phase artifacts.
    fn rename_journal_same_filesystem(source: &Path, destination: &Path) -> anyhow::Result<()> {
        let metadata = std::fs::symlink_metadata(source)
            .with_context(|| format!("inspect journal source {}", source.display()))?;
        if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
            bail!("journal source is not a regular file: {}", source.display());
        }
        std::fs::rename(source, destination).with_context(|| {
            format!(
                "atomically rename journal {} -> {}",
                source.display(),
                destination.display()
            )
        })?;
        sync_directory(source.parent().expect("journal source has parent"))?;
        if source.parent() != destination.parent() {
            sync_directory(
                destination
                    .parent()
                    .expect("journal destination has parent"),
            )?;
        }
        Ok(())
    }

    fn advance_journal_phase(
        source: &Path,
        destination: &Path,
        journal: &MaterializationJournal,
    ) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            // `std::fs::rename` has no directory fsync equivalent on Windows.
            // Write the later phase through first; a crash can retain both
            // matching phases, which recovery intentionally converges.
            Self::write_journal_new(destination, journal)?;
            Self::remove_private_regular(source)
        }
        #[cfg(not(windows))]
        {
            let _ = journal;
            Self::rename_journal_same_filesystem(source, destination)
        }
    }

    fn transition_journal(
        root: &Path,
        operation_id: &str,
        from: JournalLocation,
        to: JournalLocation,
    ) -> anyhow::Result<()> {
        let source = Self::journal_path(root, from, operation_id);
        let destination = Self::journal_path(root, to, operation_id);
        let source_journal = Self::read_journal(&source, operation_id)?;
        match std::fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::advance_journal_phase(&source, &destination, &source_journal)
            }
            Ok(metadata) if is_symlink_or_reparse(&metadata) || !metadata.is_file() => bail!(
                "journal destination is not a regular file: {}",
                destination.display()
            ),
            Ok(_) => {
                if Self::read_journal(&destination, operation_id)? != source_journal {
                    bail!("journal destination has a different payload");
                }
                Self::remove_private_regular(&source)
            }
            Err(error) => Err(error).context("inspect journal destination"),
        }
    }

    fn transition_cleanup_journal(
        root: &Path,
        operation_id: &str,
        from: CleanupPhase,
        to: CleanupPhase,
    ) -> anyhow::Result<()> {
        let source = Self::cleanup_path(root, from, operation_id);
        let destination = Self::cleanup_path(root, to, operation_id);
        let source_journal = Self::read_journal(&source, operation_id)?;
        match std::fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::advance_journal_phase(&source, &destination, &source_journal)
            }
            Ok(metadata) if is_symlink_or_reparse(&metadata) || !metadata.is_file() => bail!(
                "cleanup journal destination is not a regular file: {}",
                destination.display()
            ),
            Ok(_) => {
                if Self::read_journal(&destination, operation_id)? != source_journal {
                    bail!("cleanup journal destination has a different payload");
                }
                Self::remove_private_regular(&source)
            }
            Err(error) => Err(error).context("inspect cleanup journal destination"),
        }
    }

    fn discard_materialization(
        root: &Path,
        operation_id: &str,
        location: JournalLocation,
    ) -> anyhow::Result<()> {
        Self::remove_operation_artifacts(
            root,
            operation_id,
            &Self::journal_path(root, location, operation_id),
        )
    }

    fn list_operation_ids(directory: &Path) -> anyhow::Result<Vec<String>> {
        let mut operation_ids = Vec::new();
        for entry in std::fs::read_dir(directory)
            .with_context(|| format!("list private journal directory {}", directory.display()))?
        {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                continue;
            }
            let Ok(filename) = entry.file_name().into_string() else {
                continue;
            };
            let Some(operation_id) = filename.strip_suffix(".yaml") else {
                continue;
            };
            if !valid_operation_id(operation_id) {
                continue;
            }
            operation_ids.push(operation_id.to_owned());
        }
        operation_ids.sort();
        Ok(operation_ids)
    }

    fn active_managed_paths(profiles: &Profiles) -> HashSet<ManagedProfilePath> {
        profiles
            .items
            .values()
            .filter_map(|item| {
                item.definition
                    .source()
                    .map(|source| source.materialized().file.clone())
            })
            .collect()
    }

    fn locate_cleanup(
        root: &Path,
        operation_id: &str,
    ) -> anyhow::Result<Option<(CleanupPhase, MaterializationJournal)>> {
        if !valid_operation_id(operation_id) {
            bail!("invalid profile cleanup operation id");
        }
        let pending_path = Self::cleanup_path(root, CleanupPhase::Pending, operation_id);
        let ready_path = Self::cleanup_path(root, CleanupPhase::Ready, operation_id);
        let pending = match std::fs::symlink_metadata(&pending_path) {
            Ok(metadata) if !is_symlink_or_reparse(&metadata) && metadata.is_file() => {
                Some(Self::read_journal(&pending_path, operation_id)?)
            }
            Ok(_) => bail!("pending cleanup journal is not a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("inspect pending cleanup journal"),
        };
        let ready = match std::fs::symlink_metadata(&ready_path) {
            Ok(metadata) if !is_symlink_or_reparse(&metadata) && metadata.is_file() => {
                Some(Self::read_journal(&ready_path, operation_id)?)
            }
            Ok(_) => bail!("ready cleanup journal is not a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("inspect ready cleanup journal"),
        };
        match (pending, ready) {
            (None, None) => Ok(None),
            (Some(journal), None) => Ok(Some((CleanupPhase::Pending, journal))),
            (None, Some(journal)) => Ok(Some((CleanupPhase::Ready, journal))),
            (Some(pending), Some(ready)) if pending == ready => {
                Self::remove_private_regular(&pending_path)?;
                Ok(Some((CleanupPhase::Ready, ready)))
            }
            (Some(_), Some(_)) => bail!("cleanup journals have conflicting payloads"),
        }
    }

    fn has_materialization_journal(root: &Path, operation_id: &str) -> bool {
        JournalLocation::ALL.iter().any(|location| {
            std::fs::symlink_metadata(Self::journal_path(root, *location, operation_id)).is_ok()
        })
    }

    fn has_cleanup_journal(root: &Path, operation_id: &str) -> bool {
        [CleanupPhase::Pending, CleanupPhase::Ready]
            .iter()
            .any(|phase| {
                std::fs::symlink_metadata(Self::cleanup_path(root, *phase, operation_id)).is_ok()
            })
    }

    fn remove_cleanup_tombstone(root: &Path, operation_id: &str) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            Self::remove_nofollow(&Self::cleanup_tombstone_path(root, operation_id))
        }
        #[cfg(not(windows))]
        {
            let _ = (root, operation_id);
            Ok(())
        }
    }

    fn sweep_unreferenced_cleanup_tombstones(root: &Path) -> anyhow::Result<usize> {
        #[cfg(windows)]
        {
            let mut removed = 0;
            for entry in std::fs::read_dir(root.join("cleanup/tombstones"))? {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if !metadata.is_file() && !metadata.file_type().is_symlink() {
                    continue;
                }
                let Ok(operation_id) = entry.file_name().into_string() else {
                    continue;
                };
                if valid_operation_id(&operation_id)
                    && !Self::has_cleanup_journal(root, &operation_id)
                {
                    Self::remove_nofollow(&path)?;
                    removed += 1;
                }
            }
            Ok(removed)
        }
        #[cfg(not(windows))]
        {
            let _ = root;
            Ok(0)
        }
    }

    fn sweep_unreferenced_artifacts(root: &Path) -> anyhow::Result<usize> {
        let mut removed = 0;
        for (relative, permits_symlink) in [
            ("staging/files", false),
            ("staging/links", false),
            ("staging/ready-links", true),
            ("backup/files", false),
            ("backup/links", false),
        ] {
            for entry in std::fs::read_dir(root.join(relative))? {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                let is_ready_link = permits_symlink && metadata.file_type().is_symlink();
                if (!is_ready_link && is_symlink_or_reparse(&metadata))
                    || (!is_ready_link && !metadata.is_file())
                {
                    continue;
                }
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                let operation_id = name.strip_suffix(".restore").unwrap_or(&name);
                if valid_operation_id(operation_id)
                    && !Self::has_materialization_journal(root, operation_id)
                {
                    Self::remove_private_artifacts(root, operation_id)?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    fn remove_cleanup_target(
        &self,
        root: &Path,
        operation_id: &str,
        target: &Path,
    ) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            let tombstone = Self::cleanup_tombstone_path(root, operation_id);
            match std::fs::symlink_metadata(&tombstone) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => bail!("cleanup tombstone already exists: {}", tombstone.display()),
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("inspect cleanup tombstone {}", tombstone.display())
                    });
                }
            }
            self.ensure_managed_parent(target)?;
            Self::ensure_replaceable_target(target)?;
            replace_atomic(target, &tombstone).with_context(|| {
                format!(
                    "move cleanup target {} to tombstone {}",
                    target.display(),
                    tombstone.display()
                )
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (root, operation_id);
            Self::remove_nofollow(target)
        }
    }

    fn remove_cleanup_journal(root: &Path, operation_id: &str) -> anyhow::Result<()> {
        Self::remove_private_regular(&Self::cleanup_path(
            root,
            CleanupPhase::Pending,
            operation_id,
        ))?;
        Self::remove_private_regular(&Self::cleanup_path(root, CleanupPhase::Ready, operation_id))
    }

    fn degradation(
        phase: ProfileDegradationPhase,
        code: ProfileDegradationCode,
        error: anyhow::Error,
    ) -> ProfileDegradation {
        ProfileDegradation {
            phase,
            code,
            message: format!("{error:#}"),
        }
    }
}

/// Parse and reserialize a YAML mapping so editor saves and File-config reads
/// share one canonical shape (legacy `read_profile_file` normalization).
pub fn normalize_yaml_document(content: &str) -> anyhow::Result<String> {
    let mapping: serde_yaml::Mapping =
        serde_yaml::from_str(content).context("document is not a YAML mapping")?;
    serde_yaml::to_string(&mapping).context("failed to reserialize YAML mapping")
}

impl ProfileFsPort for ProfileFileService {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String> {
        let full = self.resolve(path)?;
        std::fs::read_to_string(&full)
            .with_context(|| format!("read profile file {}", full.display()))
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        self.ensure_managed_parent(&full)?;
        self.ensure_not_symlink(path)?;
        AtomicFile::new(&full, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("atomic write {}", full.display()))
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        Self::remove_nofollow(&full)
    }

    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String> {
        std::fs::read_to_string(target.as_path())
            .with_context(|| format!("read external profile target {target}"))
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if is_symlink_or_reparse(&meta) => bail!(
                "refusing to write through unexpected symlink or reparse point at {}",
                full.display()
            ),
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("inspect profile file {}", full.display())),
        }
    }

    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        self.ensure_managed_parent(&full)?;
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if symlink_points_to(&full, target.as_path())? {
                    return Ok(());
                }
                std::fs::remove_file(&full)?;
            }
            Ok(_) => bail!(
                "existing non-symlink file at {}, refusing to replace",
                full.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => Err(e).with_context(|| format!("inspect profile file {}", full.display()))?,
        }
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target.as_path(), &full)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target.as_path(), &full)?;
        Ok(())
    }
}

impl ProfileMaterializationPort for ProfileFileService {
    fn prepare_state_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization> {
        self.prepare_materialization(
            path,
            &resource,
            expected_revision,
            JournalLocation::StatePrepared,
        )
    }

    fn prepare_file_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization> {
        self.prepare_materialization(
            path,
            &resource,
            expected_revision,
            JournalLocation::FilePrepared,
        )
    }

    fn promote(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = prepared.operation_id();
        let Some((mut location, journal)) = self.locate_materialization(&root, operation_id)?
        else {
            bail!("materialization journal not found for operation {operation_id}");
        };

        if let Some(promoting) = location.promoting() {
            Self::transition_journal(&root, operation_id, location, promoting)?;
            location = promoting;
        }
        if matches!(
            location,
            JournalLocation::StateCompensating | JournalLocation::FileCompensating
        ) {
            bail!("cannot promote a compensating materialization");
        }

        let target = self.resolve(&journal.managed_path)?;
        if Self::path_hash(&target)? != journal.hash {
            self.promote_resource(&root, operation_id, &target, &journal.hash)?;
        }
        if location == JournalLocation::FilePromoting {
            Self::transition_journal(
                &root,
                operation_id,
                JournalLocation::FilePromoting,
                JournalLocation::FilePromoted,
            )?;
        }
        Ok(())
    }

    fn complete(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = prepared.operation_id();
        let Some((mut location, journal)) = self.locate_materialization(&root, operation_id)?
        else {
            return Ok(());
        };
        let target = self.resolve(&journal.managed_path)?;
        if Self::path_hash(&target)? != journal.hash {
            bail!("cannot complete materialization with a target hash mismatch");
        }
        if location == JournalLocation::FilePromoting {
            Self::transition_journal(
                &root,
                operation_id,
                JournalLocation::FilePromoting,
                JournalLocation::FilePromoted,
            )?;
            location = JournalLocation::FilePromoted;
        }
        if !matches!(
            location,
            JournalLocation::StatePromoting | JournalLocation::FilePromoted
        ) {
            bail!("materialization is not in a completable phase");
        }
        Self::remove_operation_artifacts(
            &root,
            operation_id,
            &Self::journal_path(&root, location, operation_id),
        )
    }

    fn compensate(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = prepared.operation_id();
        let Some((mut location, journal)) = self.locate_materialization(&root, operation_id)?
        else {
            return Ok(());
        };
        let compensating = location.compensating();
        if location != compensating {
            Self::transition_journal(&root, operation_id, location, compensating)?;
            location = compensating;
        }

        let target = self.resolve(&journal.managed_path)?;
        if !self.restore_backup(&root, operation_id, &target, &journal.hash)? {
            bail!(
                "compensation fenced by diverged target at {}",
                journal.managed_path
            );
        }
        Self::remove_operation_artifacts(
            &root,
            operation_id,
            &Self::journal_path(&root, location, operation_id),
        )
    }

    fn prepare_cleanup(
        &self,
        path: &ManagedProfilePath,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedCleanup> {
        let root = self.ensure_materialization_layout()?;
        let target = self.resolve(path)?;
        let operation_id = Self::allocate_operation_id(&root)?;
        let journal = MaterializationJournal {
            managed_path: path.clone(),
            operation_id: operation_id.clone(),
            revision: expected_revision,
            hash: Self::path_hash(&target)?,
        };
        Self::write_journal_new(
            &Self::cleanup_path(&root, CleanupPhase::Pending, &operation_id),
            &journal,
        )?;
        Ok(PreparedCleanup::new(operation_id))
    }

    fn activate_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = cleanup.operation_id();
        let Some((phase, _)) = Self::locate_cleanup(&root, operation_id)? else {
            return Ok(());
        };
        if phase == CleanupPhase::Ready {
            return Ok(());
        }
        Self::transition_cleanup_journal(
            &root,
            operation_id,
            CleanupPhase::Pending,
            CleanupPhase::Ready,
        )
        .with_context(|| format!("activate cleanup operation {operation_id}"))
    }

    fn cancel_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = cleanup.operation_id();
        match Self::locate_cleanup(&root, operation_id)? {
            None => Ok(()),
            Some((CleanupPhase::Pending, _)) => Self::remove_private_regular(&Self::cleanup_path(
                &root,
                CleanupPhase::Pending,
                operation_id,
            )),
            Some((CleanupPhase::Ready, _)) => {
                bail!("cannot cancel an activated cleanup operation")
            }
        }
    }

    fn retry_cleanup(
        &self,
        cleanup: &PreparedCleanup,
        profiles: &Profiles,
    ) -> anyhow::Result<CleanupOutcome> {
        let root = self.ensure_materialization_layout()?;
        let operation_id = cleanup.operation_id();
        let Some((phase, journal)) = Self::locate_cleanup(&root, operation_id)? else {
            return Ok(CleanupOutcome::AlreadyAbsent);
        };
        if phase == CleanupPhase::Pending {
            self.activate_cleanup(cleanup)?;
        }

        let active_paths = Self::active_managed_paths(profiles);
        if active_paths.contains(&journal.managed_path) {
            Self::remove_cleanup_journal(&root, operation_id)?;
            return Ok(CleanupOutcome::FencedActivePath);
        }

        let target = self.resolve(&journal.managed_path)?;
        let current_hash = Self::path_hash(&target)?;
        if current_hash == ABSENT_HASH {
            Self::remove_cleanup_tombstone(&root, operation_id)?;
            Self::remove_cleanup_journal(&root, operation_id)?;
            return Ok(CleanupOutcome::AlreadyAbsent);
        }
        if current_hash != journal.hash {
            Self::remove_cleanup_journal(&root, operation_id)?;
            return Ok(CleanupOutcome::FencedHashMismatch);
        }

        self.remove_cleanup_target(&root, operation_id, &target)?;
        Self::remove_cleanup_journal(&root, operation_id)?;
        Self::remove_cleanup_tombstone(&root, operation_id)?;
        Ok(CleanupOutcome::Removed)
    }

    fn reconcile(&self, profiles: &Profiles) -> anyhow::Result<MaterializationReconcileReport> {
        let root = self.ensure_materialization_layout()?;
        let revision = profiles.revision();
        let active_paths = Self::active_managed_paths(profiles);
        let mut report = MaterializationReconcileReport::default();
        if let Err(error) = Self::sweep_unreferenced_artifacts(&root) {
            report.degradations.push(Self::degradation(
                ProfileDegradationPhase::Reconcile,
                ProfileDegradationCode::MaterializationDeferred,
                error,
            ));
        }
        if let Err(error) = Self::sweep_unreferenced_cleanup_tombstones(&root) {
            report.degradations.push(Self::degradation(
                ProfileDegradationPhase::Cleanup,
                ProfileDegradationCode::CleanupDeferred,
                error,
            ));
        }

        for location in JournalLocation::ALL {
            let operation_ids = match Self::list_operation_ids(&root.join(location.directory())) {
                Ok(operation_ids) => operation_ids,
                Err(error) => {
                    report.degradations.push(Self::degradation(
                        ProfileDegradationPhase::Reconcile,
                        ProfileDegradationCode::JournalInvalid,
                        error,
                    ));
                    continue;
                }
            };

            for operation_id in operation_ids {
                let prepared = PreparedMaterialization::new(operation_id.clone());
                let recovered = (|| -> anyhow::Result<(usize, usize, usize, usize)> {
                    let Some((actual_location, journal)) =
                        self.locate_materialization(&root, &operation_id)?
                    else {
                        return Ok((0, 0, 0, 0));
                    };
                    if actual_location != location {
                        return Ok((0, 0, 0, 0));
                    }
                    let target = self.resolve(&journal.managed_path)?;
                    let active = active_paths.contains(&journal.managed_path);
                    let target_matches = Self::path_hash(&target)? == journal.hash;

                    if actual_location == JournalLocation::StateCompensating
                        || actual_location == JournalLocation::FileCompensating
                    {
                        self.compensate(&prepared)?;
                        return Ok((0, 0, 0, 1));
                    }

                    if revision > journal.revision {
                        if active {
                            match actual_location {
                                JournalLocation::StatePrepared
                                | JournalLocation::StatePromoting => {
                                    if target_matches {
                                        if actual_location == JournalLocation::StatePromoting {
                                            self.complete(&prepared)?;
                                            return Ok((0, 0, 1, 0));
                                        }
                                        Self::discard_materialization(
                                            &root,
                                            &operation_id,
                                            actual_location,
                                        )?;
                                        return Ok((1, 0, 0, 0));
                                    }
                                    if !Self::target_is_pre_promote(&root, &operation_id, &target)?
                                    {
                                        bail!(
                                            "active materialization target diverged before recovery"
                                        );
                                    }
                                    // revision advanced past this journal while the target is
                                    // still pre-promote: the transaction was superseded or
                                    // rolled back. Compensate instead of applying stale staged
                                    // content onto a newer committed profile revision.
                                    self.compensate(&prepared)?;
                                    return Ok((0, 0, 0, 1));
                                }
                                JournalLocation::FilePrepared if target_matches => {
                                    self.promote(&prepared)?;
                                    self.complete(&prepared)?;
                                    return Ok((0, 0, 1, 0));
                                }
                                JournalLocation::FilePromoting | JournalLocation::FilePromoted
                                    if target_matches =>
                                {
                                    self.complete(&prepared)?;
                                    return Ok((0, 0, 1, 0));
                                }
                                JournalLocation::FilePrepared
                                | JournalLocation::FilePromoting
                                | JournalLocation::FilePromoted => {
                                    self.compensate(&prepared)?;
                                    return Ok((1, 0, 0, 0));
                                }
                                JournalLocation::StateCompensating
                                | JournalLocation::FileCompensating => unreachable!(),
                            }
                        }
                        if target_matches {
                            Self::remove_nofollow(&target)?;
                        }
                        Self::discard_materialization(&root, &operation_id, actual_location)?;
                        return Ok((1, 0, 0, 0));
                    }

                    if revision < journal.revision || !active {
                        self.compensate(&prepared)?;
                        return Ok((0, 0, 0, 1));
                    }

                    match actual_location {
                        JournalLocation::StatePrepared
                        | JournalLocation::StatePromoting
                        | JournalLocation::FilePromoting
                        | JournalLocation::FilePromoted => {
                            self.promote(&prepared)?;
                            self.complete(&prepared)?;
                            Ok((0, 1, 1, 0))
                        }
                        JournalLocation::FilePrepared if target_matches => {
                            self.promote(&prepared)?;
                            self.complete(&prepared)?;
                            Ok((0, 0, 1, 0))
                        }
                        JournalLocation::FilePrepared => {
                            self.compensate(&prepared)?;
                            Ok((1, 0, 0, 0))
                        }
                        JournalLocation::StateCompensating | JournalLocation::FileCompensating => {
                            unreachable!()
                        }
                    }
                })();

                match recovered {
                    Ok((discarded, promoted, completed, compensated)) => {
                        report.discarded += discarded;
                        report.promoted += promoted;
                        report.completed += completed;
                        report.compensated += compensated;
                    }
                    Err(error) => report.degradations.push(Self::degradation(
                        ProfileDegradationPhase::Reconcile,
                        ProfileDegradationCode::MaterializationDeferred,
                        error,
                    )),
                }
            }
        }

        let pending_ids =
            match Self::list_operation_ids(&root.join(CleanupPhase::Pending.directory())) {
                Ok(operation_ids) => operation_ids,
                Err(error) => {
                    report.degradations.push(Self::degradation(
                        ProfileDegradationPhase::Cleanup,
                        ProfileDegradationCode::JournalInvalid,
                        error,
                    ));
                    Vec::new()
                }
            };
        for operation_id in pending_ids {
            let cleanup = PreparedCleanup::new(operation_id);
            let result = (|| -> anyhow::Result<()> {
                let Some((_, journal)) = Self::locate_cleanup(&root, cleanup.operation_id())?
                else {
                    return Ok(());
                };
                if active_paths.contains(&journal.managed_path) {
                    self.cancel_cleanup(&cleanup)
                } else {
                    self.activate_cleanup(&cleanup)
                }
            })();
            if let Err(error) = result {
                report.degradations.push(Self::degradation(
                    ProfileDegradationPhase::Cleanup,
                    ProfileDegradationCode::CleanupDeferred,
                    error,
                ));
            }
        }

        let ready_ids = match Self::list_operation_ids(&root.join(CleanupPhase::Ready.directory()))
        {
            Ok(operation_ids) => operation_ids,
            Err(error) => {
                report.degradations.push(Self::degradation(
                    ProfileDegradationPhase::Cleanup,
                    ProfileDegradationCode::JournalInvalid,
                    error,
                ));
                Vec::new()
            }
        };
        for operation_id in ready_ids {
            match self.retry_cleanup(&PreparedCleanup::new(operation_id), profiles) {
                Ok(CleanupOutcome::Removed | CleanupOutcome::AlreadyAbsent) => {
                    report.cleanups_completed += 1;
                }
                Ok(CleanupOutcome::FencedActivePath | CleanupOutcome::FencedHashMismatch) => {
                    report.cleanups_fenced += 1;
                }
                Err(error) => report.degradations.push(Self::degradation(
                    ProfileDegradationPhase::Cleanup,
                    ProfileDegradationCode::CleanupDeferred,
                    error,
                )),
            }
        }

        Ok(report)
    }
}

#[async_trait::async_trait]
impl SubscriptionFetcher for ProfileFileService {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription> {
        use backon::Retryable;

        let mut builder = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .no_proxy()
            .timeout(self.http_timeout);

        // Proxy precedence mirrors the legacy subscriber (remote.rs:129-150):
        // self_proxy wins, then system proxy, else direct.
        let proxy_url = if options.self_proxy {
            self.self_proxy_port
                .mixed_port()
                .map(|port| format!("http://127.0.0.1:{port}"))
        } else {
            None
        };
        let proxy_url = proxy_url.or_else(|| {
            if options.with_proxy {
                match sysproxy::Sysproxy::get_system_proxy() {
                    Ok(p @ sysproxy::Sysproxy { enable: true, .. }) => {
                        Some(format!("http://{}:{}", p.host, p.port))
                    }
                    _ => None,
                }
            } else {
                None
            }
        });
        if let Some(proxy_url) = proxy_url {
            use crate::utils::config::NyanpasuReqwestProxyExt;
            builder = builder.swift_set_proxy(&proxy_url);
        }

        let user_agent = options
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("clash-nyanpasu/v{}", crate::utils::dirs::APP_VERSION));
        let client = builder.user_agent(user_agent).build()?;

        let device_info = crate::utils::hwid::get_device_info();
        let sanitize = crate::utils::hwid::sanitize_for_header;
        let perform = || async {
            client
                .get(url.as_str())
                .header("x-hwid", &device_info.hwid)
                .header("x-device-os", sanitize(&device_info.device_os))
                .header("x-ver-os", sanitize(&device_info.os_version))
                .header("x-device-model", sanitize(&device_info.device_model))
                .send()
                .await?
                .error_for_status()
        };
        let resp = perform
            .retry(backon::ExponentialBuilder::default())
            .when(|error: &reqwest::Error| {
                !error.is_status()
                    || error.status().is_some_and(|status| {
                        !matches!(
                            status,
                            reqwest::StatusCode::FORBIDDEN
                                | reqwest::StatusCode::NOT_FOUND
                                | reqwest::StatusCode::UNAUTHORIZED
                        )
                    })
            })
            .await
            .with_context(|| format!("subscription download failed: {url}"))?;

        let subscription = parse_subscription_userinfo(resp.headers());
        let filename = parse_profile_title(resp.headers());
        let suggested_update_interval_minutes = parse_suggested_update_interval(resp.headers());
        let content = resp
            .text_with_charset("utf-8")
            .await
            .with_context(|| format!("read subscription response body: {url}"))?;
        let content = if let Some(content) = content.strip_prefix('\u{feff}') {
            content.to_owned()
        } else {
            content
        };
        Ok(FetchedSubscription {
            content,
            filename,
            subscription,
            suggested_update_interval_minutes,
        })
    }
}

fn parse_suggested_update_interval(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let hours = headers
        .get("profile-update-interval")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|hours| *hours > 0)?;
    let minutes = hours.checked_mul(60)?;
    minutes.checked_mul(60)?;
    Some(minutes)
}

fn parse_subscription_userinfo(headers: &reqwest::header::HeaderMap) -> SubscriptionInfo {
    let Some(value) = headers
        .get("subscription-userinfo")
        .or_else(|| headers.get("Subscription-Userinfo"))
    else {
        return SubscriptionInfo::default();
    };
    let raw = value.to_str().unwrap_or("");
    let field = |key: &str| crate::utils::help::parse_str::<u64>(raw, key);
    SubscriptionInfo {
        upload: field("upload"),
        download: field("download"),
        total: field("total"),
        expire: field("expire")
            .filter(|secs| *secs != 0)
            .and_then(|secs| i64::try_from(secs).ok())
            .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs).ok()),
    }
}

fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("profile-title").and_then(|v| v.to_str().ok()) {
        if value.trim().is_empty() {
            return parse_filename_from_content_disposition(headers);
        }
        if let Some(encoded) = value.strip_prefix("base64:") {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    if !decoded.trim().is_empty() {
                        return Some(decoded);
                    }
                }
            }
        } else {
            return Some(value.to_owned());
        }
    }

    parse_filename_from_content_disposition(headers)
}

fn parse_filename_from_content_disposition(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())?;

    value
        .split(';')
        .map(str::trim)
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("filename*") {
                decode_rfc5987_filename(value)
            } else if name.eq_ignore_ascii_case("filename") {
                Some(value.trim_matches(['"', '\'']).to_owned())
            } else {
                None
            }
        })
        .filter(|filename| !filename.is_empty())
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(['"', '\'']);
    let encoded = value
        .split_once('\'')
        .and_then(|(_, rest)| rest.split_once('\'').map(|(_, encoded)| encoded))
        .unwrap_or(value);
    percent_encoding::percent_decode(encoded.as_bytes())
        .decode_utf8()
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::SubscriptionFetcher;
    use axum::{
        Router,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use nyanpasu_config::profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource, Profiles,
        RemoteProfileOptions,
    };
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use url::Url;

    struct NoProxy;
    impl SelfProxyPortSource for NoProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    struct CountingNoProxy {
        hits: Arc<AtomicUsize>,
    }

    impl SelfProxyPortSource for CountingNoProxy {
        fn mixed_port(&self) -> Option<u16> {
            self.hits.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        service_with(Arc::new(NoProxy))
    }

    fn service_with(
        self_proxy_port: Arc<dyn SelfProxyPortSource>,
    ) -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (temp, ProfileFileService::new(paths, self_proxy_port))
    }

    fn managed(name: &str) -> ManagedProfilePath {
        ManagedProfilePath::new(name).unwrap()
    }

    fn file_resource(content: &str) -> MaterializationResource {
        MaterializationResource::File {
            content: content.to_owned(),
        }
    }

    fn profiles_at_revision(revision: u64) -> Profiles {
        let mut profiles = Profiles::default();
        for _ in 0..revision {
            profiles
                .bump_revision()
                .expect("test revision must not overflow");
        }
        profiles
    }

    fn profiles_with_path(path: &ManagedProfilePath) -> Profiles {
        let mut profiles = Profiles::default();
        profiles.append_item(ProfileItem {
            uid: ProfileId("profile".into()),
            metadata: ProfileMetadata {
                name: "Profile".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: path.clone(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        });
        profiles
    }

    fn profiles_with_path_at_revision(path: &ManagedProfilePath, revision: u64) -> Profiles {
        let mut profiles = profiles_with_path(path);
        for _ in 0..revision {
            profiles
                .bump_revision()
                .expect("test revision must not overflow");
        }
        profiles
    }

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}/"), handle)
    }

    fn options_direct() -> RemoteProfileOptions {
        options(false, false)
    }

    fn options(self_proxy: bool, with_proxy: bool) -> RemoteProfileOptions {
        RemoteProfileOptions {
            user_agent: None,
            with_proxy,
            self_proxy,
            update_interval_minutes: 120,
        }
    }

    #[test]
    fn write_atomic_then_read_round_trips_and_creates_parent() {
        let (_temp, service) = service();
        let path = managed("abc.yaml");
        service.write_atomic(&path, "proxies: []\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "proxies: []\n");
        service.write_atomic(&path, "mode: rule\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "mode: rule\n");
    }

    #[test]
    fn write_atomic_rejects_parent_symlink_escape() {
        let (temp, service) = service();
        let profiles_dir = service.paths.app_profiles_dir();
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let link_dir = profiles_dir.join("nested");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, &link_dir);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link_dir);
        if made.is_err() {
            eprintln!("directory symlink unsupported in this environment, skipping");
            return;
        }

        let err = service
            .write_atomic(&managed("nested/x.yaml"), "escaped: true\n")
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("symlink") || message.contains("reparse"));
        assert!(!outside.join("x.yaml").exists());
    }

    #[test]
    fn remove_is_idempotent() {
        let (_temp, service) = service();
        let path = managed("gone.yaml");
        service.remove(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.remove(&path).unwrap();
        assert!(service.read(&path).is_err());
    }

    #[test]
    fn state_first_stage_promote_and_complete_are_durable() {
        let (_temp, service) = service();
        let path = managed("state-first.yaml");
        let prepared = service
            .prepare_state_first(&path, file_resource("new: true\n"), 4)
            .unwrap();
        let root = service.materialization_root();

        assert!(!service.resolve(&path).unwrap().exists());
        assert!(ProfileFileService::stage_file_path(&root, prepared.operation_id()).exists());
        assert!(
            ProfileFileService::journal_path(
                &root,
                JournalLocation::StatePrepared,
                prepared.operation_id(),
            )
            .exists()
        );

        service.promote(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "new: true\n");
        assert!(
            ProfileFileService::journal_path(
                &root,
                JournalLocation::StatePromoting,
                prepared.operation_id(),
            )
            .exists()
        );

        service.complete(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "new: true\n");
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn file_first_promote_and_compensate_restore_backup() {
        let (_temp, service) = service();
        let path = managed("file-first.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 9)
            .unwrap();

        service.promote(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "new: true\n");
        service.compensate(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "old: true\n");
        service.compensate(&prepared).unwrap();
    }

    #[test]
    fn compensation_recovery_keeps_backup_until_journal_retirement() {
        let (_temp, service) = service();
        let path = managed("compensation-recovery.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 3)
            .unwrap();
        let root = service.materialization_root();
        let backup = ProfileFileService::backup_file_path(&root, prepared.operation_id());

        service.promote(&prepared).unwrap();
        ProfileFileService::transition_journal(
            &root,
            prepared.operation_id(),
            JournalLocation::FilePromoted,
            JournalLocation::FileCompensating,
        )
        .unwrap();
        let (_, journal) = service
            .locate_materialization(&root, prepared.operation_id())
            .unwrap()
            .unwrap();
        assert!(
            service
                .restore_backup(
                    &root,
                    prepared.operation_id(),
                    &service.resolve(&path).unwrap(),
                    &journal.hash,
                )
                .unwrap()
        );

        assert_eq!(service.read(&path).unwrap(), "old: true\n");
        assert!(backup.exists());
        assert!(
            ProfileFileService::journal_path(
                &root,
                JournalLocation::FileCompensating,
                prepared.operation_id(),
            )
            .exists()
        );

        service.compensate(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "old: true\n");
        assert!(!backup.exists());
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn materialization_promotes_symlink_without_writing_its_target() {
        let (temp, service) = service();
        let outside = temp.path().join("outside.yaml");
        std::fs::write(&outside, "external: true\n").unwrap();
        let target = ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        let path = managed("external.yaml");
        let prepared = service
            .prepare_state_first(
                &path,
                MaterializationResource::Symlink {
                    target: target.clone(),
                },
                2,
            )
            .unwrap();

        if let Err(error) = service.promote(&prepared) {
            eprintln!("file symlink unsupported in this environment, skipping: {error:#}");
            return;
        }
        service.complete(&prepared).unwrap();
        assert_eq!(service.read(&path).unwrap(), "external: true\n");
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "external: true\n"
        );
    }

    #[test]
    fn reconcile_completes_committed_state_first_and_restores_uncommitted_file_first() {
        let (_temp, service) = service();
        let add_path = managed("recover-add.yaml");
        let add = service
            .prepare_state_first(&add_path, file_resource("added: true\n"), 5)
            .unwrap();
        let report = service
            .reconcile(&profiles_with_path_at_revision(&add_path, 5))
            .unwrap();
        assert_eq!(report.promoted, 1);
        assert_eq!(report.completed, 1);
        assert!(report.degradations.is_empty());
        assert_eq!(service.read(&add_path).unwrap(), "added: true\n");
        assert!(
            service
                .locate_materialization(&service.materialization_root(), add.operation_id())
                .unwrap()
                .is_none()
        );

        let refresh_path = managed("recover-refresh.yaml");
        service.write_atomic(&refresh_path, "old: true\n").unwrap();
        let refresh = service
            .prepare_file_first(&refresh_path, file_resource("new: true\n"), 8)
            .unwrap();
        service.promote(&refresh).unwrap();
        let report = service
            .reconcile(&profiles_with_path_at_revision(&refresh_path, 7))
            .unwrap();
        assert_eq!(report.compensated, 1);
        assert!(report.degradations.is_empty());
        assert_eq!(service.read(&refresh_path).unwrap(), "old: true\n");
    }

    #[test]
    fn reconcile_completes_committed_file_first_with_an_older_phase_journal() {
        let (_temp, service) = service();
        let path = managed("recover-promoted-file.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let profiles = profiles_with_path_at_revision(&path, 8);
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), profiles.revision())
            .unwrap();
        service.promote(&prepared).unwrap();
        let root = service.materialization_root();
        let promoted = ProfileFileService::journal_path(
            &root,
            JournalLocation::FilePromoted,
            prepared.operation_id(),
        );
        let prepared_journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::FilePrepared,
            prepared.operation_id(),
        );
        std::fs::rename(promoted, &prepared_journal).unwrap();

        let report = service.reconcile(&profiles).unwrap();

        assert_eq!(report.completed, 1);
        assert_eq!(service.read(&path).unwrap(), "new: true\n");
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_isolates_malformed_and_temporary_journal_entries() {
        let (_temp, service) = service();
        let path = managed("recover-through-malformed.yaml");
        let prepared = service
            .prepare_state_first(&path, file_resource("added: true\n"), 5)
            .unwrap();
        let root = service.materialization_root();
        let directory = root.join(JournalLocation::StatePrepared.directory());
        std::fs::write(directory.join("abcdefghijklmnop.yaml"), "not: [valid").unwrap();
        std::fs::write(directory.join("interrupted-write.tmp"), "temporary").unwrap();

        let report = service
            .reconcile(&profiles_with_path_at_revision(&path, 5))
            .unwrap();

        assert_eq!(report.promoted, 1);
        assert_eq!(report.completed, 1);
        assert_eq!(service.read(&path).unwrap(), "added: true\n");
        assert!(
            report
                .degradations
                .iter()
                .any(|degradation| degradation.code
                    == ProfileDegradationCode::MaterializationDeferred)
        );
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_sweeps_artifacts_left_before_journal_creation() {
        let (_temp, service) = service();
        let path = managed("orphaned-artifacts.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 3)
            .unwrap();
        let root = service.materialization_root();
        let stage = ProfileFileService::stage_file_path(&root, prepared.operation_id());
        let backup = ProfileFileService::backup_file_path(&root, prepared.operation_id());
        let journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::FilePrepared,
            prepared.operation_id(),
        );
        std::fs::remove_file(journal).unwrap();

        let report = service.reconcile(&Profiles::default()).unwrap();

        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert!(!stage.exists());
        assert!(!backup.exists());
        assert_eq!(service.read(&path).unwrap(), "old: true\n");
    }

    #[test]
    fn duplicate_adjacent_materialization_journals_converge() {
        let (_temp, service) = service();
        let path = managed("duplicate-journal.yaml");
        let prepared = service
            .prepare_state_first(&path, file_resource("new: true\n"), 2)
            .unwrap();
        let root = service.materialization_root();
        let prepared_journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::StatePrepared,
            prepared.operation_id(),
        );
        let promoting_journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::StatePromoting,
            prepared.operation_id(),
        );
        std::fs::copy(&prepared_journal, &promoting_journal).unwrap();

        let (location, _) = service
            .locate_materialization(&root, prepared.operation_id())
            .unwrap()
            .unwrap();

        assert_eq!(location, JournalLocation::StatePromoting);
        assert!(!prepared_journal.exists());
        assert!(promoting_journal.exists());
        service.promote(&prepared).unwrap();
        service.complete(&prepared).unwrap();
    }

    #[test]
    fn reconcile_recovers_active_state_first_after_global_revision_advances() {
        let (_temp, service) = service();
        let path = managed("advanced-revision.yaml");
        // Superseded state-first journal: newer profiles revision remains active,
        // target still holds the pre-promote/backup bytes, staged forward content
        // must not be applied.
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_state_first(&path, file_resource("added: true\n"), 5)
            .unwrap();

        let report = service
            .reconcile(&profiles_with_path_at_revision(&path, 9))
            .unwrap();

        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert_eq!(report.compensated, 1);
        assert_eq!(report.promoted, 0);
        assert_eq!(report.completed, 0);
        assert_eq!(service.read(&path).unwrap(), "old: true\n");
        assert!(
            service
                .locate_materialization(&service.materialization_root(), prepared.operation_id(),)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn compensation_fence_keeps_diverged_target_backup_and_journal() {
        let (_temp, service) = service();
        let path = managed("compensation-fence.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 3)
            .unwrap();
        let root = service.materialization_root();
        let backup = ProfileFileService::backup_file_path(&root, prepared.operation_id());

        service.promote(&prepared).unwrap();
        service.write_atomic(&path, "foreign: true\n").unwrap();
        let error = service.compensate(&prepared).unwrap_err();

        assert!(format!("{error:#}").contains("fenced"));
        assert_eq!(service.read(&path).unwrap(), "foreign: true\n");
        assert!(backup.exists());
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn compensation_fence_preserves_an_externally_deleted_target() {
        let (_temp, service) = service();
        let path = managed("compensation-delete-fence.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 3)
            .unwrap();
        let root = service.materialization_root();
        let backup = ProfileFileService::backup_file_path(&root, prepared.operation_id());

        service.promote(&prepared).unwrap();
        service.remove(&path).unwrap();
        let error = service.compensate(&prepared).unwrap_err();

        assert!(format!("{error:#}").contains("fenced"));
        assert!(std::fs::symlink_metadata(service.resolve(&path).unwrap()).is_err());
        assert!(backup.exists());
        assert!(
            service
                .locate_materialization(&root, prepared.operation_id())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reconcile_completes_state_promoting_and_file_promoted_after_crash_before_complete() {
        let (_temp, service) = service();
        let root = service.materialization_root();

        let state_path = managed("crash-state-promoting.yaml");
        let state_profiles = profiles_with_path_at_revision(&state_path, 5);
        let state = service
            .prepare_state_first(
                &state_path,
                file_resource("state: new\n"),
                state_profiles.revision(),
            )
            .unwrap();
        service.promote(&state).unwrap();
        assert!(
            ProfileFileService::journal_path(
                &root,
                JournalLocation::StatePromoting,
                state.operation_id(),
            )
            .exists()
        );

        let report = service.reconcile(&state_profiles).unwrap();
        assert_eq!(report.completed, 1);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert_eq!(service.read(&state_path).unwrap(), "state: new\n");
        assert!(
            service
                .locate_materialization(&root, state.operation_id())
                .unwrap()
                .is_none()
        );

        let file_path = managed("crash-file-promoted.yaml");
        service.write_atomic(&file_path, "old: true\n").unwrap();
        let file_profiles = profiles_with_path_at_revision(&file_path, 8);
        let file = service
            .prepare_file_first(
                &file_path,
                file_resource("file: new\n"),
                file_profiles.revision(),
            )
            .unwrap();
        service.promote(&file).unwrap();
        assert!(
            ProfileFileService::journal_path(
                &root,
                JournalLocation::FilePromoted,
                file.operation_id(),
            )
            .exists()
        );

        let report = service.reconcile(&file_profiles).unwrap();
        assert_eq!(report.completed, 1);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert_eq!(service.read(&file_path).unwrap(), "file: new\n");
        assert!(
            service
                .locate_materialization(&root, file.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_compensates_uncommitted_file_prepared() {
        let (_temp, service) = service();
        let path = managed("file-prepared-compensate.yaml");
        service.write_atomic(&path, "old: true\n").unwrap();
        let prepared = service
            .prepare_file_first(&path, file_resource("new: true\n"), 8)
            .unwrap();

        let report = service
            .reconcile(&profiles_with_path_at_revision(&path, 7))
            .unwrap();

        assert_eq!(report.compensated, 1);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert_eq!(service.read(&path).unwrap(), "old: true\n");
        assert!(
            service
                .locate_materialization(&service.materialization_root(), prepared.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_finishes_compensating_journals() {
        let (_temp, service) = service();
        let root = service.materialization_root();

        let state_path = managed("state-compensating-recover.yaml");
        let state = service
            .prepare_state_first(&state_path, file_resource("state: staged\n"), 2)
            .unwrap();
        ProfileFileService::transition_journal(
            &root,
            state.operation_id(),
            JournalLocation::StatePrepared,
            JournalLocation::StateCompensating,
        )
        .unwrap();

        let file_path = managed("file-compensating-recover.yaml");
        service.write_atomic(&file_path, "old: true\n").unwrap();
        let file = service
            .prepare_file_first(&file_path, file_resource("new: true\n"), 3)
            .unwrap();
        service.promote(&file).unwrap();
        ProfileFileService::transition_journal(
            &root,
            file.operation_id(),
            JournalLocation::FilePromoted,
            JournalLocation::FileCompensating,
        )
        .unwrap();

        let report = service
            .reconcile(&profiles_with_path_at_revision(&file_path, 3))
            .unwrap();

        assert_eq!(report.compensated, 2);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert!(service.read(&state_path).is_err());
        assert_eq!(service.read(&file_path).unwrap(), "old: true\n");
        assert!(
            service
                .locate_materialization(&root, state.operation_id())
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .locate_materialization(&root, file.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_activates_and_completes_pending_cleanup() {
        let (_temp, service) = service();
        let path = managed("pending-cleanup-activate.yaml");
        service.write_atomic(&path, "remove: true\n").unwrap();
        let cleanup = service.prepare_cleanup(&path, 2).unwrap();
        let root = service.materialization_root();
        let pending =
            ProfileFileService::cleanup_path(&root, CleanupPhase::Pending, cleanup.operation_id());
        assert!(pending.exists());

        let report = service.reconcile(&Profiles::default()).unwrap();

        assert_eq!(report.cleanups_completed, 1);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert!(!pending.exists());
        assert!(service.read(&path).is_err());
        assert!(
            ProfileFileService::locate_cleanup(&root, cleanup.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_cancels_reactivated_pending_cleanup_and_fences_ready_hash_reuse() {
        let (_temp, service) = service();
        let root = service.materialization_root();

        let active_path = managed("pending-reactivated.yaml");
        service.write_atomic(&active_path, "keep: true\n").unwrap();
        let pending = service.prepare_cleanup(&active_path, 2).unwrap();
        let report = service
            .reconcile(&profiles_with_path(&active_path))
            .unwrap();
        assert!(
            ProfileFileService::locate_cleanup(&root, pending.operation_id())
                .unwrap()
                .is_none()
        );
        assert_eq!(service.read(&active_path).unwrap(), "keep: true\n");
        assert_eq!(report.cleanups_completed, 0);
        assert_eq!(report.cleanups_fenced, 0);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);

        let reused_path = managed("ready-hash-reuse.yaml");
        service.write_atomic(&reused_path, "old: true\n").unwrap();
        let ready = service.prepare_cleanup(&reused_path, 3).unwrap();
        service.activate_cleanup(&ready).unwrap();
        service
            .write_atomic(&reused_path, "reused: true\n")
            .unwrap();

        let report = service.reconcile(&Profiles::default()).unwrap();
        assert_eq!(report.cleanups_fenced, 1);
        assert_eq!(report.cleanups_completed, 0);
        assert!(report.degradations.is_empty(), "{:?}", report.degradations);
        assert_eq!(service.read(&reused_path).unwrap(), "reused: true\n");
        assert!(
            ProfileFileService::locate_cleanup(&root, ready.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reconcile_isolates_malformed_cleanup_while_valid_cleanup_proceeds() {
        let (_temp, service) = service();
        let path = managed("valid-cleanup-with-malformed.yaml");
        service.write_atomic(&path, "remove: true\n").unwrap();
        let cleanup = service.prepare_cleanup(&path, 2).unwrap();
        let root = service.materialization_root();
        let pending_dir = root.join(CleanupPhase::Pending.directory());
        std::fs::write(pending_dir.join("abcdefghijklmnop.yaml"), "not: [valid").unwrap();
        std::fs::write(pending_dir.join("interrupted-write.tmp"), "temporary").unwrap();

        let report = service.reconcile(&Profiles::default()).unwrap();

        assert_eq!(report.cleanups_completed, 1);
        assert!(service.read(&path).is_err());
        assert!(
            ProfileFileService::locate_cleanup(&root, cleanup.operation_id())
                .unwrap()
                .is_none()
        );
        assert!(
            report.degradations.iter().any(|degradation| {
                degradation.code == ProfileDegradationCode::CleanupDeferred
                    || degradation.code == ProfileDegradationCode::JournalInvalid
            }),
            "{:?}",
            report.degradations
        );
        assert!(pending_dir.join("abcdefghijklmnop.yaml").exists());
    }

    #[test]
    fn reconcile_isolates_mixed_family_journals_while_recovering_valid_ops() {
        let (_temp, service) = service();
        let root = service.materialization_root();

        let good_path = managed("mixed-family-good.yaml");
        let good_profiles = profiles_with_path_at_revision(&good_path, 4);
        let good = service
            .prepare_state_first(
                &good_path,
                file_resource("good: true\n"),
                good_profiles.revision(),
            )
            .unwrap();

        let bad_path = managed("mixed-family-bad.yaml");
        let bad = service
            .prepare_state_first(&bad_path, file_resource("mixed: true\n"), 4)
            .unwrap();
        let state_journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::StatePrepared,
            bad.operation_id(),
        );
        let file_journal = ProfileFileService::journal_path(
            &root,
            JournalLocation::FilePrepared,
            bad.operation_id(),
        );
        std::fs::copy(&state_journal, &file_journal).unwrap();

        let report = service.reconcile(&good_profiles).unwrap();

        assert_eq!(report.completed, 1);
        assert_eq!(service.read(&good_path).unwrap(), "good: true\n");
        assert!(
            service
                .locate_materialization(&root, good.operation_id())
                .unwrap()
                .is_none()
        );
        assert!(
            report
                .degradations
                .iter()
                .any(|degradation| degradation.code
                    == ProfileDegradationCode::MaterializationDeferred),
            "{:?}",
            report.degradations
        );
        assert!(state_journal.exists());
        assert!(file_journal.exists());
        assert!(
            service
                .locate_materialization(&root, bad.operation_id())
                .is_err()
        );
    }

    #[test]
    fn complete_fences_diverged_target_hash() {
        let (_temp, service) = service();
        let path = managed("complete-hash-fence.yaml");
        let prepared = service
            .prepare_state_first(&path, file_resource("new: true\n"), 2)
            .unwrap();
        service.promote(&prepared).unwrap();
        service.write_atomic(&path, "foreign: true\n").unwrap();

        let error = service.complete(&prepared).unwrap_err();
        assert!(format!("{error:#}").contains("hash mismatch"));
        assert_eq!(service.read(&path).unwrap(), "foreign: true\n");
        assert!(
            service
                .locate_materialization(&service.materialization_root(), prepared.operation_id())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn cleanup_cancel_and_already_absent_target_are_idempotent() {
        let (_temp, service) = service();
        let path = managed("cancel-cleanup.yaml");
        service.write_atomic(&path, "x: 1\n").unwrap();
        let cleanup = service.prepare_cleanup(&path, 2).unwrap();
        service.cancel_cleanup(&cleanup).unwrap();
        service.cancel_cleanup(&cleanup).unwrap();

        let missing = managed("already-missing.yaml");
        let cleanup = service.prepare_cleanup(&missing, 3).unwrap();
        service.activate_cleanup(&cleanup).unwrap();
        assert_eq!(
            service
                .retry_cleanup(&cleanup, &Profiles::default())
                .unwrap(),
            CleanupOutcome::AlreadyAbsent
        );
    }

    #[test]
    fn duplicate_adjacent_cleanup_journals_converge() {
        let (_temp, service) = service();
        let path = managed("duplicate-cleanup.yaml");
        service.write_atomic(&path, "remove: true\n").unwrap();
        let cleanup = service.prepare_cleanup(&path, 2).unwrap();
        let root = service.materialization_root();
        let pending =
            ProfileFileService::cleanup_path(&root, CleanupPhase::Pending, cleanup.operation_id());
        let ready =
            ProfileFileService::cleanup_path(&root, CleanupPhase::Ready, cleanup.operation_id());
        std::fs::copy(&pending, &ready).unwrap();

        service.activate_cleanup(&cleanup).unwrap();

        assert!(!pending.exists());
        assert!(ready.exists());
        assert_eq!(
            service
                .retry_cleanup(&cleanup, &Profiles::default())
                .unwrap(),
            CleanupOutcome::Removed
        );
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_retries_after_target_has_moved_to_a_tombstone() {
        let (_temp, service) = service();
        let path = managed("cleanup-tombstone.yaml");
        service.write_atomic(&path, "remove: true\n").unwrap();
        let cleanup = service.prepare_cleanup(&path, 2).unwrap();
        service.activate_cleanup(&cleanup).unwrap();
        let root = service.materialization_root();
        let target = service.resolve(&path).unwrap();
        service
            .remove_cleanup_target(&root, cleanup.operation_id(), &target)
            .unwrap();
        let tombstone = ProfileFileService::cleanup_tombstone_path(&root, cleanup.operation_id());

        assert!(std::fs::symlink_metadata(&target).is_err());
        assert!(tombstone.exists());
        assert_eq!(
            service
                .retry_cleanup(&cleanup, &Profiles::default())
                .unwrap(),
            CleanupOutcome::AlreadyAbsent
        );
        assert!(!tombstone.exists());
        assert!(
            ProfileFileService::locate_cleanup(&root, cleanup.operation_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cleanup_is_idempotent_and_fences_active_or_reused_paths() {
        let (_temp, service) = service();
        let active_path = managed("active-cleanup.yaml");
        service.write_atomic(&active_path, "old: true\n").unwrap();
        let active_job = service.prepare_cleanup(&active_path, 3).unwrap();
        service.activate_cleanup(&active_job).unwrap();
        assert_eq!(
            service
                .retry_cleanup(&active_job, &profiles_with_path(&active_path))
                .unwrap(),
            CleanupOutcome::FencedActivePath
        );
        assert_eq!(service.read(&active_path).unwrap(), "old: true\n");
        assert_eq!(
            service
                .retry_cleanup(&active_job, &Profiles::default())
                .unwrap(),
            CleanupOutcome::AlreadyAbsent
        );

        let reused_path = managed("reused-cleanup.yaml");
        service.write_atomic(&reused_path, "old: true\n").unwrap();
        let reused_job = service.prepare_cleanup(&reused_path, 4).unwrap();
        service.activate_cleanup(&reused_job).unwrap();
        service.write_atomic(&reused_path, "new: true\n").unwrap();
        assert_eq!(
            service
                .retry_cleanup(&reused_job, &Profiles::default())
                .unwrap(),
            CleanupOutcome::FencedHashMismatch
        );
        assert_eq!(service.read(&reused_path).unwrap(), "new: true\n");

        let removed_path = managed("removed-cleanup.yaml");
        service
            .write_atomic(&removed_path, "remove: true\n")
            .unwrap();
        let removed_job = service.prepare_cleanup(&removed_path, 5).unwrap();
        service.activate_cleanup(&removed_job).unwrap();
        assert_eq!(
            service
                .retry_cleanup(&removed_job, &Profiles::default())
                .unwrap(),
            CleanupOutcome::Removed
        );
        assert_eq!(
            service
                .retry_cleanup(&removed_job, &Profiles::default())
                .unwrap(),
            CleanupOutcome::AlreadyAbsent
        );
    }

    #[test]
    fn cleanup_removes_managed_symlink_without_following_target() {
        let (temp, service) = service();
        let outside = temp.path().join("cleanup-target.yaml");
        std::fs::write(&outside, "keep: true\n").unwrap();
        let path = managed("cleanup-link.yaml");
        let target = ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        if service.ensure_symlink(&path, &target).is_err() {
            eprintln!("file symlink unsupported in this environment, skipping");
            return;
        }
        let cleanup = service.prepare_cleanup(&path, 1).unwrap();
        service.activate_cleanup(&cleanup).unwrap();
        assert_eq!(
            service
                .retry_cleanup(&cleanup, &Profiles::default())
                .unwrap(),
            CleanupOutcome::Removed
        );
        assert!(std::fs::symlink_metadata(service.resolve(&path).unwrap()).is_err());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "keep: true\n");
    }

    #[test]
    fn journal_schema_contains_only_fence_fields_and_rejects_extras() {
        let (_temp, service) = service();
        let path = managed("schema.yaml");
        let prepared = service
            .prepare_file_first(&path, file_resource("schema: true\n"), 12)
            .unwrap();
        let journal_path = ProfileFileService::journal_path(
            &service.materialization_root(),
            JournalLocation::FilePrepared,
            prepared.operation_id(),
        );
        let content = std::fs::read_to_string(&journal_path).unwrap();
        assert_eq!(
            ProfileFileService::read_journal(&journal_path, prepared.operation_id())
                .unwrap()
                .revision,
            12
        );
        let value: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
        let mut keys = value
            .as_mapping()
            .unwrap()
            .keys()
            .map(|key| key.as_str().unwrap())
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(keys, ["hash", "managed_path", "operation_id", "revision"]);
        for forbidden in ["phase", "content", "config", "subscription"] {
            assert!(!content.contains(forbidden));
        }

        std::fs::write(&journal_path, format!("{content}extra: true\n")).unwrap();
        assert!(ProfileFileService::read_journal(&journal_path, prepared.operation_id()).is_err());

        let unsafe_id = "abcdefghijklmnop";
        let unsafe_path = ProfileFileService::journal_path(
            &service.materialization_root(),
            JournalLocation::StatePrepared,
            unsafe_id,
        );
        std::fs::write(
            &unsafe_path,
            format!(
                "managed_path: ../escape.yaml\noperation_id: {unsafe_id}\nrevision: 1\nhash: {}\n",
                "0".repeat(64)
            ),
        )
        .unwrap();
        assert!(ProfileFileService::read_journal(&unsafe_path, unsafe_id).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn resolve_rejects_windows_private_root_aliases() {
        let (_temp, service) = service();
        for name in [
            ".PROFILE-MATERIALIZATION-V1",
            ".profile-materialization-v1.",
            ".Profile-Materialization-V1 ",
        ] {
            let error = service
                .resolve(&managed(&format!("{name}/blocked.yaml")))
                .unwrap_err();
            assert!(format!("{error:#}").contains("reserved private storage"));
        }
    }

    #[test]
    fn private_root_and_private_artifact_symlinks_are_rejected() {
        let (temp, service) = service();
        let profiles_dir = service.paths.app_profiles_dir();
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let outside_dir = temp.path().join("outside-private");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let private_root = profiles_dir.join(MATERIALIZATION_ROOT);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside_dir, &private_root);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside_dir, &private_root);
        if made.is_err() {
            eprintln!("directory symlink unsupported in this environment, skipping");
            return;
        }
        let error = service
            .prepare_state_first(&managed("blocked.yaml"), file_resource("x: 1\n"), 1)
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("symlink") || message.contains("reparse"));
        assert!(outside_dir.read_dir().unwrap().next().is_none());
    }

    #[test]
    fn staged_backup_journal_and_cleanup_symlinks_fail_without_following() {
        let (temp, service) = service();
        let outside = temp.path().join("outside-artifact");
        std::fs::write(&outside, "outside: unchanged\n").unwrap();

        let stage_path = managed("stage-attack.yaml");
        let staged = service
            .prepare_state_first(&stage_path, file_resource("new: true\n"), 1)
            .unwrap();
        let stage = ProfileFileService::stage_file_path(
            &service.materialization_root(),
            staged.operation_id(),
        );
        std::fs::remove_file(&stage).unwrap();
        if create_file_symlink(&outside, &stage).is_err() {
            eprintln!("file symlink unsupported in this environment, skipping");
            return;
        }
        assert!(service.promote(&staged).is_err());
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "outside: unchanged\n"
        );

        let backup_path = managed("backup-attack.yaml");
        service.write_atomic(&backup_path, "old: true\n").unwrap();
        let backup = service
            .prepare_file_first(&backup_path, file_resource("new: true\n"), 2)
            .unwrap();
        let backup_artifact = ProfileFileService::backup_file_path(
            &service.materialization_root(),
            backup.operation_id(),
        );
        std::fs::remove_file(&backup_artifact).unwrap();
        create_file_symlink(&outside, &backup_artifact).unwrap();
        service.promote(&backup).unwrap();
        assert!(service.compensate(&backup).is_err());
        assert_eq!(service.read(&backup_path).unwrap(), "new: true\n");
        assert!(
            service
                .locate_materialization(&service.materialization_root(), backup.operation_id())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "outside: unchanged\n"
        );

        let journal_path = managed("journal-attack.yaml");
        let journal = service
            .prepare_state_first(&journal_path, file_resource("x: 1\n"), 3)
            .unwrap();
        let journal_artifact = ProfileFileService::journal_path(
            &service.materialization_root(),
            JournalLocation::StatePrepared,
            journal.operation_id(),
        );
        std::fs::remove_file(&journal_artifact).unwrap();
        create_file_symlink(&outside, &journal_artifact).unwrap();
        assert!(service.promote(&journal).is_err());

        let cleanup_path = managed("cleanup-attack.yaml");
        service.write_atomic(&cleanup_path, "x: 1\n").unwrap();
        let cleanup = service.prepare_cleanup(&cleanup_path, 4).unwrap();
        let cleanup_artifact = ProfileFileService::cleanup_path(
            &service.materialization_root(),
            CleanupPhase::Pending,
            cleanup.operation_id(),
        );
        std::fs::remove_file(&cleanup_artifact).unwrap();
        create_file_symlink(&outside, &cleanup_artifact).unwrap();
        assert!(service.activate_cleanup(&cleanup).is_err());
        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "outside: unchanged\n"
        );
    }

    #[test]
    fn write_atomic_rejects_symlink_leaf_without_touching_target() {
        let (temp, service) = service();
        let outside = temp.path().join("write-target.yaml");
        std::fs::write(&outside, "old: true\n").unwrap();
        let path = managed("write-link.yaml");
        service.ensure_profiles_root().unwrap();
        let full = service.resolve(&path).unwrap();
        if create_file_symlink(&outside, &full).is_err() {
            eprintln!("file symlink unsupported in this environment, skipping");
            return;
        }
        assert!(service.write_atomic(&path, "new: true\n").is_err());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "old: true\n");
    }

    #[test]
    fn ensure_not_symlink_rejects_links_and_accepts_files() {
        let (_temp, service) = service();
        let path = managed("real.yaml");
        service.ensure_not_symlink(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.ensure_not_symlink(&path).unwrap();

        let link = managed("link.yaml");
        let target_file = service.resolve(&path).unwrap();
        let link_file = service.resolve(&link).unwrap();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target_file, &link_file);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target_file, &link_file);
        if made.is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert!(service.ensure_not_symlink(&link).is_err());
    }

    #[test]
    fn ensure_symlink_creates_and_repairs() {
        let (temp, service) = service();
        let outside = temp.path().join("outside.yaml");
        std::fs::write(&outside, "external: true\n").unwrap();
        let target =
            nyanpasu_config::profile::ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        let link = managed("ext.yaml");
        if service.ensure_symlink(&link, &target).is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert_eq!(service.read(&link).unwrap(), "external: true\n");
        service.ensure_symlink(&link, &target).unwrap();

        let occupied = managed("occupied.yaml");
        service.write_atomic(&occupied, "x: 1\n").unwrap();
        assert!(service.ensure_symlink(&occupied, &target).is_err());
    }

    #[test]
    fn ensure_symlink_keeps_existing_link_when_canonical_targets_match() {
        let (temp, service) = service();
        let outside = temp.path().join("outside.yaml");
        std::fs::write(&outside, "external: true\n").unwrap();
        let target =
            nyanpasu_config::profile::ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        let link = managed("ext.yaml");
        if service.ensure_symlink(&link, &target).is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }

        let link_path = service.resolve(&link).unwrap();
        let before = std::fs::read_link(&link_path).unwrap();
        let canonical_target = std::fs::canonicalize(&outside).unwrap();
        let equivalent_target =
            nyanpasu_config::profile::ExternalProfilePath::new(canonical_target.to_string_lossy())
                .unwrap();

        service.ensure_symlink(&link, &equivalent_target).unwrap();
        assert_eq!(std::fs::read_link(&link_path).unwrap(), before);
    }

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }

    #[test]
    fn content_disposition_filename_parsing_matches_legacy_variants() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; Filename=\"a.yaml\""),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("a.yaml")
        );

        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static(
                "attachment; filename*=UTF-8'en'my%20cfg.yaml",
            ),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("my cfg.yaml")
        );

        headers.insert(
            "profile-title",
            reqwest::header::HeaderValue::from_static("   "),
        );
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; filename=fallback.yaml"),
        );
        assert_eq!(
            parse_profile_title(&headers).as_deref(),
            Some("fallback.yaml")
        );
    }

    #[test]
    fn subscription_expire_ignores_absurd_overflow_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "subscription-userinfo",
            reqwest::header::HeaderValue::from_static(
                "upload=1; download=2; total=3; expire=18446744073709551615",
            ),
        );
        let parsed = parse_subscription_userinfo(&headers);
        assert_eq!(parsed.upload, Some(1));
        assert!(parsed.expire.is_none());
    }

    #[test]
    fn suggested_update_interval_parses_hours_and_ignores_invalid_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_suggested_update_interval(&headers), None);

        headers.insert(
            "profile-update-interval",
            reqwest::header::HeaderValue::from_static("2"),
        );
        assert_eq!(parse_suggested_update_interval(&headers), Some(120));

        for invalid in ["not-a-number", "0", "18446744073709551615"] {
            headers.insert(
                "profile-update-interval",
                reqwest::header::HeaderValue::from_str(invalid).unwrap(),
            );
            assert_eq!(parse_suggested_update_interval(&headers), None);
        }
    }

    #[tokio::test]
    async fn fetch_parses_userinfo_and_title_headers() {
        let router = Router::new().route(
            "/",
            get(|| async {
                let mut headers = AxumHeaderMap::new();
                headers.insert(
                    "subscription-userinfo",
                    "upload=1; download=2; total=3; expire=0".parse().unwrap(),
                );
                headers.insert("profile-title", "My Sub".parse().unwrap());
                headers.insert("profile-update-interval", "6".parse().unwrap());
                (headers, "proxies: []\n")
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "proxies: []\n");
        assert_eq!(fetched.filename.as_deref(), Some("My Sub"));
        assert_eq!(fetched.subscription.upload, Some(1));
        assert_eq!(fetched.subscription.download, Some(2));
        assert_eq!(fetched.subscription.total, Some(3));
        assert!(fetched.subscription.expire.is_none());
        assert_eq!(fetched.suggested_update_interval_minutes, Some(360));
    }

    #[tokio::test]
    async fn fetch_sends_default_user_agent_and_hwid() {
        let seen = Arc::new(Mutex::new(None::<(String, bool)>));
        let seen_request = Arc::clone(&seen);
        let router = Router::new().route(
            "/",
            get(move |headers: AxumHeaderMap| {
                let seen_request = Arc::clone(&seen_request);
                async move {
                    let ua = headers
                        .get(header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    let has_hwid = headers
                        .get("x-hwid")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|value| !value.trim().is_empty());
                    *seen_request.lock().unwrap() = Some((ua, has_hwid));
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();

        let (ua, has_hwid) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            ua,
            format!("clash-nyanpasu/v{}", crate::utils::dirs::APP_VERSION)
        );
        assert!(has_hwid);
    }

    #[tokio::test]
    async fn fetch_falls_back_to_direct_when_self_proxy_port_is_unavailable() {
        let hits = Arc::new(AtomicUsize::new(0));
        let source_hits = Arc::new(AtomicUsize::new(0));
        let request_hits = Arc::clone(&hits);
        let router = Router::new().route(
            "/",
            get(move || {
                let request_hits = Arc::clone(&request_hits);
                async move {
                    request_hits.fetch_add(1, Ordering::SeqCst);
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(true, false))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_continues_proxy_chain_when_self_proxy_port_is_unavailable() {
        let source_hits = Arc::new(AtomicUsize::new(0));
        let router = Router::new().route("/", get(|| async { "ok: true\n" }));
        let (url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(true, true))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_retries_transient_errors_but_not_auth_failures() {
        let hits = Arc::new(AtomicUsize::new(0));
        let flaky_hits = Arc::clone(&hits);
        let flaky = Router::new().route(
            "/",
            get(move || {
                let flaky_hits = Arc::clone(&flaky_hits);
                async move {
                    if flaky_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    } else {
                        "ok: true\n".into_response()
                    }
                }
            }),
        );
        let (url, _server) = serve(flaky).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "ok: true\n");
        assert!(hits.load(Ordering::SeqCst) >= 2);

        for status in [
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
            StatusCode::NOT_FOUND,
        ] {
            let hits = Arc::new(AtomicUsize::new(0));
            let status_hits = Arc::clone(&hits);
            let router = Router::new().route(
                "/",
                get(move || {
                    let status_hits = Arc::clone(&status_hits);
                    async move {
                        status_hits.fetch_add(1, Ordering::SeqCst);
                        status
                    }
                }),
            );
            let (url, _server) = serve(router).await;
            assert!(
                service
                    .fetch(&Url::parse(&url).unwrap(), &options_direct())
                    .await
                    .is_err()
            );
            assert_eq!(hits.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn fetch_timeout_is_managed_internally() {
        let slow = Router::new().route(
            "/",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "never"
            }),
        );
        let (url, _server) = serve(slow).await;
        let (_temp, service) = service();
        let service = service.with_http_timeout(Duration::from_millis(300));
        let started = Instant::now();
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
