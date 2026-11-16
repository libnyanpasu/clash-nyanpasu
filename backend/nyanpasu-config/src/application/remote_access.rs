//! Persisted remote-access settings.
//!
//! The section is owned by the application config but is never part of an ordinary
//! preferences patch: it widens who can reach the application, so it is written only
//! through a dedicated command that is not exposed over HTTP.

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

use super::Scope;

/// How the HTTP listener decides who may call it.
///
/// Further strategies join this enum in the change that implements their complete
/// request, scope and transport checks. Until then a stored value cannot name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpAccessStrategy {
    /// Loopback only; every request must carry a valid credential.
    #[default]
    LocalAuthenticated,
    /// Loopback only; a request without a credential is an anonymous caller holding
    /// [`HttpAccessConfig::local_open_scopes`]. Every local process of every OS user can
    /// reach it, because loopback TCP is not user-scoped.
    LocalOpen,
}

/// HTTP listener settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct HttpAccessConfig {
    /// Master switch. Off by default.
    pub enabled: bool,
    /// Kept while the switch is off, so turning it on restores the earlier choice.
    pub strategy: HttpAccessStrategy,
    /// `None` until the first start binds a free port and records it. Later starts
    /// reuse the recorded port, so URLs stay valid across restarts.
    pub port: Option<u16>,
    /// Serve the bundled web UI. When false only the API is served.
    pub serve_webui: bool,
    /// What an anonymous caller holds under [`HttpAccessStrategy::LocalOpen`]. Kept while
    /// another strategy is selected.
    pub local_open_scopes: Vec<Scope>,
}

impl Default for HttpAccessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: HttpAccessStrategy::default(),
            port: None,
            serve_webui: true,
            local_open_scopes: vec![Scope::Read],
        }
    }
}

/// The persisted `remote_access` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct RemoteAccessConfig {
    pub http: HttpAccessConfig,
}

/// Why a remote-access value cannot be committed.
#[derive(Debug, Clone, PartialEq, Eq, Snafu, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteAccessError {
    /// Port 0 asks the OS for any port; the persisted port is always a concrete one.
    #[snafu(display("the HTTP port must be between 1 and 65535"))]
    PortZero,
    /// An anonymous caller must hold at least one scope; `local_open` with none admits nothing.
    #[snafu(display("local_open_scopes must name at least one scope"))]
    LocalOpenScopesEmpty,
    /// An anonymous caller never holds secrets, system control or remote administration.
    #[snafu(display("local_open_scopes cannot include {scope}"))]
    LocalOpenScopeForbidden {
        /// The scope that anonymous callers can never hold.
        scope: Scope,
    },
    /// The section is changed only by the dedicated remote-access command, never by a
    /// general application patch.
    UseDedicatedCommand,
}

impl RemoteAccessConfig {
    /// Checks a candidate before it is committed.
    ///
    /// # Errors
    /// Returns the first invalid field.
    pub fn validate(&self) -> Result<(), RemoteAccessError> {
        if self.http.port == Some(0) {
            return Err(RemoteAccessError::PortZero);
        }
        if self.http.local_open_scopes.is_empty() {
            return Err(RemoteAccessError::LocalOpenScopesEmpty);
        }
        if let Some(&scope) = self.http.local_open_scopes.iter().find(|scope| {
            matches!(
                scope,
                Scope::SensitiveRead | Scope::System | Scope::RemoteAdmin
            )
        }) {
            return Err(RemoteAccessError::LocalOpenScopeForbidden { scope });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_is_off_and_keeps_todays_strategy() {
        let config = RemoteAccessConfig::default();
        assert!(!config.http.enabled);
        assert_eq!(config.http.strategy, HttpAccessStrategy::LocalAuthenticated);
        assert_eq!(config.http.port, None);
        assert!(config.http.serve_webui);
        assert_eq!(config.http.local_open_scopes, vec![Scope::Read]);
    }

    #[test]
    fn older_documents_decode_with_defaults() {
        let config: RemoteAccessConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, RemoteAccessConfig::default());
        let partial: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"enabled":true}}"#).unwrap();
        assert!(partial.http.enabled);
        assert!(partial.http.serve_webui);
    }

    #[test]
    fn local_open_is_stored_but_strategies_that_are_not_delivered_are_not() {
        let open: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"strategy":"local_open"}}"#).unwrap();
        assert_eq!(open.http.strategy, HttpAccessStrategy::LocalOpen);
        assert_eq!(open.http.local_open_scopes, vec![Scope::Read]);
        let lan = r#"{"http":{"strategy":"lan_authenticated"}}"#;
        assert!(serde_json::from_str::<RemoteAccessConfig>(lan).is_err());
    }

    #[test]
    fn local_open_scopes_are_validated_before_commit() {
        let mut config = RemoteAccessConfig::default();
        config.http.local_open_scopes = vec![Scope::Read, Scope::Proxy, Scope::Profile];
        assert_eq!(config.validate(), Ok(()));

        config.http.local_open_scopes = Vec::new();
        assert_eq!(
            config.validate(),
            Err(RemoteAccessError::LocalOpenScopesEmpty)
        );

        for scope in [Scope::SensitiveRead, Scope::System, Scope::RemoteAdmin] {
            config.http.local_open_scopes = vec![Scope::Read, scope];
            assert_eq!(
                config.validate(),
                Err(RemoteAccessError::LocalOpenScopeForbidden { scope })
            );
        }
    }

    #[test]
    fn port_zero_is_rejected_before_commit() {
        let mut config = RemoteAccessConfig::default();
        config.http.port = Some(0);
        assert_eq!(config.validate(), Err(RemoteAccessError::PortZero));
        config.http.port = Some(8765);
        assert_eq!(config.validate(), Ok(()));
    }
}
