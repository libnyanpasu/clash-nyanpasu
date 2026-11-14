//! Zeroizing, non-serializable redacted secret wrappers.

use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Zeroizing wrapper for raw bearer tokens, session cookies, launch codes, and private keys.
///
/// Intentionally omits `Display`, `Serialize`, `Deserialize`, and `PartialEq` so secret
/// material cannot leak into DTOs, logs, or non-constant-time comparisons.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap sensitive text that will be zeroized when dropped.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the contained secret slice for hashing, transmission, or sealed persistence.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([REDACTED])")
    }
}

/// Zeroizing wrapper for raw cryptographic byte secrets and sealed blobs.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Wrap sensitive bytes that will be zeroized when dropped.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Borrow the underlying sensitive bytes.
    #[must_use]
    pub fn expose_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes([REDACTED])")
    }
}

/// SHA-256 digest of a bearer token, session cookie, or one-time launch code.
///
/// Even though digests are one-way hashes, omitting `Display` and `Serialize` prevents
/// accidental inclusion in public status/list responses.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretHash([u8; 32]);

impl SecretHash {
    /// Wrap a 32-byte SHA-256 digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Access the fixed 32-byte digest for constant-time comparison or storage encoding.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretHash([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::{SecretBytes, SecretHash, SecretString};

    #[test]
    fn secret_types_redact_debug_output() {
        let s = SecretString::new("npk_0123456789abcdef_secretvalue");
        let b = SecretBytes::new(b"top-secret-key-bytes".to_vec());
        let h = SecretHash::from_bytes([0xAB; 32]);

        let dbg_s = format!("{s:?}");
        let dbg_b = format!("{b:?}");
        let dbg_h = format!("{h:?}");

        assert!(!dbg_s.contains("secretvalue"));
        assert!(!dbg_s.contains("0123456789abcdef"));
        assert_eq!(dbg_s, "SecretString([REDACTED])");
        assert!(!dbg_b.contains("top-secret"));
        assert_eq!(dbg_b, "SecretBytes([REDACTED])");
        assert!(!dbg_h.contains("171"));
        assert_eq!(dbg_h, "SecretHash([REDACTED])");
    }
}
