//! The byte-level remote storage port.

use std::sync::Arc;

use bytes::Bytes;
use snafu::{Snafu, ensure};

use super::provider::{ProviderConfig, ProviderKind, ProviderSecrets};
use crate::BoxedCause;

/// What a backend claims to support. The vault self-test decides what is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCapabilities {
    /// [`PutCondition::IfAbsent`] is advertised as an atomic create.
    pub create_if_absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    /// Relative to the vault root, forward slashes.
    pub key: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutCondition {
    /// Create or overwrite.
    Any,
    /// Create only; an existing object fails with [`RemoteError::AlreadyExists`].
    IfAbsent,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum RemoteError {
    #[snafu(display("remote object {key:?} does not exist"))]
    NotFound { key: String },
    /// [`PutCondition::IfAbsent`] found an object.
    #[snafu(display("remote object {key:?} already exists"))]
    AlreadyExists { key: String },
    #[snafu(display("the remote rejected the credentials"))]
    Unauthorized { source: BoxedCause },
    /// Network failure, timeout, server error or rate limit, after the adapter's own retries.
    #[snafu(display("the remote is unavailable"))]
    Unavailable { source: BoxedCause },
    #[snafu(display("the remote does not support {operation}"))]
    Unsupported { operation: &'static str },
    #[snafu(display("{key:?} is not a valid remote key"))]
    InvalidKey { key: String },
    #[snafu(display("the provider configuration is incomplete: {field} is missing"))]
    MissingSetting { field: &'static str },
    #[snafu(display("remote operation failed"))]
    Other { source: BoxedCause },
}

/// Objects of one vault, addressed by keys relative to its root.
///
/// Keys use forward slashes and pass [`validate_key`]. Deadlines and retries
/// belong to the implementation: callers await the real result.
#[async_trait::async_trait]
pub trait RemoteStore: Send + Sync + 'static {
    fn capabilities(&self) -> RemoteCapabilities;

    /// Checks reachability and credentials.
    async fn check(&self) -> Result<(), RemoteError>;

    /// Every object whose key starts with `prefix`, recursively, in any order.
    /// A prefix without objects is an empty list, not an error.
    async fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, RemoteError>;

    /// The object's bytes, or [`RemoteError::NotFound`].
    async fn get(&self, key: &str) -> Result<Bytes, RemoteError>;

    async fn put(&self, key: &str, body: Bytes, condition: PutCondition)
    -> Result<(), RemoteError>;

    /// Removes the object. Deleting a missing object succeeds.
    async fn delete(&self, key: &str) -> Result<(), RemoteError>;
}

/// Opens stores for the provider kinds compiled into a build.
pub trait RemoteStoreFactory: Send + Sync + 'static {
    fn kinds(&self) -> Vec<ProviderKind>;

    /// Builds a store without contacting the provider.
    ///
    /// # Errors
    ///
    /// [`RemoteError::Unsupported`] for a kind this build lacks, or a
    /// configuration the backend rejects.
    fn open(
        &self,
        config: &ProviderConfig,
        secrets: &ProviderSecrets,
    ) -> Result<Arc<dyn RemoteStore>, RemoteError>;
}

/// Checks that `key` is relative, uses forward slashes and has no empty,
/// `.` or `..` segment, so no backend can resolve it outside the vault root.
///
/// # Errors
///
/// [`RemoteError::InvalidKey`].
pub fn validate_key(key: &str) -> Result<(), RemoteError> {
    let valid = !key.contains('\\')
        && key
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    ensure!(valid, InvalidKeySnafu { key });
    Ok(())
}
