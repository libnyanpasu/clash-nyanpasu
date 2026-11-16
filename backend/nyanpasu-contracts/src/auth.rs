//! Transport-neutral caller principals, scopes, and authorization contexts.

use std::collections::BTreeSet;

pub use nyanpasu_config::application::Scope;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Canonical deduplicated set of independent permission scopes held by a caller.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ScopeSet(BTreeSet<Scope>);

impl ScopeSet {
    /// An empty scope set holding no permissions.
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    /// Every declared application scope.
    pub fn full() -> Self {
        Self(Scope::ALL.into_iter().collect())
    }

    /// Whether this set contains the requested scope.
    pub fn contains(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
    }

    /// Whether no scopes are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of distinct scopes in the set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterate over held scopes in canonical `Scope::ALL` declaration order.
    pub fn iter(&self) -> impl Iterator<Item = Scope> + '_ {
        self.0.iter().copied()
    }

    /// Materialize held scopes in canonical `Scope::ALL` order.
    pub fn to_vec(&self) -> Vec<Scope> {
        self.iter().collect()
    }
}

impl FromIterator<Scope> for ScopeSet {
    fn from_iter<T: IntoIterator<Item = Scope>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<const N: usize> From<[Scope; N]> for ScopeSet {
    fn from(scopes: [Scope; N]) -> Self {
        scopes.into_iter().collect()
    }
}

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
    /// Scoped bearer token identified by its public ID and user-assigned label.
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

/// Resolved authorization identity and effective permission set for a call or stream.
///
/// Separate from `RpcOwner`, which keys server-side resource lifetimes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuthContext {
    pub principal: Principal,
    pub scopes: ScopeSet,
}

impl AuthContext {
    /// Construct an authorization context from a resolved principal and scope set.
    pub fn new(principal: Principal, scopes: ScopeSet) -> Self {
        Self { principal, scopes }
    }

    /// Native desktop shell context holding every declared scope for `window`.
    pub fn desktop(window: impl Into<String>) -> Self {
        Self::new(
            Principal::Desktop {
                window: window.into(),
            },
            ScopeSet::full(),
        )
    }

    /// Same-user local socket peer context holding every declared scope.
    pub fn local_socket(user: impl Into<String>) -> Self {
        Self::new(
            Principal::LocalSocket { user: user.into() },
            ScopeSet::full(),
        )
    }

    /// Authenticated HTTP session holding every declared scope, identified by a
    /// non-secret session correlation ID.
    pub fn session(id: impl Into<String>) -> Self {
        Self::new(Principal::Session { id: id.into() }, ScopeSet::full())
    }

    /// Whether the caller holds `scope`. Scopes are independent; no implication applies.
    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(scope)
    }

    /// Whether the caller holds `Scope::SensitiveRead` and may view unprojected
    /// paths, URLs, host secrets, and raw error details.
    pub fn has_sensitive_read(&self) -> bool {
        self.has_scope(Scope::SensitiveRead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_set_deduplicates_and_orders_canonically() {
        let scopes: ScopeSet = [Scope::System, Scope::Read, Scope::Proxy, Scope::Read]
            .into_iter()
            .collect();
        assert_eq!(scopes.len(), 3);
        assert_eq!(
            scopes.to_vec(),
            vec![Scope::Read, Scope::Proxy, Scope::System]
        );
        let json = serde_json::to_value(&scopes).unwrap();
        assert_eq!(json, serde_json::json!(["read", "proxy", "system"]));
    }

    #[test]
    fn scopes_are_independent_with_no_implied_grants() {
        let proxy_only = AuthContext::new(Principal::Anonymous, ScopeSet::from([Scope::Proxy]));
        assert!(proxy_only.has_scope(Scope::Proxy));
        assert!(!proxy_only.has_scope(Scope::Read));
        assert!(!proxy_only.has_sensitive_read());

        let sensitive_only = AuthContext::new(
            Principal::Token {
                id: "tok_1".to_owned(),
                name: "reader".to_owned(),
            },
            ScopeSet::from([Scope::SensitiveRead]),
        );
        assert!(sensitive_only.has_sensitive_read());
        assert!(!sensitive_only.has_scope(Scope::Read));
    }

    #[test]
    fn full_contexts_hold_every_scope_and_audit_labels_stay_non_secret() {
        for auth in [
            AuthContext::desktop("main"),
            AuthContext::local_socket("S-1-5-21-1000"),
            AuthContext::session("sess_public_1"),
        ] {
            for scope in Scope::ALL {
                assert!(auth.has_scope(scope));
            }
            assert!(auth.has_sensitive_read());
        }

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
