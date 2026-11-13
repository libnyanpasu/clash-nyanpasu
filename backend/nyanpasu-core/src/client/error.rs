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

/// Why a Clash read or probe a caller without `sensitive_read` made has no result.
///
/// The messages are fixed and carry no source: the core's response, the request it failed on and
/// the private URLs behind a probe stay out of the error, and the raw diagnostics stay on the
/// protected full queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClashReadError {
    #[error("the Clash core settings are unavailable")]
    SettingsUnavailable,
    #[error("the proxies are unavailable")]
    ProxiesUnavailable,
    #[error("the proxy group does not exist")]
    UnknownGroup,
    #[error("the delay test could not be completed")]
    DelayTestFailed,
    #[error("the Clash core version is unavailable")]
    VersionUnavailable,
    #[error("the rules are unavailable")]
    RulesUnavailable,
    #[error("the rule providers are unavailable")]
    RuleProvidersUnavailable,
    #[error("the proxy providers are unavailable")]
    ProxyProvidersUnavailable,
    #[error("the Clash streams are unavailable")]
    StreamsUnavailable,
}

/// Why a core log query a caller without `sensitive_read` made has no result.
///
/// The messages are fixed and carry no source: backend store paths and storage errors stay out of
/// the error, and their diagnostics stay on the protected full queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreLogReadError {
    #[error("invalid core log request")]
    InvalidRequest,
    #[error("the log cursor has expired")]
    CursorExpired,
    #[error("the log record is no longer available")]
    RecordGone,
    #[error("the log record exceeds the storage limit")]
    TooLarge,
    #[error("core log storage is unavailable")]
    Unavailable,
}

impl From<&nyanpasu_observers::logs::CoreLogError> for CoreLogReadError {
    fn from(error: &nyanpasu_observers::logs::CoreLogError) -> Self {
        match error {
            nyanpasu_observers::logs::CoreLogError::InvalidRequest => Self::InvalidRequest,
            nyanpasu_observers::logs::CoreLogError::CursorExpired => Self::CursorExpired,
            nyanpasu_observers::logs::CoreLogError::RecordGone => Self::RecordGone,
            nyanpasu_observers::logs::CoreLogError::TooLarge => Self::TooLarge,
            nyanpasu_observers::logs::CoreLogError::Unavailable(_) => Self::Unavailable,
        }
    }
}

impl From<nyanpasu_observers::logs::CoreLogError> for CoreLogReadError {
    fn from(error: nyanpasu_observers::logs::CoreLogError) -> Self {
        Self::from(&error)
    }
}

/// Why a profile's sync status or run history a caller without `sensitive_read` made has no result.
///
/// The messages are fixed and carry no source: the journal, the scheduler and the adapters stay
/// out of the error, and their diagnostics stay on the protected full queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileSyncReadError {
    #[error("the profile does not exist")]
    UnknownProfile,
    #[error("the profile has no remote source to sync")]
    NotRemote,
    #[error("the run history cursor is invalid")]
    InvalidCursor,
    #[error("the sync status is unavailable")]
    Unavailable,
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
