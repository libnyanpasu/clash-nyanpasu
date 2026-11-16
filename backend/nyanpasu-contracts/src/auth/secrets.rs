//! Zeroizing, non-serializable redacted secret wrappers.

use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Zeroizing wrapper for raw bearer tokens, session cookies and launch codes.
///
/// Intentionally omits `Display`, `Serialize`, `Deserialize` and `PartialEq` so secret
/// material cannot leak into DTOs, logs or non-constant-time comparisons.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap sensitive text that will be zeroized when dropped.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the contained secret for hashing or transmission.
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

/// SHA-256 digest of a bearer token, session cookie or launch code.
///
/// Omitting `Display` and `Serialize` keeps the digest out of public status and list
/// responses.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretHash([u8; 32]);

impl SecretHash {
    /// Wrap a 32-byte SHA-256 digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest, for constant-time comparison or storage encoding.
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
    use super::{SecretHash, SecretString};

    #[test]
    fn secret_types_redact_debug_output() {
        let secret = SecretString::new("npk_0123456789abcdef_secretvalue");
        let hash = SecretHash::from_bytes([0xAB; 32]);

        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
        assert_eq!(format!("{hash:?}"), "SecretHash([REDACTED])");
    }
}
