//! Narrow consumer-owned infrastructure ports for core authorization and secret persistence.
//!
//! Implemented by OS/platform adapters in later layers and deterministic fakes in unit tests.

use async_trait::async_trait;
use snafu::Snafu;

use super::secrets::SecretBytes;

/// Injectable wall and monotonic clock for expiry, idle timeout, and rate limiting.
pub trait AuthClock: Send + Sync {
    /// Current wall-clock time in Unix epoch milliseconds (persisted metadata and wall deadlines).
    fn now_unix_ms(&self) -> i64;

    /// Current monotonic clock in milliseconds (idle timeouts and lockout windows).
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

/// Reported owner-protection guarantee of a durable secret medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageProtection {
    /// Backing medium enforces strict single-user ACL/mode protection on the opened target.
    Protected,
    /// Backing medium cannot prove or enforce single-user protection (e.g. world/group readable,
    /// unsupported filesystem, or missing OS ACL support); secrets MUST be encrypted with an
    /// OS credential store key before persistence.
    Unprotected,
}

/// Raw durable record paired with the medium protection verified when the handle was read.
#[derive(Clone)]
pub struct LoadedSecretRecord {
    /// Raw encoded envelope bytes read from durable storage.
    pub bytes: SecretBytes,
    /// Protection status observed on the opened store handle during load.
    pub protection: StorageProtection,
}

impl std::fmt::Debug for LoadedSecretRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedSecretRecord")
            .field("bytes", &self.bytes)
            .field("protection", &self.protection)
            .finish()
    }
}

/// Failure modes for the durable secret store port.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum SecretStoreError {
    /// Write-time verification determined the backing medium is not owner-protected; adapter
    /// refused without writing plaintext.
    #[snafu(display("secret medium is not owner-protected at write time"))]
    UnprotectedMedium,
    /// Underlying filesystem or I/O operation failed.
    #[snafu(display("secret store I/O failed: {reason}"))]
    Io {
        /// Safe non-secret I/O diagnostic.
        reason: String,
    },
}

/// Narrow port for durable persistence of the remote-access secret store envelope.
///
/// Implementations MUST verify owner-only medium protection inside `save_protected_plaintext`
/// on/alongside the opened write handle (no TOCTOU gap relying on a stale prior probe).
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Advisory query of current backing medium protection status.
    async fn protection(&self) -> Result<StorageProtection, SecretStoreError>;

    /// Load the persisted secret store envelope and the medium protection observed when read.
    async fn load(&self) -> Result<Option<LoadedSecretRecord>, SecretStoreError>;

    /// Persist plaintext secret material ONLY if the medium is atomically verified
    /// owner-protected at write time; MUST return `SecretStoreError::UnprotectedMedium`
    /// without writing bytes when protection cannot be guaranteed.
    async fn save_protected_plaintext(&self, bytes: &[u8]) -> Result<(), SecretStoreError>;

    /// Persist authenticated ciphertext envelope sealed by `SecretCipher`.
    async fn save_encrypted(&self, bytes: &[u8]) -> Result<(), SecretStoreError>;
}

/// Failure modes for OS-credential-backed envelope encryption.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum SecretCipherError {
    /// OS credential store or keyring is unavailable.
    #[snafu(display("OS credential store unavailable: {reason}"))]
    Unavailable {
        /// Safe diagnostic reason.
        reason: String,
    },
    /// Ciphertext failed authentication/integrity check or decryption failed.
    #[snafu(display("secret decryption or integrity check failed: {reason}"))]
    Crypto {
        /// Safe non-secret diagnostic reason.
        reason: String,
    },
}

/// Narrow port for sealing and opening secret store envelopes using a key held in an OS
/// credential store when filesystem ACL protection is unavailable or downgraded.
#[async_trait]
pub trait SecretCipher: Send + Sync {
    /// Encrypt and authenticate `plaintext` bound to associated data `aad`.
    async fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SecretCipherError>;

    /// Verify and decrypt `ciphertext` bound to associated data `aad`.
    async fn open(&self, ciphertext: &[u8], aad: &[u8]) -> Result<SecretBytes, SecretCipherError>;
}
