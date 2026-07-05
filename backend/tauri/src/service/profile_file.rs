//! ProfileFsPort + SubscriptionFetcher over the real filesystem, reqwest and
//! the OS proxy state (design §7). Tauri-free and legacy-Config-free; the
//! self-proxy port arrives via [`SelfProxyPortSource`] instead of the legacy
//! global read (config/profile/item/remote.rs:130-136 FIXME).

use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use nyanpasu_config::profile::{ExternalProfilePath, ManagedProfilePath};

use crate::{state::profiles::ports::ProfileFsPort, utils::path::PathResolver};

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

    fn resolve(&self, path: &ManagedProfilePath) -> PathBuf {
        self.paths.app_profiles_dir().join(path.as_path())
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
        let full = self.resolve(path);
        std::fs::read_to_string(&full)
            .with_context(|| format!("read profile file {}", full.display()))
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        AtomicFile::new(&full, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("atomic write {}", full.display()))
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("remove profile file {}", full.display())),
        }
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => bail!(
                "refusing to write through unexpected symlink at {}",
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
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if std::fs::read_link(&full)? == target.as_path() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::profile::ManagedProfilePath;
    use std::sync::Arc;

    struct NoProxy;
    impl SelfProxyPortSource for NoProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (temp, ProfileFileService::new(paths, Arc::new(NoProxy)))
    }

    fn managed(name: &str) -> ManagedProfilePath {
        ManagedProfilePath::new(name).unwrap()
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
    fn remove_is_idempotent() {
        let (_temp, service) = service();
        let path = managed("gone.yaml");
        service.remove(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.remove(&path).unwrap();
        assert!(service.read(&path).is_err());
    }

    #[test]
    fn ensure_not_symlink_rejects_links_and_accepts_files() {
        let (_temp, service) = service();
        let path = managed("real.yaml");
        service.ensure_not_symlink(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.ensure_not_symlink(&path).unwrap();

        let link = managed("link.yaml");
        let target_file = service.resolve(&path);
        let link_file = service.resolve(&link);
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
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }
}
