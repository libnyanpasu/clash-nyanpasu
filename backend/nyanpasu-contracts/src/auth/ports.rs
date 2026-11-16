//! Narrow consumer-owned infrastructure ports for remote-access authentication.
//!
//! Implemented by OS adapters in `nyanpasu-os` and by deterministic fakes in tests.

use async_trait::async_trait;
use snafu::Snafu;
use zeroize::Zeroizing;

/// Injectable wall and monotonic clock for expiry and idle timeouts.
pub trait AuthClock: Send + Sync {
    /// Wall-clock time in Unix epoch milliseconds, for persisted metadata and token expiry.
    fn now_unix_ms(&self) -> i64;

    /// Monotonic milliseconds, for launch-code lifetimes and session idle timeouts.
    fn monotonic_ms(&self) -> u64;
}

/// Failure from cryptographic random generation.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("cryptographic entropy source failed: {reason}"))]
pub struct EntropyError {
    /// Non-secret failure reason.
    pub reason: String,
}

/// Injectable cryptographic random byte generator.
pub trait EntropySource: Send + Sync {
    /// Fill `dest` with cryptographically strong random bytes.
    fn fill_bytes(&self, dest: &mut [u8]) -> Result<(), EntropyError>;
}

/// Failure modes of the durable secret record.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum SecretStoreError {
    /// The record is larger than the store's bound.
    #[snafu(display("secret record exceeds {max_bytes} bytes"))]
    TooLarge {
        /// The bound that was exceeded.
        max_bytes: usize,
    },
    /// The target is a symlink or another non-regular file, which is refused.
    #[snafu(display("secret record path is not a regular file"))]
    NotRegularFile,
    /// An I/O operation failed.
    #[snafu(display("secret store I/O failed: {reason}"))]
    Io {
        /// Safe, non-secret diagnostic.
        reason: String,
    },
}

/// Durable, owner-only storage of the one remote-access secret record.
///
/// Implementations keep the record readable by the current OS user only and replace it
/// atomically, so a crash leaves the old or the new record, never a partial one.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// The persisted record, or `None` when none was written yet.
    async fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError>;

    /// Atomically replaces the record with `bytes`.
    async fn save(&self, bytes: &[u8]) -> Result<(), SecretStoreError>;
}
