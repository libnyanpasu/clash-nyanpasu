//! Validated remote-access identifiers, issuance payloads, resource leases, and revocations.

use std::{fmt, net::IpAddr};

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use tokio_util::sync::CancellationToken;

use super::{AuthContext, Principal, ScopeSet, secrets::SecretString};

/// Maximum length in bytes of a public token, session, or resource-session ID.
pub const MAX_AUTH_ID_LEN: usize = 64;
/// Maximum length in bytes of a user-assigned token label.
pub const MAX_TOKEN_NAME_LEN: usize = 64;
/// Maximum length in bytes of a resource owner correlation identifier.
pub const MAX_RESOURCE_OWNER_ID_LEN: usize = 96;
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

impl From<SessionId> for String {
    fn from(value: SessionId) -> Self {
        value.0
    }
}

/// Validated public identifier for an HTTP/stream resource session.
///
/// Distinct from `SessionId` and `Principal`: multiple connections or browser tabs
/// sharing one token or local UID allocate separate resource sessions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceSessionId(String);

impl ResourceSessionId {
    /// Construct and validate a resource session identifier.
    pub fn parse(raw: impl Into<String>) -> Result<Self, AuthValidationError> {
        let value = raw.into();
        validate_ascii_id(&value, "resource_session_id")?;
        Ok(Self(value))
    }

    /// Borrow the resource session ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ResourceSessionId {
    type Error = AuthValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ResourceSessionId> for String {
    fn from(value: ResourceSessionId) -> Self {
        value.0
    }
}

/// Transport-neutral server-side resource ownership key used to tie streams and leases
/// to a single connection/process lifetime without collapsing into `Principal`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceOwnerId(String);

impl ResourceOwnerId {
    /// Construct and validate a resource owner key.
    pub fn parse(raw: impl Into<String>) -> Result<Self, AuthValidationError> {
        let value = raw.into();
        if value.is_empty() || value.len() > MAX_RESOURCE_OWNER_ID_LEN {
            return Err(AuthValidationError::InvalidLength {
                field: "resource_owner_id",
                max: MAX_RESOURCE_OWNER_ID_LEN,
            });
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b':')
        {
            return Err(AuthValidationError::InvalidCharacters {
                field: "resource_owner_id",
            });
        }
        Ok(Self(value))
    }

    /// Borrow the resource owner key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceOwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ResourceOwnerId {
    type Error = AuthValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ResourceOwnerId> for String {
    fn from(value: ResourceOwnerId) -> Self {
        value.0
    }
}

/// Bounded peer identity key for per-peer authentication rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthPeer {
    /// Canonical remote IP address of the authentication caller.
    pub ip: IpAddr,
}

impl AuthPeer {
    /// Key rate limiting by canonical peer IP address.
    #[must_use]
    pub const fn from_ip(ip: IpAddr) -> Self {
        Self { ip }
    }
}

/// Lifecycle state reported for a named access token (non-secret).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Monotonic idle expiration deadline in milliseconds (`AuthClock::monotonic_ms`).
    pub idle_expires_at_monotonic_ms: u64,
    /// Optional wall-clock hard expiration tied to a backing token, if any.
    pub token_expires_at_ms: Option<i64>,
}

/// Result of establishing a new browser session via launch code exchange or token login.
pub struct EstablishedBrowserSession {
    /// Public non-secret metadata for inspection/revocation.
    pub summary: BrowserSessionSummary,
    /// Authoritative resolved caller identity and independent `ScopeSet`.
    pub auth: AuthContext,
    /// Monotonic owner generation epoch for admission revalidation.
    pub auth_generation: u64,
    /// Secret HttpOnly session cookie credential (`nps_<session_id>_<256bit_hex_secret>`).
    pub cookie: SecretString,
}

impl fmt::Debug for EstablishedBrowserSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EstablishedBrowserSession")
            .field("summary", &self.summary)
            .field("auth", &self.auth)
            .field("auth_generation", &self.auth_generation)
            .field("cookie", &self.cookie)
            .finish()
    }
}

/// Owner-bound cancellation handle for server-side streams and resource allocations.
///
/// Separate from business-operation cancellation: credential revocation cancels this
/// lease so active streams/resources close promptly while already-admitted in-flight
/// business operations continue to completion with their captured `AuthContext`.
#[derive(Debug, Clone)]
pub struct ResourceLease {
    /// Resource owner key associated with this lease.
    pub owner_id: ResourceOwnerId,
    cancellation: CancellationToken,
}

impl ResourceLease {
    /// Create a new resource lease wrapping a dedicated invalidation token.
    #[must_use]
    pub fn new(owner_id: ResourceOwnerId, cancellation: CancellationToken) -> Self {
        Self {
            owner_id,
            cancellation,
        }
    }

    /// Borrow the invalidation token for owned streams/resources.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Whether the resource owner has been invalidated or expired.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

/// Resolved caller context returned when verifying a credential or revalidating admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedCaller {
    /// Resolved non-secret principal and independent scope grants.
    pub auth: AuthContext,
    /// Optional wall-clock hard expiration in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
    /// Owner credential epoch at issuance/verification time, checked by `admit_caller`.
    pub auth_generation: u64,
}

/// Result of allocating a principal-bound HTTP/stream resource session.
///
/// The returned `cookie` authorizes only attachment to `owner_id` for the same principal
/// and NEVER grants authentication or scopes.
pub struct EstablishedResourceSession {
    /// Non-secret resource session identifier.
    pub id: ResourceSessionId,
    /// Non-secret resource owner correlation key passed to RPC resource APIs.
    pub owner_id: ResourceOwnerId,
    /// Principal that owns this resource session (for audit).
    pub principal: Principal,
    /// Invalidation lease cancelled when this resource session or its principal is revoked.
    pub lease: ResourceLease,
    /// Secret resource cookie (`npr_<resource_session_id>_<256bit_hex_secret>`).
    pub cookie: SecretString,
}

impl fmt::Debug for EstablishedResourceSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EstablishedResourceSession")
            .field("id", &self.id)
            .field("owner_id", &self.owner_id)
            .field("principal", &self.principal)
            .field("lease", &self.lease)
            .field("cookie", &self.cookie)
            .finish()
    }
}

/// Non-authenticating resolution result for a resource cookie.
///
/// Intentionally carries NO `AuthContext` and NO `ScopeSet` so a transport adapter cannot
/// accidentally promote a resource cookie into an authenticated principal.
#[derive(Debug, Clone)]
pub struct ResolvedResourceSession {
    /// Non-secret resource session identifier.
    pub id: ResourceSessionId,
    /// Resource owner key identifying server-side owned resources.
    pub owner_id: ResourceOwnerId,
    /// Principal that owns this resource session.
    pub principal: Principal,
    /// Invalidation lease cancelled when revoked/expired/released.
    pub lease: ResourceLease,
}

/// Revocation or expiration event emitted by the auth owner for transport resource cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthRevocation {
    /// Named access token and all derived sessions/resources were revoked or expired.
    Token {
        /// Public ID of the invalidated token.
        token_id: TokenId,
        /// Every active resource owner key bound to that token or its sessions.
        resource_owners: Vec<ResourceOwnerId>,
    },
    /// Browser session and its bound resources were logged out, expired, or evicted.
    Session {
        /// Public ID of the invalidated browser session.
        session_id: SessionId,
        /// Every active resource owner key bound to that session.
        resource_owners: Vec<ResourceOwnerId>,
    },
    /// Single resource session was explicitly released or evicted.
    ResourceSession {
        /// Public ID of the invalidated resource session.
        resource_session_id: ResourceSessionId,
        /// Resource owner key to tear down in RPC resource registries.
        owner_id: ResourceOwnerId,
    },
}

/// Persisted TLS certificate/key contract stored exclusively via protected persistence.
///
/// Omits `Serialize` and redacts `Debug` so the private key is never exposed in DTOs or logs.
#[derive(Clone)]
pub struct TlsMaterialBlob {
    /// PEM-encoded certificate chain.
    pub cert_pem: String,
    /// PEM-encoded private key wrapped in zeroizing storage.
    pub private_key_pem: SecretString,
    /// SHA-256 fingerprint of the certificate (public, hex lowercase).
    pub fingerprint_sha256: String,
    /// Optional certificate expiration timestamp in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
}

impl fmt::Debug for TlsMaterialBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsMaterialBlob")
            .field("fingerprint_sha256", &self.fingerprint_sha256)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("private_key_pem", &self.private_key_pem)
            .finish()
    }
}

/// Public non-secret status summary for persisted TLS material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsMaterialSummary {
    /// SHA-256 certificate fingerprint (hex lowercase).
    pub fingerprint_sha256: String,
    /// Optional certificate expiration timestamp in Unix epoch milliseconds.
    pub expires_at_ms: Option<i64>,
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

/// Parse `npr_<resource_session_id>_<64_hex>` resource cookie syntax strictly.
pub fn parse_resource_cookie(raw: &str) -> Option<ResourceSessionId> {
    let rest = raw.strip_prefix("npr_")?;
    let (id_part, secret_hex) = rest.rsplit_once('_')?;
    if !is_lower_hex_64(secret_hex) {
        return None;
    }
    ResourceSessionId::parse(id_part).ok()
}

/// Validate `npl_<64_hex>` single-use launch code syntax strictly.
#[must_use]
pub fn is_valid_launch_code_syntax(raw: &str) -> bool {
    raw.strip_prefix("npl_").is_some_and(is_lower_hex_64)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_AUTH_ID_LEN, MAX_TOKEN_NAME_LEN, ResourceOwnerId, ResourceSessionId, SessionId,
        TokenId, TokenName, is_valid_launch_code_syntax, parse_access_token, parse_resource_cookie,
        parse_session_cookie,
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
        assert!(ResourceSessionId::parse("res_0123456789abcdef").is_ok());
        assert!(ResourceOwnerId::parse("http_res:res_0123456789abcdef").is_ok());
        assert!(ResourceOwnerId::parse("").is_err());
        assert!(ResourceOwnerId::parse("bad/slash").is_err());
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

        let res_cookie = format!("npr_res_0123456789abcdef_{hex64}");
        assert_eq!(
            parse_resource_cookie(&res_cookie).unwrap().as_str(),
            "res_0123456789abcdef"
        );
        assert!(parse_resource_cookie(&sess_cookie).is_none());

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
}
