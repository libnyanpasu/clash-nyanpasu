//! Persisted remote-access settings.
//!
//! The section is owned by the application config but is never part of an ordinary
//! preferences patch: it widens who can reach the application, so it is written only
//! through a dedicated command that is not exposed over HTTP.

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

/// How the HTTP listener decides who may call it.
///
/// Further strategies join this enum in the change that implements their complete
/// request, scope and transport checks. Until then a stored value cannot name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpAccessStrategy {
    /// Loopback only; every request must carry the per-start credential.
    #[default]
    LocalAuthenticated,
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
}

impl Default for HttpAccessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: HttpAccessStrategy::default(),
            port: None,
            serve_webui: true,
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
    fn strategies_that_are_not_delivered_cannot_be_stored() {
        for strategy in ["local_open", "lan_authenticated"] {
            let document = format!(r#"{{"http":{{"strategy":"{strategy}"}}}}"#);
            assert!(serde_json::from_str::<RemoteAccessConfig>(&document).is_err());
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
