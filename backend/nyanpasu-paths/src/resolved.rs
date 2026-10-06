use crate::{CLASH_CFG_GUARD_OVERRIDES, NYANPASU_CONFIG, PROFILE_YAML, PathResolver, STORAGE_DB};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Fixed directory values for application owners. This is a snapshot produced by
/// the resolver, not a second platform-discovery service.
#[derive(Debug, Clone)]
pub struct ResolvedPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    resources_dir: Option<PathBuf>,
    resolver: PathResolver,
}

impl ResolvedPaths {
    pub(crate) fn new(
        config_dir: PathBuf,
        data_dir: PathBuf,
        resources_dir: Option<PathBuf>,
        resolver: PathResolver,
    ) -> Self {
        Self {
            config_dir,
            data_dir,
            resources_dir,
            resolver,
        }
    }
    pub fn with_base_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let resolver = PathResolver::with_base_dirs(config_dir.clone(), data_dir.clone());
        Self::new(config_dir, data_dir, None, resolver)
    }
    pub fn with_install_dir(mut self, install_dir: PathBuf) -> Self {
        self.resolver = self.resolver.with_install_dir(install_dir);
        self
    }
    pub fn resolver(&self) -> &PathResolver {
        &self.resolver
    }
    pub fn app_config_dir(&self) -> &Path {
        &self.config_dir
    }
    pub fn app_data_dir(&self) -> &Path {
        &self.data_dir
    }
    pub fn app_install_dir(&self) -> Result<PathBuf> {
        self.resolver.install_dir()
    }
    pub fn service_binary_path(&self) -> Result<PathBuf> {
        Ok(self
            .app_install_dir()?
            .join(format!("nyanpasu-service{}", std::env::consts::EXE_SUFFIX)))
    }
    pub fn app_resources_dir(&self) -> Result<&Path> {
        self.resources_dir
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the bundled resources dir is unknown"))
    }
    pub fn app_profiles_dir(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }
    pub fn profiles_path(&self) -> PathBuf {
        self.config_dir.join(PROFILE_YAML)
    }
    pub fn nyanpasu_config_path(&self) -> PathBuf {
        self.config_dir.join(NYANPASU_CONFIG)
    }
    pub fn application_config_path(&self) -> PathBuf {
        self.config_dir.join("application.yaml")
    }
    pub fn session_state_path(&self) -> PathBuf {
        self.config_dir.join("session-state.yaml")
    }
    pub fn clash_config_path(&self) -> PathBuf {
        self.config_dir.join("clash-config.yaml")
    }
    pub fn clash_guard_overrides_path(&self) -> PathBuf {
        self.config_dir.join(CLASH_CFG_GUARD_OVERRIDES)
    }
    pub fn storage_path(&self) -> PathBuf {
        self.data_dir.join(STORAGE_DB)
    }
    pub fn jobs_path(&self) -> PathBuf {
        self.data_dir.join("jobs.redb")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }
    pub fn clash_pid_path(&self) -> PathBuf {
        self.data_dir.join("clash.pid")
    }
    pub fn app_logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }
    pub fn scripts_dir(&self) -> PathBuf {
        self.data_dir.join("scripts")
    }
    pub fn data_or_sidecar_path(&self, binary_name: impl AsRef<str>) -> Result<PathBuf> {
        self.resolver.data_or_sidecar_path(binary_name)
    }
    pub fn single_instance_placeholder(&self) -> Result<String> {
        self.resolver.single_instance_placeholder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolver() -> ResolvedPaths {
        ResolvedPaths::with_base_dirs(PathBuf::from("/cfg"), PathBuf::from("/data"))
    }
    #[test]
    fn data_derived_paths_join_data_dir() {
        let r = resolver();
        assert_eq!(r.storage_path(), Path::new("/data").join(STORAGE_DB));
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
        let dir = tempfile::tempdir().unwrap();
        let r = resolver().with_install_dir(dir.path().to_owned());
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
