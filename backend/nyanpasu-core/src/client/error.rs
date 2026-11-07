use crate::storage::StorageOperationError;

pub type Result<T = ()> = std::result::Result<T, ClientError>;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml_ng::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Profiles(#[from] nyanpasu_runtime_values::profiles::ProfilesError),
    #[error(transparent)]
    Jobs(#[from] nyanpasu_jobs::Error),
    #[error(transparent)]
    Config(#[from] crate::state::config_error::ConfigError),
    #[error(transparent)]
    Runtime(#[from] nyanpasu_runtime_values::runtime_error::RuntimeError),
    #[error(transparent)]
    Window(#[from] super::hotkey::ports::WindowError),
    #[error("{0}")]
    Custom(String),
}

impl From<String> for ClientError {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl From<&str> for ClientError {
    fn from(value: &str) -> Self {
        Self::Custom(value.to_string())
    }
}
