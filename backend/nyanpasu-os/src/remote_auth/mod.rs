//! OS adapters for the neutral remote-access authentication ports.
//!
//! Secrets live in one plain owner-only file under the data directory
//! ([`FsSecretStore`]); there is no OS keyring or cipher layer. [`RcgenCertificateGenerator`]
//! makes the self-signed certificate of the HTTPS listener.

mod clock_entropy;
mod secret_store;
mod tls;

#[cfg(test)]
mod tests;

pub use clock_entropy::{OsAuthClock, OsEntropySource};
pub use secret_store::FsSecretStore;
pub use tls::RcgenCertificateGenerator;
