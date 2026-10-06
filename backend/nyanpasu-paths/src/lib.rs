//! Application directory and binary resolution, independent of any frontend.
//!
//! [`PathResolver`] is the one path value the application passes around. It holds
//! the resolved roots and the install-dir outcome; every other path is a pure
//! function of that context. The only discovery step is [`PathResolver::discover`],
//! which asks the OS for the roots once and creates nothing.

use camino::{Utf8Path, Utf8PathBuf};
use snafu::OptionExt as _;
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

mod error;
#[cfg(windows)]
pub use error::RegistryError;
pub use error::{CreateDirError, DiscoverError, InstallDirError, Root};
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(not(windows))]
#[path = "unix.rs"]
mod platform;
#[cfg(windows)]
pub mod registry;

pub const CLASH_CFG_GUARD_OVERRIDES: &str = "clash-guard-overrides.yaml";
pub const NYANPASU_CONFIG: &str = "nyanpasu-config.yaml";
pub const PROFILE_YAML: &str = "profiles.yaml";
pub const STORAGE_DB: &str = "storage.db";

/// What a frontend knows about the host. Collected once; the resolver derives the rest.
#[derive(Debug)]
pub struct HostInputs {
    pub app_name: String,
    /// The directory the executable lives in. Resolved once by the frontend, which also
    /// derives `portable` from it.
    pub install_dir: Result<Utf8PathBuf, InstallDirError>,
    /// Whether the package keeps its config and data next to the executable. Only Windows
    /// has a portable layout.
    pub portable: bool,
    /// Binaries a development build runs from, keyed by executable name.
    pub development_binaries: BTreeMap<String, Utf8PathBuf>,
}

#[derive(Debug)]
struct Context {
    config_dir: Utf8PathBuf,
    data_dir: Utf8PathBuf,
    install_dir: Result<Utf8PathBuf, InstallDirError>,
    development_binaries: BTreeMap<String, Utf8PathBuf>,
}

/// The resolved directories of one application instance. Cloning shares them.
#[derive(Debug, Clone)]
pub struct PathResolver(Arc<Context>);

impl PathResolver {
    /// Resolve the config and data roots from the host. Windows picks portable layout, then
    /// the registry's custom config dir, then the OS default; data never uses the registry.
    /// Nothing is created: see [`PathResolver::create_base_dirs`].
    pub fn discover(inputs: HostInputs) -> Result<Self, DiscoverError> {
        let (config_dir, data_dir) = platform::base_dirs(&inputs)?;
        Ok(Self(Arc::new(Context {
            config_dir,
            data_dir,
            install_dir: inputs.install_dir,
            development_binaries: inputs.development_binaries,
        })))
    }

    /// Explicit roots, for tests and tools that do not run from an installed executable.
    /// The install dir is unknown.
    pub fn with_base_dirs(config_dir: Utf8PathBuf, data_dir: Utf8PathBuf) -> Self {
        Self(Arc::new(Context {
            config_dir,
            data_dir,
            install_dir: Err(InstallDirError::Unknown),
            development_binaries: BTreeMap::new(),
        }))
    }

    pub fn app_config_dir(&self) -> &Utf8Path {
        &self.0.config_dir
    }

    pub fn app_data_dir(&self) -> &Utf8Path {
        &self.0.data_dir
    }

    /// The directory the executable lives in; sidecars are placed here.
    pub fn app_install_dir(&self) -> Result<&Utf8Path, InstallDirError> {
        self.0
            .install_dir
            .as_deref()
            .map_err(InstallDirError::clone)
    }

    pub fn app_profiles_dir(&self) -> Utf8PathBuf {
        self.0.config_dir.join("profiles")
    }

    pub fn profiles_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join(PROFILE_YAML)
    }

    pub fn nyanpasu_config_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join(NYANPASU_CONFIG)
    }

    pub fn application_config_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join("application.yaml")
    }

    pub fn session_state_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join("session-state.yaml")
    }

    pub fn clash_config_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join("clash-config.yaml")
    }

    pub fn clash_guard_overrides_path(&self) -> Utf8PathBuf {
        self.0.config_dir.join(CLASH_CFG_GUARD_OVERRIDES)
    }

    pub fn storage_path(&self) -> Utf8PathBuf {
        self.0.data_dir.join(STORAGE_DB)
    }

    pub fn jobs_path(&self) -> Utf8PathBuf {
        self.0.data_dir.join("jobs.redb")
    }

    pub fn backups_dir(&self) -> Utf8PathBuf {
        self.0.data_dir.join("backups")
    }

    pub fn clash_pid_path(&self) -> Utf8PathBuf {
        self.0.data_dir.join("clash.pid")
    }

    pub fn app_logs_dir(&self) -> Utf8PathBuf {
        self.0.data_dir.join("logs")
    }

    pub fn cache_dir(&self) -> Utf8PathBuf {
        self.0.data_dir.join("cache")
    }

    pub fn scripts_dir(&self) -> Utf8PathBuf {
        self.0.data_dir.join("scripts")
    }

    /// Create the config dir, then the data dir, failing on either. The profiles, logs and
    /// cache dirs are created best-effort: they are recreated by whoever uses them.
    pub fn create_base_dirs(&self) -> Result<(), CreateDirError> {
        create_dir_all(self.app_config_dir())?;
        create_dir_all(self.app_data_dir())?;
        for leaf in [
            self.app_profiles_dir(),
            self.app_logs_dir(),
            self.cache_dir(),
        ] {
            if let Err(error) = create_dir_all(&leaf) {
                log::error!(target: "app", "{error:#?}");
            }
        }
        Ok(())
    }

    /// Search for a core binary: data dir, install dir, then the development candidate.
    /// Checked on every call, because a binary may be downloaded after startup.
    pub fn find_binary_path(&self, executable_name: &str) -> io::Result<Utf8PathBuf> {
        let context = &*self.0;
        [
            Some(context.data_dir.join(executable_name)),
            context
                .install_dir
                .as_ref()
                .ok()
                .map(|dir| dir.join(executable_name)),
            context.development_binaries.get(executable_name).cloned(),
        ]
        .into_iter()
        .flatten()
        .find(|path| path.exists())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{executable_name} not found"),
            )
        })
    }

    /// The data dir when it holds the binary, the binary's install-dir path otherwise.
    /// A data hit deliberately returns the directory, not the binary.
    pub fn data_or_sidecar_path(
        &self,
        name: impl AsRef<str>,
    ) -> Result<Utf8PathBuf, InstallDirError> {
        let name = platform::executable_name(name.as_ref());
        if self.0.data_dir.join(&name).exists() {
            return Ok(self.0.data_dir.clone());
        }
        Ok(self.app_install_dir()?.join(name))
    }
}

/// A root the OS suggested, as the UTF-8 value the resolver stores.
fn suggested(root: Root, dir: Option<PathBuf>) -> Result<Utf8PathBuf, DiscoverError> {
    let dir = dir.context(error::discover_error::NoPlatformDefaultSnafu { root })?;
    Utf8PathBuf::from_path_buf(dir).map_err(|path| DiscoverError::NotUtf8 { root, path })
}

/// The directory of a supplied executable, after canonicalization.
pub fn installation_dir(executable: &Path) -> Result<Utf8PathBuf, InstallDirError> {
    let executable =
        dunce::canonicalize(executable).map_err(|error| InstallDirError::Canonicalize {
            source: Arc::new(error),
        })?;
    let dir = executable
        .parent()
        .context(error::install_dir_error::NoParentSnafu)?;
    Utf8PathBuf::from_path_buf(dir.to_owned()).map_err(|path| InstallDirError::NotUtf8 { path })
}

/// Create `dir` and its parents. A file in the way is removed first.
pub fn create_dir_all(dir: &Utf8Path) -> Result<(), CreateDirError> {
    let create = || -> io::Result<()> {
        if let Ok(meta) = fs_err::metadata(dir) {
            if meta.is_dir() {
                return Ok(());
            }
            fs_err::remove_file(dir)?;
        }
        fs_extra::dir::create_all(dir, false).map_err(|error| {
            io::Error::other(format!(
                "failed to create dir: {:?}, kind: {:?}",
                error, error.kind
            ))
        })
    };
    create().map_err(|source| CreateDirError {
        path: dir.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf8(path: &Path) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(path.to_owned()).unwrap()
    }

    fn resolver() -> PathResolver {
        PathResolver::with_base_dirs("/cfg".into(), "/data".into())
    }

    #[test]
    fn data_derived_paths_join_the_data_dir() {
        let r = resolver();
        assert_eq!(r.storage_path(), Utf8Path::new("/data").join(STORAGE_DB));
        assert_eq!(r.backups_dir(), Utf8Path::new("/data").join("backups"));
        assert_eq!(r.clash_pid_path(), Utf8Path::new("/data").join("clash.pid"));
        assert_eq!(r.app_logs_dir(), Utf8Path::new("/data").join("logs"));
        assert_eq!(r.cache_dir(), Utf8Path::new("/data").join("cache"));
        assert_eq!(r.scripts_dir(), Utf8Path::new("/data").join("scripts"));
    }

    #[test]
    #[ignore]
    fn test_dir_placeholder() {
        let placeholder = platform::directory_name("clash-nyanpasu");
        if cfg!(windows) {
            assert_eq!(placeholder, "Clash Nyanpasu");
        } else {
            assert_eq!(placeholder, "clash-nyanpasu");
        }
    }

    #[test]
    fn base_dirs_are_exposed_verbatim() {
        let r = resolver();
        assert_eq!(r.app_config_dir(), Utf8Path::new("/cfg"));
        assert_eq!(r.app_data_dir(), Utf8Path::new("/data"));
    }

    #[test]
    fn explicit_roots_know_no_install_dir() {
        assert!(matches!(
            resolver().app_install_dir(),
            Err(InstallDirError::Unknown)
        ));
    }

    #[test]
    fn binaries_are_searched_in_data_then_install_then_development_order() {
        let root = tempfile::tempdir().unwrap();
        let (data, install, development) = (
            utf8(&root.path().join("data")),
            utf8(&root.path().join("install")),
            utf8(&root.path().join("development")),
        );
        for dir in [&data, &install, &development] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let resolver = PathResolver(Arc::new(Context {
            config_dir: utf8(&root.path().join("config")),
            data_dir: data.clone(),
            install_dir: Ok(install.clone()),
            development_binaries: BTreeMap::from([("core".into(), development.join("core"))]),
        }));

        assert_eq!(
            resolver.find_binary_path("core").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::write(development.join("core"), "").unwrap();
        assert_eq!(
            resolver.find_binary_path("core").unwrap(),
            development.join("core")
        );
        std::fs::write(install.join("core"), "").unwrap();
        assert_eq!(
            resolver.find_binary_path("core").unwrap(),
            install.join("core")
        );
        std::fs::write(data.join("core"), "").unwrap();
        assert_eq!(
            resolver.find_binary_path("core").unwrap(),
            data.join("core")
        );
    }

    #[test]
    fn a_data_hit_names_the_directory_and_a_miss_the_install_path() {
        let root = tempfile::tempdir().unwrap();
        let (data, install) = (
            utf8(&root.path().join("data")),
            utf8(&root.path().join("install")),
        );
        std::fs::create_dir_all(&data).unwrap();
        let resolver = PathResolver(Arc::new(Context {
            config_dir: utf8(&root.path().join("config")),
            data_dir: data.clone(),
            install_dir: Ok(install.clone()),
            development_binaries: BTreeMap::new(),
        }));
        let name = platform::executable_name("core");

        assert_eq!(
            resolver.data_or_sidecar_path("core").unwrap(),
            install.join(&name)
        );
        std::fs::write(data.join(&name), "").unwrap();
        assert_eq!(resolver.data_or_sidecar_path("core").unwrap(), data);
    }

    #[test]
    fn create_dir_all_replaces_a_file_in_the_way() {
        let root = tempfile::tempdir().unwrap();
        let dir = utf8(&root.path().join("a"));
        std::fs::write(&dir, "").unwrap();

        create_dir_all(&dir).unwrap();

        assert!(dir.is_dir());
        create_dir_all(&dir.join("b")).unwrap();
        assert!(dir.join("b").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_executable_dir_is_rejected() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(OsStr::from_bytes(b"inst\xffall"));
        std::fs::create_dir(&dir).unwrap();
        let executable = dir.join("app");
        std::fs::write(&executable, "").unwrap();

        assert!(matches!(
            installation_dir(&executable),
            Err(InstallDirError::NotUtf8 { .. })
        ));
    }
}
