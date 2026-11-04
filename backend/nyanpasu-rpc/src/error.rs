//! Maps application failures into the RPC envelope. [`IpcError`] is the
//! command error type the frontend localizes by domain;
//! [`application_error`] wraps any command failure into an [`RpcError`],
//! keeping the structured domain error and the core's retry metadata.

use std::fmt;

use nyanpasu_core::{
    client::{
        ClientError, RuntimeError, SystemDnsError, effects::error::EffectsError,
        system_proxy::ports::OsProxyError,
    },
    state::{config_error::ConfigError, profiles::ProfilesError},
    storage::StorageOperationError,
};
use nyanpasu_rpc_protocol::{RpcError, RpcErrorKind, RpcValue};
use serde::Serialize;

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    /// The original error, copied by the user for diagnosis.
    detail: String,
}

impl IpcError {
    /// A failure outside every typed domain, such as one from a shell
    /// framework; only its message describes it.
    pub fn unknown(error: impl fmt::Display + fmt::Debug) -> Self {
        Self {
            message: error.to_string(),
            detail: format!("{error:?}"),
            kind: IpcErrorKind::Unknown,
        }
    }
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
    E: fmt::Display + fmt::Debug,
    IpcErrorKind: From<E>,
{
    fn from(error: E) -> Self {
        Self {
            message: error.to_string(),
            detail: format!("{error:?}"),
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
    serde_yaml_ng::Error,
    serde_json::Error,
    anyhow::Error,
    nyanpasu_migration::backup::BackupError,
);

/// Wraps a command failure into the RPC envelope.
pub fn application_error(error: impl std::error::Error + 'static) -> RpcError {
    application_error_ref(&error)
}

/// Wraps a command failure into the RPC envelope, keeping the structured
/// domain error and the core's retry metadata found anywhere in its source
/// chain.
pub fn application_error_ref(error: &(dyn std::error::Error + 'static)) -> RpcError {
    let mut result = RpcError::new(RpcErrorKind::ApplicationError, error.to_string());
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if let Some(log) = error.downcast_ref::<nyanpasu_logging::LogError>() {
            result.domain_error = serde_json::to_value(log)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        if let Some(log) = error.downcast_ref::<nyanpasu_core::logs::CoreLogError>() {
            result.domain_error = serde_json::to_value(log)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        if let Some(core) = error.downcast_ref::<nyanpasu_core_manager::CoreError>() {
            result.code = core.kind.map(|kind| kind.to_string());
            result.retryable = Some(core.retryable);
            result.operation_id = core.operation_id.map(|id| id.to_string());
        }
        if let Some(ipc) = error.downcast_ref::<IpcError>() {
            result.domain_error = serde_json::to_value(ipc)
                .ok()
                .map(|value| Box::new(RpcValue(value)));
        }
        source = error.source();
    }
    result
}

#[cfg(test)]
mod tests;
