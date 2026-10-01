//! Centralized filesystem path resolution.
//!
//! [`PathResolver`] mirrors every path-producing helper currently exposed as a
//! free function in [`crate::utils::dirs`], but bundles them behind a single,
//! injectable value. The two base directories (config / data) are resolved once
//! at construction so leaf paths are pure, infallible joins; the harder
//! platform-specific resolution (portable flag, Windows registry override,
//! XDG/Known-Folder lookup) is still delegated to [`dirs`] so there is a single
//! source of truth for it.
//!
//! The migration subsystem and application composition root consume this module
//! for now. The existing `dirs::*` call sites are intentionally left untouched
//! and will be migrated to `PathResolver` in a follow-up change.
//!
//! Known remaining coupling, to be removed by the follow-up that makes
//! `PathResolver` truly own path resolution: [`PathResolver::from_env`] and the
//! delegating helpers ([`PathResolver::app_install_dir`],
//! [`PathResolver::data_or_sidecar_path`],
//! [`PathResolver::single_instance_placeholder`]) still call into `dirs::*`
//! rather than resolving paths themselves.

use crate::utils::dirs;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// The service binary's name, without the platform's executable suffix.
const SERVICE_BINARY: &str = "nyanpasu-service";

/// Resolves application paths from a fixed pair of base directories.
///
/// Construct with [`PathResolver::from_env`] for the real, platform-resolved
/// directories, or [`PathResolver::with_base_dirs`] to inject explicit roots
/// (portable layouts, tests, migration dry-runs).
#[derive(Debug, Clone)]
pub struct PathResolver {
    config_dir: PathBuf,
    data_dir: PathBuf,
    /// The bundle's `resources` dir. Only Tauri knows where the bundle is, so
    /// the composition root resolves it and hands it in. `None` when that
    /// failed, and for resolvers built from explicit roots.
    resources_dir: Option<PathBuf>,
}

#[allow(dead_code)]
impl PathResolver {
    /// Resolve the base directories from the current environment.
    ///
    /// Delegates to [`dirs::app_config_dir`] / [`dirs::app_data_dir`], which
    /// ensure the directories exist and honor the portable flag and Windows
    /// registry override.
    pub fn from_env(resources_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            config_dir: dirs::app_config_dir()?,
            data_dir: dirs::app_data_dir()?,
            resources_dir,
        })
    }

    /// Build a resolver from explicit base directories without touching the
    /// environment. Does not create the directories.
    pub fn with_base_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
            resources_dir: None,
        }
    }

    // -- base directories ---------------------------------------------------

    /// The app config dir (settings, profiles, lockfiles).
    pub fn app_config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The app data dir (storage, logs, cache, runtime files).
    pub fn app_data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The directory the executable lives in. Resolved on demand from the
    /// current executable, independent of the base dirs.
    pub fn app_install_dir(&self) -> Result<PathBuf> {
        dirs::app_install_dir()
    }

    /// The `nyanpasu-service` binary, shipped next to the executable.
    pub fn service_binary_path(&self) -> Result<PathBuf> {
        Ok(self
            .app_install_dir()?
            .join(format!("{SERVICE_BINARY}{}", std::env::consts::EXE_SUFFIX)))
    }

    /// The bundled `resources` dir, as the composition root resolved it.
    pub fn app_resources_dir(&self) -> Result<&Path> {
        self.resources_dir
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the bundled resources dir is unknown"))
    }

    // -- config-derived paths ----------------------------------------------

    /// Directory holding individual profile files.
    pub fn app_profiles_dir(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }

    /// `profiles.yaml` index file.
    pub fn profiles_path(&self) -> PathBuf {
        self.config_dir.join(dirs::PROFILE_YAML)
    }

    /// `nyanpasu-config.yaml` main config file.
    pub fn nyanpasu_config_path(&self) -> PathBuf {
        self.config_dir.join(dirs::NYANPASU_CONFIG)
    }

    /// Typed application config path used by the ApplicationActor.
    pub fn application_config_path(&self) -> PathBuf {
        self.config_dir.join("application.yaml")
    }

    /// Typed session/window state path used by the SessionStateActor.
    pub fn session_state_path(&self) -> PathBuf {
        self.config_dir.join("session-state.yaml")
    }

    /// Typed persistent Clash config path used by the ClashConfigActor.
    pub fn clash_config_path(&self) -> PathBuf {
        self.config_dir.join("clash-config.yaml")
    }

    /// `clash-guard-overrides.yaml` file.
    pub fn clash_guard_overrides_path(&self) -> PathBuf {
        self.config_dir.join(dirs::CLASH_CFG_GUARD_OVERRIDES)
    }

    /// Tray icon PNG for the given mode, e.g. `icons/<mode>.png`.
    pub fn tray_icons_path(&self, mode: &str) -> PathBuf {
        self.config_dir.join("icons").join(format!("{mode}.png"))
    }

    // -- data-derived paths -------------------------------------------------

    /// `storage.db` key-value store file.
    pub fn storage_path(&self) -> PathBuf {
        self.data_dir.join(dirs::STORAGE_DB)
    }

    /// Durable job runs and their scoped journals.
    pub fn jobs_path(&self) -> PathBuf {
        self.data_dir.join("jobs.redb")
    }

    /// Config backups, one directory per backup.
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// `clash.pid` runtime file.
    pub fn clash_pid_path(&self) -> PathBuf {
        self.data_dir.join("clash.pid")
    }

    /// Logs directory.
    pub fn app_logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Cache directory (safe to clean up).
    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }

    /// Where the JavaScript runner writes the modules it runs.
    pub fn scripts_dir(&self) -> PathBuf {
        self.data_dir.join("scripts")
    }

    // -- delegating helpers -------------------------------------------------

    /// Resolve the data-dir or sidecar-dir path for a bundled binary.
    pub fn data_or_sidecar_path(&self, binary_name: impl AsRef<str>) -> Result<PathBuf> {
        dirs::get_data_or_sidecar_path(binary_name)
    }

    /// Per-user single-instance placeholder/lock identifier.
    pub fn single_instance_placeholder(&self) -> Result<String> {
        dirs::get_single_instance_placeholder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> PathResolver {
        PathResolver::with_base_dirs(PathBuf::from("/cfg"), PathBuf::from("/data"))
    }

    #[test]
    fn config_derived_paths_join_config_dir() {
        let r = resolver();
        assert_eq!(
            r.profiles_path(),
            Path::new("/cfg").join(dirs::PROFILE_YAML)
        );
        assert_eq!(
            r.nyanpasu_config_path(),
            Path::new("/cfg").join(dirs::NYANPASU_CONFIG)
        );
        assert_eq!(
            r.clash_guard_overrides_path(),
            Path::new("/cfg").join(dirs::CLASH_CFG_GUARD_OVERRIDES)
        );
        assert_eq!(
            r.application_config_path(),
            Path::new("/cfg").join("application.yaml")
        );
        assert_eq!(
            r.session_state_path(),
            Path::new("/cfg").join("session-state.yaml")
        );
        assert_eq!(
            r.clash_config_path(),
            Path::new("/cfg").join("clash-config.yaml")
        );
        assert_eq!(r.app_profiles_dir(), Path::new("/cfg").join("profiles"));
        assert_eq!(
            r.tray_icons_path("light"),
            Path::new("/cfg").join("icons").join("light.png")
        );
    }

    #[test]
    fn data_derived_paths_join_data_dir() {
        let r = resolver();
        assert_eq!(r.storage_path(), Path::new("/data").join(dirs::STORAGE_DB));
        assert_eq!(r.backups_dir(), Path::new("/data").join("backups"));
        assert_eq!(r.clash_pid_path(), Path::new("/data").join("clash.pid"));
        assert_eq!(r.app_logs_dir(), Path::new("/data").join("logs"));
        assert_eq!(r.cache_dir(), Path::new("/data").join("cache"));
        assert_eq!(r.scripts_dir(), Path::new("/data").join("scripts"));
    }

    #[test]
    fn base_dirs_are_exposed_verbatim() {
        let r = resolver();
        assert_eq!(r.app_config_dir(), Path::new("/cfg"));
        assert_eq!(r.app_data_dir(), Path::new("/data"));
    }

    #[test]
    fn explicit_roots_know_no_bundle_resources() {
        assert!(resolver().app_resources_dir().is_err());
    }

    #[test]
    fn the_service_binary_sits_next_to_the_executable() {
        let r = resolver();
        let binary = r.service_binary_path().unwrap();
        assert_eq!(
            binary.parent(),
            Some(r.app_install_dir().unwrap().as_path())
        );
        assert_eq!(
            binary.file_name().unwrap().to_str(),
            Some(format!("nyanpasu-service{}", std::env::consts::EXE_SUFFIX).as_str())
        );
    }
}
