//! Persisted remote-access settings.
//!
//! The section is owned by the application config but is never part of an ordinary
//! preferences patch: it widens who can reach the application, so it is written only
//! through a dedicated command that is not exposed over HTTP.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

use super::{Access, Permissions, Resource, validate_anonymous_permissions};

/// How the HTTP listener decides who may call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpAccessStrategy {
    /// Loopback only; every request must carry a valid credential.
    #[default]
    LocalAuthenticated,
    /// Loopback only; a request without a credential is an anonymous caller holding capability
    /// discovery plus [`HttpAccessConfig::anonymous_permissions`]. Every local process of every
    /// OS user can reach it, because loopback TCP is not user-scoped.
    LocalOpen,
    /// Listens on the LAN over HTTPS only. Every request must carry a valid credential, also
    /// one from a loopback peer: a reverse proxy makes every request look local.
    LanAuthenticated,
}

/// Whether the listener speaks plain HTTP or HTTPS with the generated self-signed certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpTls {
    /// Plain HTTP. Only the loopback strategies may use it.
    #[default]
    Http,
    /// HTTPS with the application's self-signed certificate, which clients pin by fingerprint.
    SelfSigned,
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
    /// What an anonymous caller holds under [`HttpAccessStrategy::LocalOpen`] beyond capability
    /// discovery, at most the `operator` role. Empty by default; kept while another strategy is
    /// selected.
    pub anonymous_permissions: Permissions,
    /// The address [`HttpAccessStrategy::LanAuthenticated`] listens on; `None` listens on every
    /// IPv4 interface. The loopback strategies always listen on `127.0.0.1`.
    pub lan_bind: Option<IpAddr>,
    /// [`HttpAccessStrategy::LanAuthenticated`] requires [`HttpTls::SelfSigned`]; the loopback
    /// strategies may opt into it.
    pub tls: HttpTls,
}

impl Default for HttpAccessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: HttpAccessStrategy::default(),
            port: None,
            serve_webui: true,
            anonymous_permissions: Permissions::NONE,
            lan_bind: None,
            tls: HttpTls::default(),
        }
    }
}

/// Local socket listener settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct LocalSocketAccessConfig {
    /// Master switch. Off by default.
    pub enabled: bool,
}

/// The persisted `remote_access` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct RemoteAccessConfig {
    pub http: HttpAccessConfig,
    pub local_socket: LocalSocketAccessConfig,
}

/// Why a remote-access value cannot be committed.
#[derive(Debug, Clone, PartialEq, Eq, Snafu, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteAccessError {
    /// Port 0 asks the OS for any port; the persisted port is always a concrete one.
    #[snafu(display("the HTTP port must be between 1 and 65535"))]
    PortZero,
    /// Anonymous callers are unidentified local processes, so they never exceed the `operator`
    /// role.
    #[snafu(display("anonymous_permissions cannot hold {resource}:{access}"))]
    AnonymousBeyondOperator {
        /// The first permission beyond the `operator` role.
        resource: Resource,
        access: Access,
    },
    /// The section is changed only by the dedicated remote-access command, never by a
    /// general application patch.
    UseDedicatedCommand,
    /// Credentials would cross the LAN in the clear.
    #[snafu(display("the lan_authenticated strategy requires tls: self_signed"))]
    LanRequiresTls,
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
        if self.http.strategy == HttpAccessStrategy::LanAuthenticated
            && self.http.tls != HttpTls::SelfSigned
        {
            return Err(RemoteAccessError::LanRequiresTls);
        }
        validate_anonymous_permissions(&self.http.anonymous_permissions).map_err(
            |super::PermissionError::AnonymousBeyondOperator { held }| {
                RemoteAccessError::AnonymousBeyondOperator {
                    resource: held.resource,
                    access: held.access,
                }
            },
        )
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
        assert_eq!(config.http.anonymous_permissions, Permissions::NONE);
        assert!(!config.local_socket.enabled);
    }

    #[test]
    fn older_documents_decode_with_defaults() {
        let config: RemoteAccessConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, RemoteAccessConfig::default());
        let partial: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"enabled":true}}"#).unwrap();
        assert!(partial.http.enabled);
        assert!(partial.http.serve_webui);
        assert!(!partial.local_socket.enabled);
    }

    #[test]
    fn local_socket_round_trips_through_serde() {
        for enabled in [false, true] {
            let config = RemoteAccessConfig {
                local_socket: LocalSocketAccessConfig { enabled },
                ..Default::default()
            };
            let json = serde_json::to_string(&config).unwrap();
            let decoded: RemoteAccessConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, config);

            let yaml = yaml_serde::to_string(&config).unwrap();
            let decoded_yaml: RemoteAccessConfig = yaml_serde::from_str(&yaml).unwrap();
            assert_eq!(decoded_yaml, config);
        }
    }

    #[test]
    fn every_strategy_is_stored_and_unknown_ones_are_not() {
        let open: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"strategy":"local_open"}}"#).unwrap();
        assert_eq!(open.http.strategy, HttpAccessStrategy::LocalOpen);
        assert_eq!(open.http.anonymous_permissions, Permissions::NONE);
        let lan: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"strategy":"lan_authenticated"}}"#).unwrap();
        assert_eq!(lan.http.strategy, HttpAccessStrategy::LanAuthenticated);
        let unknown = r#"{"http":{"strategy":"internet_open"}}"#;
        assert!(serde_json::from_str::<RemoteAccessConfig>(unknown).is_err());
    }

    #[test]
    fn a_fresh_install_listens_on_plain_http_without_a_lan_address() {
        let http = RemoteAccessConfig::default().http;
        assert_eq!(http.tls, HttpTls::Http);
        assert_eq!(http.lan_bind, None);
    }

    #[test]
    fn lan_settings_round_trip_through_serde() {
        let mut config = RemoteAccessConfig::default();
        config.http.strategy = HttpAccessStrategy::LanAuthenticated;
        config.http.tls = HttpTls::SelfSigned;
        config.http.lan_bind = Some("192.168.1.20".parse().unwrap());
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""lan_bind":"192.168.1.20""#), "{json}");
        assert!(json.contains(r#""tls":"self_signed""#), "{json}");
        assert_eq!(
            serde_json::from_str::<RemoteAccessConfig>(&json).unwrap(),
            config
        );
        let yaml = yaml_serde::to_string(&config).unwrap();
        assert_eq!(
            yaml_serde::from_str::<RemoteAccessConfig>(&yaml).unwrap(),
            config
        );
    }

    #[test]
    fn the_lan_strategy_requires_tls_before_commit() {
        let mut config = RemoteAccessConfig::default();
        config.http.strategy = HttpAccessStrategy::LanAuthenticated;
        assert_eq!(config.validate(), Err(RemoteAccessError::LanRequiresTls));

        config.http.tls = HttpTls::SelfSigned;
        assert_eq!(config.validate(), Ok(()));

        // The loopback strategies may use either scheme.
        config.http.strategy = HttpAccessStrategy::LocalAuthenticated;
        assert_eq!(config.validate(), Ok(()));
        config.http.tls = HttpTls::Http;
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn anonymous_permissions_are_validated_before_commit() {
        let mut config = RemoteAccessConfig::default();
        assert_eq!(config.validate(), Ok(()));
        config.http.anonymous_permissions = super::super::Role::Operator.permissions();
        assert_eq!(config.validate(), Ok(()));

        for (resource, access) in [
            (Resource::System, Access::Write),
            (Resource::Logs, Access::Write),
            (Resource::Remote, Access::Read),
        ] {
            config.http.anonymous_permissions = Permissions::NONE
                .with(Resource::Proxy, Access::Read)
                .with(resource, access);
            assert_eq!(
                config.validate(),
                Err(RemoteAccessError::AnonymousBeyondOperator { resource, access })
            );
        }
    }

    #[test]
    fn anonymous_permissions_persist_as_a_map() {
        let config: RemoteAccessConfig = serde_json::from_str(
            r#"{"http":{"anonymous_permissions":{"proxy":"write","logs":"read"}}}"#,
        )
        .unwrap();
        assert_eq!(
            config.http.anonymous_permissions,
            Permissions::NONE
                .with(Resource::Proxy, Access::Write)
                .with(Resource::Logs, Access::Read)
        );
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
