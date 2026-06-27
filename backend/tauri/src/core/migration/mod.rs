use crate::utils::path::PathResolver;
use semver::Version;
use std::path::PathBuf;

pub(crate) mod fs;
pub mod modules;
pub mod registry;
pub mod runner;
pub mod store;

pub use runner::Runner;

pub type MigrationId = &'static str;
pub type ModuleId = &'static str;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    NotStarted,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationAdvice {
    Pending,
    Ignored,
    Done,
}

impl std::fmt::Display for MigrationAdvice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationAdvice::Pending => write!(f, "Pending"),
            MigrationAdvice::Ignored => write!(f, "Ignored"),
            MigrationAdvice::Done => write!(f, "Done"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ctx {
    paths: PathResolver,
}

impl Ctx {
    pub fn from_app_dirs() -> anyhow::Result<Self> {
        Ok(Self {
            paths: PathResolver::from_env()?,
        })
    }

    #[cfg(test)]
    pub fn new(app_config_dir: PathBuf, app_data_dir: PathBuf) -> Self {
        Self {
            paths: PathResolver::with_base_dirs(app_config_dir, app_data_dir),
        }
    }

    /// The underlying path resolver, the single source of truth for app paths.
    pub fn paths(&self) -> &PathResolver {
        &self.paths
    }

    pub fn profiles_path(&self) -> PathBuf {
        self.paths.profiles_path()
    }

    pub fn nyanpasu_config_path(&self) -> PathBuf {
        self.paths.nyanpasu_config_path()
    }

    pub fn storage_path(&self) -> PathBuf {
        self.paths.storage_path()
    }

    pub fn state_path(&self) -> PathBuf {
        self.paths.app_config_dir().join(store::STORE_FILE_NAME)
    }
}

pub trait MigrationStep: Send + Sync {
    fn id(&self) -> MigrationId;
    fn module(&self) -> ModuleId;
    fn revision(&self) -> u64;
    fn introduced_in(&self) -> &'static Version;
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()>;
    fn rollback(&self, _: &mut Ctx) -> anyhow::Result<()> {
        Ok(())
    }
}

pub trait ModuleMigrator: Send + Sync {
    fn module(&self) -> ModuleId;
    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64>;
    fn steps(&self) -> &'static [&'static dyn MigrationStep];
}

pub fn current_version() -> anyhow::Result<Version> {
    Version::parse(crate::consts::BUILD_INFO.pkg_version).map_err(Into::into)
}
