//! Narrow credential-store backend and concrete OS keyring provider adapter.

use std::fmt;

use async_trait::async_trait;
use zeroize::Zeroizing;

/// Safe, redacted error category returned by [`CredentialSecretBackend`].
///
/// Underlying `keyring-core` errors can embed raw encoded bytes (`BadEncoding(Vec<u8>)` or
/// `BadDataFormat(Vec<u8>, _)`), so implementations MUST map by variant to static messages
/// without formatting raw provider errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyringBackendError {
    /// No credential entry exists for `(service, account)`.
    NotFound,
    /// Credential service is inaccessible, unconfigured, or failed at the OS boundary.
    Unavailable {
        /// Safe non-secret failure category.
        reason: &'static str,
    },
    /// Stored credential entry exists but its format is corrupted or not 32-byte key hex.
    InvalidStoredFormat {
        /// Safe non-secret format category.
        reason: &'static str,
    },
}

impl fmt::Display for KeyringBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("credential entry not found"),
            Self::Unavailable { reason } => f.write_str(reason),
            Self::InvalidStoredFormat { reason } => f.write_str(reason),
        }
    }
}

impl std::error::Error for KeyringBackendError {}

/// Abstract key-storage seam allowing real OS keyring execution and deterministic tests without
/// modifying the caller's global keyring store or user keychain.
#[async_trait]
pub trait CredentialSecretBackend: Send + Sync {
    /// Fetch the stored key material for `(service, account)`.
    async fn get_key_hex(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<String>, KeyringBackendError>;

    /// Persist hex-encoded key material for `(service, account)`.
    async fn set_key_hex(
        &self,
        service: &str,
        account: &str,
        hex_secret: &str,
    ) -> Result<(), KeyringBackendError>;
}

/// Concrete OS keyring backend constructed explicitly without touching `keyring_core::set_default_store`.
#[derive(Debug, Clone, Default)]
pub struct OsKeyringBackend;

impl OsKeyringBackend {
    /// Construct a new OS keyring backend handle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CredentialSecretBackend for OsKeyringBackend {
    async fn get_key_hex(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<String>, KeyringBackendError> {
        let service = service.to_owned();
        let account = account.to_owned();
        let joined = tokio::task::spawn_blocking(move || get_os_key_hex(&service, &account)).await;
        crate::blocking::join(joined)
    }

    async fn set_key_hex(
        &self,
        service: &str,
        account: &str,
        hex_secret: &str,
    ) -> Result<(), KeyringBackendError> {
        let service = service.to_owned();
        let account = account.to_owned();
        let hex_secret = Zeroizing::new(hex_secret.to_owned());
        let joined =
            tokio::task::spawn_blocking(move || set_os_key_hex(&service, &account, &hex_secret))
                .await;
        crate::blocking::join(joined)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn build_platform_entry(
    service: &str,
    account: &str,
) -> Result<keyring_core::Entry, KeyringBackendError> {
    use keyring_core::api::CredentialStoreApi;

    #[cfg(target_os = "linux")]
    let store = zbus_secret_service_keyring_store::Store::new().map_err(map_keyring_error)?;
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new().map_err(map_keyring_error)?;
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new().map_err(map_keyring_error)?;

    #[cfg(target_os = "windows")]
    {
        let modifiers = std::collections::HashMap::from([("persistence", "Local")]);
        store
            .build(service, account, Some(&modifiers))
            .map_err(map_keyring_error)
    }

    #[cfg(not(target_os = "windows"))]
    {
        store
            .build(service, account, None)
            .map_err(map_keyring_error)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn get_os_key_hex(service: &str, account: &str) -> Result<Zeroizing<String>, KeyringBackendError> {
    let entry = build_platform_entry(service, account)?;
    entry
        .get_password()
        .map(Zeroizing::new)
        .map_err(map_keyring_error)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn set_os_key_hex(
    service: &str,
    account: &str,
    hex_secret: &str,
) -> Result<(), KeyringBackendError> {
    let entry = build_platform_entry(service, account)?;
    entry.set_password(hex_secret).map_err(map_keyring_error)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn get_os_key_hex(
    _service: &str,
    _account: &str,
) -> Result<Zeroizing<String>, KeyringBackendError> {
    Err(KeyringBackendError::Unavailable {
        reason: "OS credential store unsupported on this platform",
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn set_os_key_hex(
    _service: &str,
    _account: &str,
    _hex_secret: &str,
) -> Result<(), KeyringBackendError> {
    Err(KeyringBackendError::Unavailable {
        reason: "OS credential store unsupported on this platform",
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn map_keyring_error(error: keyring_core::Error) -> KeyringBackendError {
    match error {
        keyring_core::Error::NoEntry => KeyringBackendError::NotFound,
        keyring_core::Error::BadEncoding(_)
        | keyring_core::Error::BadDataFormat(_, _)
        | keyring_core::Error::BadStoreFormat(_) => KeyringBackendError::InvalidStoredFormat {
            reason: "OS credential store entry encoding is invalid",
        },
        keyring_core::Error::NoStorageAccess(_)
        | keyring_core::Error::PlatformFailure(_)
        | keyring_core::Error::NoDefaultStore
        | keyring_core::Error::NotSupportedByStore(_) => KeyringBackendError::Unavailable {
            reason: "OS credential store service is unavailable",
        },
        keyring_core::Error::Invalid(_, _)
        | keyring_core::Error::TooLong(_, _)
        | keyring_core::Error::Ambiguous(_) => KeyringBackendError::Unavailable {
            reason: "OS credential store entry lookup failed",
        },
        _ => KeyringBackendError::Unavailable {
            reason: "OS credential store operation failed",
        },
    }
}
