//! OS adapters for neutral remote-access authentication ports.

use std::{path::Path, sync::Arc};

use nyanpasu_contracts::auth::{AuthClock, EntropySource, SecretCipher, SecretStore};

mod clock_entropy;
mod keyring_backend;
mod secret_cipher;
mod secret_store;

#[cfg(unix)]
mod unix_store;
#[cfg(windows)]
mod windows_store;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod tests;

pub use clock_entropy::{OsAuthClock, OsEntropySource};
pub use keyring_backend::{CredentialSecretBackend, KeyringBackendError, OsKeyringBackend};
pub use secret_cipher::{KeyringSecretCipher, instance_key_account};
pub use secret_store::FsSecretStore;

const SHARED_REMOTE_ACCESS_KEY_SCOPE: &str = "remote-access";

/// Neutral OS-backed authentication adapters constructed with a shared installation key identity.
pub struct OsRemoteAuthBundle {
    /// OS wall + monotonic clock source.
    pub clock: Arc<dyn AuthClock>,
    /// OS cryptographic random byte generator.
    pub entropy: Arc<dyn EntropySource>,
    /// Handle-verified filesystem secret store.
    pub store: Arc<dyn SecretStore>,
    /// Lazy OS keyring + XChaCha20-Poly1305 fallback cipher.
    pub cipher: Arc<dyn SecretCipher>,
}

/// Build the production OS-backed remote-auth bundle for `secrets_path` and `max_record_bytes`
/// using the platform keyring backend and shared `"remote-access"` key scope.
#[must_use]
pub fn build_os_remote_auth_bundle(
    secrets_path: &Path,
    max_record_bytes: usize,
) -> OsRemoteAuthBundle {
    build_remote_auth_bundle_with_backend(
        secrets_path,
        max_record_bytes,
        Arc::new(OsKeyringBackend::new()),
    )
}

/// Build the remote-auth OS bundle using a caller-supplied [`CredentialSecretBackend`] while
/// preserving the shared `"remote-access"` key identity, `FsSecretStore`, `OsAuthClock`, and
/// `OsEntropySource`.
#[must_use]
pub fn build_remote_auth_bundle_with_backend(
    secrets_path: &Path,
    max_record_bytes: usize,
    backend: Arc<dyn CredentialSecretBackend>,
) -> OsRemoteAuthBundle {
    let clock: Arc<dyn AuthClock> = Arc::new(OsAuthClock::new());
    let entropy: Arc<dyn EntropySource> = Arc::new(OsEntropySource);
    let store: Arc<dyn SecretStore> = Arc::new(FsSecretStore::new(
        secrets_path.to_path_buf(),
        max_record_bytes,
    ));
    let account = instance_key_account(SHARED_REMOTE_ACCESS_KEY_SCOPE, secrets_path);
    let cipher: Arc<dyn SecretCipher> = Arc::new(KeyringSecretCipher::with_default_service(
        account,
        backend,
        Arc::clone(&entropy),
    ));
    OsRemoteAuthBundle {
        clock,
        entropy,
        store,
        cipher,
    }
}
