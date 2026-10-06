//! OS identity and directory inputs for service operations, not service commands.
use crate::PathResolver;
use std::path::PathBuf;

#[cfg(windows)]
pub type UserError = std::io::Error;
#[cfg(not(windows))]
pub type UserError = whoami::Error;

#[derive(Debug, thiserror::Error)]
pub enum ServicePathError {
    #[error("could not resolve the current user: {0}")]
    User(#[source] UserError),
    #[error("could not resolve the application directories: {0}")]
    Directories(#[source] anyhow::Error),
}

#[derive(Debug)]
pub struct ServiceInstallPaths {
    pub user: String,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub install_dir: PathBuf,
}

#[derive(Debug)]
pub struct ServiceUpdatePaths {
    pub user: Option<String>,
    pub data_dir: Option<PathBuf>,
}

impl PathResolver {
    pub async fn service_install_paths(&self) -> Result<ServiceInstallPaths, ServicePathError> {
        let user = self.service_user().await?;
        let data_dir = self.data_dir().map_err(ServicePathError::Directories)?;
        let config_dir = self.config_dir().map_err(ServicePathError::Directories)?;
        let install_dir = self.install_dir().map_err(ServicePathError::Directories)?;
        Ok(ServiceInstallPaths {
            user,
            data_dir,
            config_dir,
            install_dir,
        })
    }
}
