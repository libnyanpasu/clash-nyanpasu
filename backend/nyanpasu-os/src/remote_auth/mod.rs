//! OS adapters for neutral remote-access authentication ports.

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
