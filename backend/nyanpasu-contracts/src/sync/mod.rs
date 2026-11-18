//! Ports and values of configuration sync that an implementation crate provides.
//!
//! The application core plans and merges; a storage adapter only moves bytes.
//! [`RemoteStore`] is that byte-level boundary, small enough that a second
//! backend or a test fake is cheap, and free of the features providers
//! disagree on (etags, rename, copy, conditional overwrite).

mod ports;
mod provider;
mod secrets;
mod settings;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use ports::{
    PutCondition, RemoteCapabilities, RemoteEntry, RemoteError, RemoteStore, RemoteStoreFactory,
    validate_key,
};
pub use provider::{ProviderConfig, ProviderKind, ProviderSecrets};
pub use secrets::{SecretStore, SecretStoreError};
pub use settings::{JoinStrategy, SyncNetwork, SyncSettings};
