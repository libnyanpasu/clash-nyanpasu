//! Validated remote-access identifiers, issuance payloads, policy and revocations.

use std::fmt;

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

use super::{AuthContext, ScopeSet, secrets::SecretString};

/// Maximum length in bytes of a public token or session ID.
pub const MAX_AUTH_ID_LEN: usize = 64;
/// Maximum length in bytes of a user-assigned token label.
pub const MAX_TOKEN_NAME_LEN: usize = 64;
/// Expected byte length of 256-bit hex secrets.
pub const SECRET_HEX_LEN: usize = 64;

/// Validation error when constructing bounded authentication identifiers or metadata.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum AuthValidationError {
    /// Field was empty or exceeded its maximum allowed byte length.
    #[snafu(display("invalid {field} length (expected 1..={max} bytes)"))]
    InvalidLength {
        /// Field name for diagnostics.
        field: &'static str,
        /// Maximum permitted length in bytes.
        max: usize,
    },
    /// Field contained disallowed characters (control characters, whitespace, or non-identifier glyphs).
    #[snafu(display("invalid characters in {field}"))]
    InvalidCharacters {
        /// Field name for diagnostics.
        field: &'static str,
    },
}

fn validate_ascii_id(value: &str, field: &'static str) -> Result<(), AuthValidationError> {
    if value.is_empty() || value.len() > MAX_AUTH_ID_LEN {
        return Err(AuthValidationError::InvalidLength {
            field,
            max: MAX_AUTH_ID_LEN,
        });
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(AuthValidationError::InvalidCharacters { field });
    }
    Ok(())
}

/// Validated non-secret public identifier for a named access token.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TokenId(String);

impl TokenId {
    /// Construct and validate a public token identifier.
    pub fn parse(raw: impl Into<String>) -> Result<Self, AuthValidationError> {
        let value = raw.into();
        validate_ascii_id(&value, "token_id")?;
        Ok(Self(value))
    }

    /// Borrow the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TokenId {
    type Error = AuthValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl Type for TokenId {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

impl From<TokenId> for String {
    fn from(value: TokenId) -> Self {
        value.0
    }
}

/// Validated non-secret human label for a named access token.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TokenName(String);

impl TokenName {
    /// Construct and validate a bounded human-readable token label.
    ///
    /// Rejects empty strings, leading/trailing whitespace, control characters, and values
    /// exceeding `MAX_TOKEN_NAME_LEN` bytes.
    pub fn parse(raw: impl Into<String>) -> Result<Self, AuthValidationError> {
        let value = raw.into();
        if value.is_empty() || value.len() > MAX_TOKEN_NAME_LEN {
            return Err(AuthValidationError::InvalidLength {
                field: "token_name",
                max: MAX_TOKEN_NAME_LEN,
            });
        }
        if value.trim() != value || value.chars().any(char::is_control) {
            return Err(AuthValidationError::InvalidCharacters {
                field: "token_name",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the validated label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TokenName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TokenName {
    type Error = AuthValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl Type for TokenName {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

impl From<TokenName> for String {
    fn from(value: TokenName) -> Self {
        value.0
    }
}

/// Validated non-secret public correlation identifier for a browser/access session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(String);

impl SessionId {
    /// Construct and validate a public session identifier.
    pub fn parse(raw: impl Into<String>) -> Result<Self, AuthValidationError> {
        let value = raw.into();
        validate_ascii_id(&value, "session_id")?;
        Ok(Self(value))
    }

    /// Borrow the session ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for SessionId {
    type Error = AuthValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl Type for SessionId {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

impl From<SessionId> for String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}

/// Lifecycle state reported for a named access token (non-secret).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AccessTokenStatus {
    /// Token may authenticate and issue sessions until optional expiry.
    Active,
    /// Token has passed its `expires_at_ms` timestamp.
    Expired,
}

/// Non-secret metadata for a persisted access token.
///
/// Never contains the raw bearer token or its SHA-256 hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AccessTokenSummary {
    /// Public non-secret token identifier.
    pub id: TokenId,
    /// User-assigned display label.
    pub name: TokenName,
    /// Independent permissions granted to this token.
    pub scopes: ScopeSet,
    /// Wall-clock creation timestamp in Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Optional wall-clock expiration timestamp in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
    /// Wall-clock timestamp of last successful verification, if any.
    pub last_used_at_ms: Option<i64>,
    /// Current status at inspection time.
    pub status: AccessTokenStatus,
}

/// Trusted domain request to mint a new named access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAccessTokenRequest {
    /// User-assigned non-secret label for the token.
    pub name: TokenName,
    /// Independent scopes granted to the token.
    pub scopes: ScopeSet,
    /// Optional wall-clock expiration timestamp in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
}

/// Once-returned result of minting a named access token.
///
/// Contains the raw bearer token (`npk_<public_id>_<256bit_hex_secret>`) wrapped in
/// `SecretString`; subsequent list/status APIs never return this secret again.
pub struct IssuedAccessToken {
    /// Public non-secret metadata for the new token.
    pub summary: AccessTokenSummary,
    /// Raw bearer credential shown once at issuance time.
    pub token: SecretString,
}

impl fmt::Debug for IssuedAccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedAccessToken")
            .field("summary", &self.summary)
            .field("token", &self.token)
            .finish()
    }
}

/// Once-returned result of minting a 60-second single-use browser launch code.
///
/// The launch code is bound to trusted composition-supplied scopes rather than any
/// user-provided principal JSON.
pub struct IssuedLaunchCode {
    /// Single-use secret launch code shown once for handoff into the browser URL.
    pub code: SecretString,
    /// Exact independent scopes that will be granted to the resulting browser session.
    pub scopes: ScopeSet,
    /// Wall-clock issuance timestamp in Unix epoch milliseconds.
    pub issued_at_ms: i64,
    /// Wall-clock expiration timestamp in Unix epoch milliseconds (`issued_at_ms + 60_000`).
    pub expires_at_ms: i64,
}

impl fmt::Debug for IssuedLaunchCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedLaunchCode")
            .field("code", &self.code)
            .field("scopes", &self.scopes)
            .field("issued_at_ms", &self.issued_at_ms)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

/// Non-secret metadata for an active browser/access session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BrowserSessionSummary {
    /// Public non-secret session identifier (never the cookie).
    pub id: SessionId,
    /// Backing token ID when the session was established via `login_with_token`.
    pub backing_token_id: Option<TokenId>,
    /// Independent scopes held by this session.
    pub scopes: ScopeSet,
    /// Wall-clock creation timestamp in Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Wall-clock last-activity timestamp in Unix epoch milliseconds.
    pub last_active_at_ms: i64,
    /// Optional wall-clock hard expiration tied to a backing token, if any.
    pub token_expires_at_ms: Option<i64>,
}

/// Result of establishing a new browser session via launch code exchange or token login.
pub struct EstablishedBrowserSession {
    /// Public non-secret metadata for inspection/revocation.
    pub summary: BrowserSessionSummary,
    /// Authoritative resolved caller identity and independent `ScopeSet`.
    pub auth: AuthContext,
    /// Secret HttpOnly session cookie credential (`nps_<session_id>_<256bit_hex_secret>`).
    pub cookie: SecretString,
}

impl fmt::Debug for EstablishedBrowserSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EstablishedBrowserSession")
            .field("summary", &self.summary)
            .field("auth", &self.auth)
            .field("cookie", &self.cookie)
            .finish()
    }
}

/// Resolved caller context returned when verifying a credential or revalidating admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedCaller {
    /// Resolved non-secret principal and independent scope grants.
    pub auth: AuthContext,
    /// Optional wall-clock hard expiration in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
}

/// Bounds and lifetimes of the authentication owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteAuthPolicy {
    /// Maximum persisted named access tokens.
    pub max_tokens: usize,
    /// How long a single-use launch code stays valid, in milliseconds.
    pub launch_code_ttl_ms: u64,
    /// How long a browser session may sit unused before it expires, in milliseconds.
    pub session_idle_ms: u64,
    /// Maximum open browser sessions.
    pub max_sessions: usize,
}

impl RemoteAuthPolicy {
    /// Whether every bound and lifetime is at least 1.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.max_tokens > 0
            && self.launch_code_ttl_ms > 0
            && self.session_idle_ms > 0
            && self.max_sessions > 0
    }
}

impl Default for RemoteAuthPolicy {
    fn default() -> Self {
        Self {
            max_tokens: 64,
            launch_code_ttl_ms: 60_000,
            session_idle_ms: 30 * 60 * 1000,
            max_sessions: 64,
        }
    }
}

/// Revocation or expiry announced by the auth owner so transports can end the streams of that
/// principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthRevocation {
    /// A named access token was revoked or expired, and with it the sessions it backed.
    Token {
        /// Public ID of the invalidated token.
        token_id: TokenId,
        /// Every browser session closed along with the token.
        session_ids: Vec<SessionId>,
    },
    /// A browser session was logged out, revoked or went idle.
    Session {
        /// Public ID of the invalidated browser session.
        session_id: SessionId,
    },
}

/// Parsed parts of a raw named bearer token (`npk_<token_id>_<64_lowercase_hex>`).
///
/// Omits derived `Debug` and manually redacts `secret_hex` so formatted parser results
/// never expose the raw 256-bit bearer secret.
#[derive(PartialEq, Eq)]
pub struct ParsedAccessToken<'a> {
    /// Extracted public token identifier.
    pub id: TokenId,
    /// Extracted 64-char lowercase hex secret portion (verified against full token hash too).
    pub secret_hex: &'a str,
}

impl fmt::Debug for ParsedAccessToken<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedAccessToken")
            .field("id", &self.id)
            .field("secret_hex", &"[REDACTED]")
            .finish()
    }
}

fn is_lower_hex_64(value: &str) -> bool {
    value.len() == SECRET_HEX_LEN
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Parse `npk_<token_id>_<64_hex>` syntax strictly without accepting malformed variants.
pub fn parse_access_token(raw: &str) -> Option<ParsedAccessToken<'_>> {
    let rest = raw.strip_prefix("npk_")?;
    let (id_part, secret_hex) = rest.rsplit_once('_')?;
    if !is_lower_hex_64(secret_hex) {
        return None;
    }
    let id = TokenId::parse(id_part).ok()?;
    Some(ParsedAccessToken { id, secret_hex })
}

/// Parse `nps_<session_id>_<64_hex>` browser cookie syntax strictly.
pub fn parse_session_cookie(raw: &str) -> Option<SessionId> {
    let rest = raw.strip_prefix("nps_")?;
    let (id_part, secret_hex) = rest.rsplit_once('_')?;
    if !is_lower_hex_64(secret_hex) {
        return None;
    }
    SessionId::parse(id_part).ok()
}

/// Validate `npl_<64_hex>` single-use launch code syntax strictly.
#[must_use]
pub fn is_valid_launch_code_syntax(raw: &str) -> bool {
    raw.strip_prefix("npl_").is_some_and(is_lower_hex_64)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_AUTH_ID_LEN, MAX_TOKEN_NAME_LEN, RemoteAuthPolicy, SessionId, TokenId, TokenName,
        is_valid_launch_code_syntax, parse_access_token, parse_session_cookie,
    };

    #[test]
    fn identifiers_enforce_bounds_and_character_rules() {
        assert!(TokenId::parse("").is_err());
        assert!(TokenId::parse("a".repeat(MAX_AUTH_ID_LEN + 1)).is_err());
        assert!(TokenId::parse("bad:id").is_err());
        assert!(TokenId::parse("0123456789abcdef").is_ok());

        assert!(TokenName::parse("").is_err());
        assert!(TokenName::parse(" leading").is_err());
        assert!(TokenName::parse("trailing ").is_err());
        assert!(TokenName::parse("line\nbreak").is_err());
        assert!(TokenName::parse("x".repeat(MAX_TOKEN_NAME_LEN + 1)).is_err());
        assert_eq!(
            TokenName::parse("claude-cli").unwrap().as_str(),
            "claude-cli"
        );

        assert!(SessionId::parse("sess_0123456789abcdef").is_ok());
    }

    #[test]
    fn credential_parsers_accept_canonical_wire_formats_and_reject_malformed() {
        let hex64 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let raw_tok = format!("npk_0123456789abcdef_{hex64}");
        let parsed = parse_access_token(&raw_tok).expect("valid token format");
        assert_eq!(parsed.id.as_str(), "0123456789abcdef");
        assert_eq!(parsed.secret_hex, hex64);

        // Uppercase hex or wrong length or bad prefix must be refused uniformly.
        assert!(parse_access_token(&raw_tok.to_uppercase()).is_none());
        assert!(parse_access_token(&format!("npk_0123456789abcdef_{}", &hex64[..62])).is_none());
        assert!(parse_access_token(&format!("nps_0123456789abcdef_{hex64}")).is_none());

        let sess_cookie = format!("nps_sess_0123456789abcdef_{hex64}");
        assert_eq!(
            parse_session_cookie(&sess_cookie).unwrap().as_str(),
            "sess_0123456789abcdef"
        );
        assert!(parse_session_cookie("nps_sess_short").is_none());

        assert!(is_valid_launch_code_syntax(&format!("npl_{hex64}")));
        assert!(!is_valid_launch_code_syntax("npl_short"));
    }

    #[test]
    fn deserializing_bounded_identifiers_rejects_oversized_inputs() {
        let oversized = serde_json::json!("x".repeat(MAX_AUTH_ID_LEN + 1));
        assert!(serde_json::from_value::<TokenId>(oversized).is_err());

        let bad_name = serde_json::json!(" padded ");
        assert!(serde_json::from_value::<TokenName>(bad_name).is_err());
    }

    #[test]
    fn parsed_access_token_debug_redacts_secret_hex() {
        let hex64 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let raw_tok = format!("npk_0123456789abcdef_{hex64}");
        let parsed = parse_access_token(&raw_tok);
        let dbg = format!("{parsed:?}");

        assert!(!dbg.contains(hex64));
        assert!(dbg.contains("0123456789abcdef"));
        assert!(dbg.contains("[REDACTED]"));
    }

    #[test]
    fn default_policy_is_valid_and_zero_bounds_are_not() {
        let policy = RemoteAuthPolicy::default();
        assert!(policy.is_valid());
        assert_eq!(policy.launch_code_ttl_ms, 60_000);
        assert_eq!(policy.session_idle_ms, 30 * 60 * 1000);
        assert!(
            !RemoteAuthPolicy {
                max_sessions: 0,
                ..policy
            }
            .is_valid()
        );
    }
}
