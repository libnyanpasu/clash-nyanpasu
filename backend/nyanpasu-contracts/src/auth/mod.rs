//! Transport-neutral caller principals, grants, authorization contexts, and auth ports.

pub mod certificate;
pub mod grant;
pub mod ports;
pub mod secrets;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod tls;
pub mod types;

pub use certificate::{RemoteAccessCertificates, TlsCertificateDetails, TlsCertificateParseError};
pub use grant::{Grant, GrantError};
/// The permission model, defined beside the configuration that persists it.
pub use nyanpasu_config::application::{Access, Permission, Permissions, Resource, Role};
pub use ports::{AuthClock, EntropyError, EntropySource, SecretStore, SecretStoreError};
pub use secrets::{SecretHash, SecretString};
use serde::{Deserialize, Serialize};
use specta::Type;
pub use tls::{
    TlsAuthority, TlsCertificateGenerator, TlsCertificateRequest, TlsCertificateSummary,
    TlsFingerprint, TlsFingerprintError, TlsGenerateError, TlsIdentity,
};
pub use types::{
    AccessTokenStatus, AccessTokenSummary, AuthRevocation, AuthValidationError,
    AuthenticatedCaller, BrowserSessionSummary, CreateAccessTokenRequest,
    EstablishedBrowserSession, IssuedAccessToken, IssuedLaunchCode, MAX_AUTH_ID_LEN,
    MAX_TOKEN_NAME_LEN, ParsedAccessToken, RemoteAuthPolicy, SECRET_HEX_LEN, SessionId, TokenId,
    TokenName, is_valid_launch_code_syntax, parse_access_token, parse_session_cookie,
};

/// Public identity of the resolved caller attached at transport admission.
///
/// Contains only non-secret identity labels and identifiers suitable for audit
/// attribution and self-inspection. Never carries raw secrets, bearer tokens,
/// session cookies, or unhashed credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal {
    /// Native Tauri desktop shell webview window.
    Desktop {
        /// Non-secret desktop window label.
        window: String,
    },
    /// Authenticated caller via the same-user local IPC socket or named pipe.
    LocalSocket {
        /// Opaque host user security identity (Unix UID decimal string or Windows SID).
        user: String,
    },
    /// Authenticated browser or bootstrap HTTP session.
    Session {
        /// Public non-secret session correlation identifier (never a cookie or access token).
        id: String,
    },
    /// Bearer token identified by its public ID and user-assigned label.
    Token {
        /// Public non-secret token identifier.
        id: String,
        /// User-assigned token label.
        name: String,
    },
    /// Unauthenticated remote caller allowed under an explicit anonymous policy.
    Anonymous,
}

impl Principal {
    /// Non-secret audit-safe label for this principal.
    pub fn audit_label(&self) -> String {
        match self {
            Self::Desktop { window } => format!("desktop:{window}"),
            Self::LocalSocket { user } => format!("local_socket:{user}"),
            Self::Session { id } => format!("session:{id}"),
            Self::Token { id, .. } => format!("token:{id}"),
            Self::Anonymous => "anonymous".to_owned(),
        }
    }
}

/// The authentication result: who is calling, and the permissions their grant expands to.
///
/// Admission reads only [`AuthContext::permissions`]. Separate from `RpcOwner`, which keys
/// server-side resource lifetimes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuthContext {
    pub principal: Principal,
    /// The role the permissions expand from; `None` for a custom permission map. Only
    /// discovery and the user interface read it.
    pub role: Option<Role>,
    pub permissions: Permissions,
}

impl AuthContext {
    /// A caller holding `grant`, expanded once here.
    pub fn new(principal: Principal, grant: Grant) -> Self {
        Self {
            principal,
            role: grant.role(),
            permissions: grant.permissions(),
        }
    }

    /// The desktop window: the owner.
    pub fn desktop(window: impl Into<String>) -> Self {
        Self::new(
            Principal::Desktop {
                window: window.into(),
            },
            Grant::Role(Role::Owner),
        )
    }

    /// A same-user local socket peer: the owner.
    pub fn local_socket(user: impl Into<String>) -> Self {
        Self::new(
            Principal::LocalSocket { user: user.into() },
            Grant::Role(Role::Owner),
        )
    }

    /// An HTTP request without a credential: the `anonymous` role, which holds only capability
    /// discovery, plus the configured `additional` permissions.
    pub fn anonymous(additional: Permissions) -> Self {
        Self {
            principal: Principal::Anonymous,
            role: Some(Role::Anonymous),
            permissions: additional,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_caller_construction_point_holds_its_role() {
        for auth in [
            AuthContext::desktop("main"),
            AuthContext::local_socket("S-1-5-21-1000"),
        ] {
            assert_eq!(auth.role, Some(Role::Owner));
            assert_eq!(auth.permissions, Role::Owner.permissions());
        }
        let anonymous = AuthContext::anonymous(Permissions::NONE);
        assert_eq!(anonymous.principal, Principal::Anonymous);
        assert_eq!(anonymous.role, Some(Role::Anonymous));
        assert_eq!(anonymous.permissions, Permissions::NONE);

        let custom = Permissions::NONE.with(
            nyanpasu_config::application::Resource::Proxy,
            nyanpasu_config::application::Access::Write,
        );
        let token = AuthContext::new(
            Principal::Token {
                id: "tok_1".to_owned(),
                name: "agent".to_owned(),
            },
            Grant::Custom(custom),
        );
        assert_eq!(token.role, None);
        assert_eq!(token.permissions, custom);
    }

    #[test]
    fn audit_labels_stay_non_secret() {
        assert_eq!(
            Principal::Desktop {
                window: "main".to_owned()
            }
            .audit_label(),
            "desktop:main"
        );
        assert_eq!(
            Principal::LocalSocket {
                user: "1000".to_owned()
            }
            .audit_label(),
            "local_socket:1000"
        );
        assert_eq!(
            Principal::Session {
                id: "sess_public_1".to_owned()
            }
            .audit_label(),
            "session:sess_public_1"
        );
        assert_eq!(
            Principal::Token {
                id: "tok_1".to_owned(),
                name: "my secret name".to_owned(),
            }
            .audit_label(),
            "token:tok_1"
        );
        assert_eq!(Principal::Anonymous.audit_label(), "anonymous");
    }

    #[test]
    fn principal_variants_serialize_with_tagged_kind_for_discovery() {
        let desktop = Principal::Desktop {
            window: "main".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&desktop).unwrap(),
            serde_json::json!({
                "kind": "desktop",
                "window": "main"
            })
        );

        let socket = Principal::LocalSocket {
            user: "S-1-5-21-1000".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&socket).unwrap(),
            serde_json::json!({
                "kind": "local_socket",
                "user": "S-1-5-21-1000"
            })
        );

        let session = Principal::Session {
            id: "sess_01J".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&session).unwrap(),
            serde_json::json!({
                "kind": "session",
                "id": "sess_01J"
            })
        );

        let token = Principal::Token {
            id: "t_01J".to_owned(),
            name: "skill-cli".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&token).unwrap(),
            serde_json::json!({
                "kind": "token",
                "id": "t_01J",
                "name": "skill-cli"
            })
        );
    }
}
