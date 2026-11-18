//! Caller identity values; transport adapters assign and parse the labels.

use std::{fmt, str::FromStr};

use snafu::Snafu;

/// The longest accepted client nonce.
pub const MAX_CLIENT_NONCE_LEN: usize = 64;

/// A client-chosen identifier that separates concurrent clients of one HTTP principal.
///
/// It is one to 64 characters of `[A-Za-z0-9_-]`, so it can never contain the `:` separator of
/// an owner label. An omitted header is [`ClientNonce::default`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientNonce(String);

/// The text is not a valid client nonce.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("a client id is 1 to 64 characters of letters, digits, '_' or '-'"))]
pub struct ClientNonceError;

impl ClientNonce {
    const DEFAULT: &'static str = "default";

    /// Validates untrusted text.
    ///
    /// # Errors
    /// Returns an error when the text is empty, too long or has a character outside
    /// `[A-Za-z0-9_-]`.
    pub fn parse(text: &str) -> Result<Self, ClientNonceError> {
        let valid = (1..=MAX_CLIENT_NONCE_LEN).contains(&text.len())
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ClientNonceError)
        }
    }

    /// Borrow the validated text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ClientNonce {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl fmt::Display for ClientNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ClientNonce {
    type Err = ClientNonceError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// An opaque caller label assigned by the transport that owns the resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcOwner(String);

impl RpcOwner {
    /// Construct an owner from a label assigned by a transport adapter.
    pub fn new(label: String) -> Self {
        Self(label)
    }

    /// Retain the invoking desktop window label as its resource owner.
    pub fn desktop(label: &str) -> Self {
        Self(label.to_owned())
    }

    /// One server-assigned connection, namespaced by a fresh server instance.
    /// Neither component comes from request JSON; reconnects receive a new owner.
    pub fn local_connection(instance: &str, connection: u64) -> Self {
        Self(format!("local:{instance}:{connection}"))
    }

    /// One HTTP client of an authenticated or anonymous principal.
    ///
    /// `principal` is the server-resolved audit label of the caller; it is never taken from the
    /// request, so a client cannot enter another principal's namespace. `nonce` is the validated
    /// client-chosen identifier that keeps concurrent clients of one principal independent.
    pub fn http(principal: &str, nonce: &ClientNonce) -> Self {
        Self(format!("http:{principal}:{nonce}"))
    }

    /// Borrow the resource owner label without changing its transport identity.
    pub fn label(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_owners_are_namespaced_by_the_server_resolved_principal() {
        let nonce = ClientNonce::parse("tab-1_A").unwrap();
        assert_eq!(
            RpcOwner::http("token:a1b2", &nonce).label(),
            "http:token:a1b2:tab-1_A"
        );
        assert_ne!(
            RpcOwner::http("token:a1b2", &nonce),
            RpcOwner::http("session:a1b2", &nonce)
        );
        assert_eq!(
            RpcOwner::http("anonymous", &ClientNonce::default()).label(),
            "http:anonymous:default"
        );
    }

    #[test]
    fn a_nonce_cannot_carry_the_separator_or_exceed_its_bound() {
        assert!(ClientNonce::parse(&"a".repeat(MAX_CLIENT_NONCE_LEN)).is_ok());
        for bad in [
            "",
            "token:other",
            "a b",
            "a/b",
            "é",
            &"a".repeat(MAX_CLIENT_NONCE_LEN + 1),
        ] {
            assert_eq!(ClientNonce::parse(bad), Err(ClientNonceError), "{bad:?}");
        }
    }
}
