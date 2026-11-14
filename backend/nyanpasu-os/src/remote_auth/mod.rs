//! OS adapters for the neutral remote-access authentication ports.
//!
//! Secrets live in one plain owner-only file under the data directory
//! ([`FsSecretStore`]); there is no OS keyring or cipher layer.

mod clock_entropy;
mod secret_store;

#[cfg(test)]
mod tests;

pub use clock_entropy::{OsAuthClock, OsEntropySource};
pub use secret_store::FsSecretStore;
