//! The credentials of one sync provider, held apart from its settings.
//!
//! Settings are ordinary configuration and may be copied or inspected; these
//! values are write-only over RPC and never serialized with the settings, so a
//! synced vault never contains the means to reach itself.

use snafu::Snafu;

use super::provider::ProviderSecrets;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum SecretStoreError {
    #[snafu(display("the sync credential record is not a regular file"))]
    NotRegularFile,
    #[snafu(display("failed to read or write the sync credential record: {reason}"))]
    Io { reason: String },
}

/// Durable, owner-only storage of the provider credentials.
///
/// The default implementation writes one owner-only file; an OS keychain
/// adapter can replace it behind this same port. A missing record is `None`,
/// not an error.
pub trait SecretStore: Send + Sync + 'static {
    /// The stored credentials, or `None` when none were written yet.
    ///
    /// # Errors
    ///
    /// [`SecretStoreError::Io`] when the record exists but cannot be read.
    fn load(&self) -> Result<Option<ProviderSecrets>, SecretStoreError>;

    /// Atomically replaces the stored credentials.
    ///
    /// # Errors
    ///
    /// [`SecretStoreError::Io`] when the record cannot be written.
    fn save(&self, secrets: &ProviderSecrets) -> Result<(), SecretStoreError>;

    /// Removes the stored credentials. Clearing an absent record succeeds.
    ///
    /// # Errors
    ///
    /// [`SecretStoreError::Io`] when the record exists but cannot be removed.
    fn clear(&self) -> Result<(), SecretStoreError>;
}
