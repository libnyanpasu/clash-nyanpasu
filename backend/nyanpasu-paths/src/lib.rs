//! Application directory and binary resolution, independent of any frontend.
//!
//! A resolver owns host inputs; platform policy is private to this crate. Creating
//! it does not resolve or create directories. `ResolvedPaths` is the fixed-root
//! snapshot used by application owners after startup has resolved those roots.

use anyhow::Result;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

mod resolved;
pub use resolved::ResolvedPaths;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(not(windows))]
#[path = "unix.rs"]
mod platform;

pub const CLASH_CFG_GUARD_OVERRIDES: &str = "clash-guard-overrides.yaml";
pub const NYANPASU_CONFIG: &str = "nyanpasu-config.yaml";
pub const PROFILE_YAML: &str = "profiles.yaml";
pub const STORAGE_DB: &str = "storage.db";

#[derive(Debug)]
pub struct HostInputs {
    pub app_name: String,
    pub executable: std::result::Result<PathBuf, Arc<std::io::Error>>,
    pub portable: bool,
    pub development_binaries: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PathResolver {
    host: Option<Arc<HostInputs>>,
    directory_name: Option<String>,
    roots: Option<(PathBuf, PathBuf)>,
    installation: Option<PathBuf>,
}

impl PathResolver {
    pub fn new(inputs: HostInputs) -> Self {
        let directory_name = Some(platform::directory_name(&inputs.app_name));
        Self {
            host: Some(Arc::new(inputs)),
            directory_name,
            roots: None,
            installation: None,
        }
    }

    pub fn with_base_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            host: None,
            directory_name: None,
            roots: Some((config_dir, data_dir)),
            installation: None,
        }
    }

    pub fn with_roots(mut self, config_dir: PathBuf, data_dir: PathBuf) -> Self {
        self.roots = Some((config_dir, data_dir));
        self
    }

    pub fn with_install_dir(mut self, dir: PathBuf) -> Self {
        self.installation = Some(dir);
        self
    }

    pub fn directory_name(&self) -> Result<&str> {
        self.directory_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the application identity is unknown"))
    }

    pub fn config_dir(&self) -> Result<PathBuf> {
        let dir = match &self.roots {
            Some((config, _)) => config.clone(),
            None => self.platform_config_dir()?,
        };
        create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn resolve_data_dir(&self) -> Result<PathBuf> {
        match &self.roots {
            Some((_, data)) => Ok(data.clone()),
            None => self.platform_data_dir(),
        }
    }

    pub fn data_dir(&self) -> Result<PathBuf> {
        let dir = self.resolve_data_dir()?;
        create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn install_dir(&self) -> Result<PathBuf> {
        if let Some(dir) = &self.installation {
            return Ok(dir.clone());
        }
        let host = self
            .host
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the application install dir is unknown"))?;
        let exe = host
            .executable
            .as_ref()
            .map_err(|error| std::io::Error::new(error.kind(), error.clone()))?;
        installation_dir(exe)
    }

    pub fn resolve_paths(&self, resources_dir: Option<PathBuf>) -> Result<ResolvedPaths> {
        let config = self.config_dir()?;
        let data = self.data_dir()?;
        let mut resolver = self.clone();
        resolver.roots = Some((config.clone(), data.clone()));
        Ok(ResolvedPaths::new(config, data, resources_dir, resolver))
    }

    pub fn profiles_dir(&self) -> Result<PathBuf> {
        self.config_leaf("profiles")
    }
    pub fn logs_dir(&self) -> Result<PathBuf> {
        self.data_leaf("logs")
    }
    pub fn cache_dir(&self) -> Result<PathBuf> {
        self.data_leaf("cache")
    }
    pub fn profiles_path(&self) -> Result<PathBuf> {
        Ok(self.config_dir()?.join(PROFILE_YAML))
    }
    pub fn clash_pid_path(&self) -> Result<PathBuf> {
        Ok(self.data_dir()?.join("clash.pid"))
    }

    fn config_leaf(&self, name: &str) -> Result<PathBuf> {
        let dir = self.config_dir()?.join(name);
        if let Err(error) = create_dir_all(&dir) {
            log_leaf_error(&error);
        }
        Ok(dir)
    }

    fn data_leaf(&self, name: &str) -> Result<PathBuf> {
        let dir = self.data_dir()?.join(name);
        if let Err(error) = create_dir_all(&dir) {
            log_leaf_error(&error);
        }
        Ok(dir)
    }

    pub fn find_binary_path(&self, executable_name: &str) -> std::io::Result<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(data) = self.data_dir() {
            candidates.push(data.join(executable_name));
        }
        if let Ok(install) = self.install_dir() {
            candidates.push(install.join(executable_name));
        }
        if let Some(path) = self
            .host
            .as_ref()
            .and_then(|host| host.development_binaries.get(executable_name))
        {
            candidates.push(path.clone());
        }
        candidates
            .into_iter()
            .find(|path| path.exists())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("{executable_name} not found"),
                )
            })
    }

    /// Diagnostics deliberately preserve the existing directory-valued data hit.
    pub fn data_or_sidecar_path(&self, name: impl AsRef<str>) -> Result<PathBuf> {
        let name = platform::executable_name(name.as_ref());
        let data = self.data_dir()?;
        if data.join(&name).exists() {
            return Ok(data);
        }
        Ok(self.install_dir()?.join(name))
    }
}

// Keep leaf preparation best-effort while propagating base-root errors.
fn log_leaf_error(error: &std::io::Error) {
    log::error!(target: "app", "{error:#?}");
}

/// Resolve a supplied executable's parent, including canonicalization.
pub fn installation_dir(executable: &Path) -> Result<PathBuf> {
    let executable = dunce::canonicalize(executable)?;
    executable
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| anyhow::anyhow!("failed to get the app install dir"))
}

pub fn create_dir_all(dir: &Path) -> std::io::Result<()> {
    if let Ok(meta) = fs_err::metadata(dir) {
        if !meta.is_dir() {
            fs_err::remove_file(dir)?;
        } else {
            return Ok(());
        }
    }
    fs_extra::dir::create_all(dir, false).map_err(|error| {
        std::io::Error::other(format!(
            "failed to create dir: {:?}, kind: {:?}",
            error, error.kind
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod test {
    #[test]
    #[ignore]
    fn test_dir_placeholder() {
        let resolver = super::PathResolver::new(super::HostInputs {
            app_name: "clash-nyanpasu".into(),
            executable: std::env::current_exe().map_err(std::sync::Arc::new),
            portable: false,
            development_binaries: Default::default(),
        });
        let placeholder = resolver.directory_name().unwrap();
        if cfg!(windows) {
            assert_eq!(placeholder, "Clash Nyanpasu");
        } else {
            assert_eq!(placeholder, "clash-nyanpasu");
        }
    }
}
