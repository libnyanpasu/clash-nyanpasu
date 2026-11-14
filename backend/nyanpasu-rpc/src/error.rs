//! Command error mapping independent of the desktop transport.

use nyanpasu_contracts::{system_dns::SystemDnsError, system_proxy::OsProxyError};
use nyanpasu_core::{
    client::{
        ClashReadError, ClientError, CoreLogReadError, ProfileSyncReadError, SetRemoteAccessError,
        http_server::HttpServerControlError,
    },
    state::config_error::ConfigError,
    storage::StorageOperationError,
};
use nyanpasu_rpc_protocol::{ResponseVisibility, RpcError, types::PublicFailure};
use nyanpasu_runtime_values::{
    effects::EffectsError, profiles::ProfilesError, runtime_error::RuntimeError,
};
use serde::Serialize;

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for IpcError {}

/// A failed command as the frontend receives it.
#[derive(Debug, Serialize, specta::Type)]
pub struct IpcError {
    /// The domain failure; the frontend localizes it.
    kind: IpcErrorKind,
    /// The error's own message, shown when `kind` cannot be localized.
    message: String,
    /// The original error, copied by the user for diagnosis when permitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    detail: Option<String>,
}

/// The domain a command failed in. A domain joins once its errors are typed.
#[derive(Debug, Serialize, specta::Type)]
#[serde(tag = "domain", content = "error", rename_all = "snake_case")]
pub enum IpcErrorKind {
    /// Not classified into a domain; only `message` describes it.
    Unknown,
    Profiles(Box<ProfilesError>),
    Runtime(Box<RuntimeError>),
    Config(Box<ConfigError>),
    Storage(Box<StorageOperationError>),
    SystemDns(Box<SystemDnsError>),
    SystemProxy(Box<OsProxyError>),
    Effects(Box<EffectsError>),
    HttpServer(Box<HttpServerControlError>),
    ClashRead(Box<ClashReadError>),
    CoreLogRead(Box<CoreLogReadError>),
    ProfileSyncRead(Box<ProfileSyncReadError>),
    /// Exhaustive public classification for callers without `Scope::SensitiveRead`.
    Public(Box<PublicFailure>),
}

impl From<HttpServerControlError> for IpcErrorKind {
    fn from(error: HttpServerControlError) -> Self {
        Self::HttpServer(Box::new(error))
    }
}

impl From<ClashReadError> for IpcErrorKind {
    fn from(error: ClashReadError) -> Self {
        Self::ClashRead(Box::new(error))
    }
}

impl From<CoreLogReadError> for IpcErrorKind {
    fn from(error: CoreLogReadError) -> Self {
        Self::CoreLogRead(Box::new(error))
    }
}

impl From<ProfileSyncReadError> for IpcErrorKind {
    fn from(error: ProfileSyncReadError) -> Self {
        Self::ProfileSyncRead(Box::new(error))
    }
}

impl From<SetRemoteAccessError> for IpcErrorKind {
    fn from(error: SetRemoteAccessError) -> Self {
        match error {
            SetRemoteAccessError::Server { source } => source.into(),
            SetRemoteAccessError::Client { source } => (*source).into(),
            SetRemoteAccessError::OwnerStopped => Self::Unknown,
        }
    }
}

impl From<EffectsError> for IpcErrorKind {
    fn from(error: EffectsError) -> Self {
        Self::Effects(Box::new(error))
    }
}

impl From<OsProxyError> for IpcErrorKind {
    fn from(error: OsProxyError) -> Self {
        Self::SystemProxy(Box::new(error))
    }
}

impl From<SystemDnsError> for IpcErrorKind {
    fn from(error: SystemDnsError) -> Self {
        Self::SystemDns(Box::new(error))
    }
}

impl From<StorageOperationError> for IpcErrorKind {
    fn from(error: StorageOperationError) -> Self {
        Self::Storage(Box::new(error))
    }
}

impl From<ConfigError> for IpcErrorKind {
    fn from(error: ConfigError) -> Self {
        Self::Config(Box::new(error))
    }
}

impl From<RuntimeError> for IpcErrorKind {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(Box::new(error))
    }
}

impl From<ProfilesError> for IpcErrorKind {
    fn from(error: ProfilesError) -> Self {
        Self::Profiles(Box::new(error))
    }
}

impl From<ClientError> for IpcErrorKind {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Profiles(error) => Self::Profiles(Box::new(error)),
            ClientError::Runtime(error) => Self::Runtime(Box::new(error)),
            ClientError::Config(error) => Self::Config(Box::new(error)),
            ClientError::Storage(error) => Self::Storage(Box::new(error)),
            _ => Self::Unknown,
        }
    }
}

impl<E> From<E> for IpcError
where
    E: std::fmt::Display + std::fmt::Debug,
    IpcErrorKind: From<E>,
{
    fn from(error: E) -> Self {
        Self {
            message: error.to_string(),
            detail: Some(format!("{error:?}")),
            kind: error.into(),
        }
    }
}

macro_rules! unknown_domain {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for IpcErrorKind {
            fn from(_: $error) -> Self {
                Self::Unknown
            }
        }
    )*};
}

unknown_domain!(
    String,
    std::io::Error,
    serde_yaml::Error,
    serde_json::Error,
    anyhow::Error,
    nyanpasu_migration::backup::BackupError,
);

pub type Result<T = ()> = std::result::Result<T, IpcError>;

/// Error union emitted by `clear_core_logs`: rich `CoreLogError` for
/// `SensitiveRead` callers and projected public [`IpcError`] otherwise.
#[derive(Debug, Serialize, specta::Type)]
#[serde(untagged)]
pub enum CoreLogCommandError {
    CoreLog(nyanpasu_observers::logs::CoreLogError),
    Public(IpcError),
}

impl std::fmt::Display for CoreLogCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CoreLog(error) => error.fmt(f),
            Self::Public(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CoreLogCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CoreLog(error) => Some(error),
            Self::Public(error) => Some(error),
        }
    }
}

impl From<nyanpasu_observers::logs::CoreLogError> for CoreLogCommandError {
    fn from(error: nyanpasu_observers::logs::CoreLogError) -> Self {
        Self::CoreLog(error)
    }
}

impl IpcError {
    pub fn unknown(error: impl std::fmt::Display + std::fmt::Debug) -> Self {
        Self {
            kind: IpcErrorKind::Unknown,
            message: error.to_string(),
            detail: Some(format!("{error:?}")),
        }
    }

    pub(crate) fn public(failure: PublicFailure, message: String) -> Self {
        Self {
            kind: IpcErrorKind::Public(Box::new(failure)),
            message,
            detail: None,
        }
    }

    pub(crate) fn public_unknown(message: impl Into<String>) -> Self {
        Self {
            kind: IpcErrorKind::Unknown,
            message: message.into(),
            detail: None,
        }
    }

    pub(crate) fn kind_ref(&self) -> &IpcErrorKind {
        &self.kind
    }

    pub(crate) fn message_str(&self) -> &str {
        &self.message
    }
}

/// Wrap a command failure, preserving domain errors and core retry metadata.
pub fn application_error(error: impl std::error::Error + 'static) -> RpcError {
    application_error_for(ResponseVisibility::Full, error)
}

/// Wrap a command failure while enforcing caller `ResponseVisibility`.
pub fn application_error_for(
    visibility: ResponseVisibility,
    error: impl std::error::Error + 'static,
) -> RpcError {
    application_error_ref_for(visibility, &error)
}

/// Wrap a command failure reference while enforcing caller `ResponseVisibility`.
pub fn application_error_ref_for(
    visibility: ResponseVisibility,
    error: &(dyn std::error::Error + 'static),
) -> RpcError {
    crate::privacy::application_error_for(visibility, error)
}
