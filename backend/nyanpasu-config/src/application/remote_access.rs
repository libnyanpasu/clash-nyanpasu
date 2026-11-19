//! Persisted remote-access settings.
//!
//! The section is owned by the application config but is never part of an ordinary
//! preferences patch: it widens who can reach the application, so it is written only
//! through a dedicated command that is not exposed over HTTP.

use std::net::Ipv4Addr;

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use specta::Type;

use super::{Access, Permissions, Resource, validate_anonymous_permissions};

/// Whether an HTTP caller must present a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpAuthentication {
    /// Every request must carry a valid credential, also one from a loopback peer: a reverse
    /// proxy makes every request look local.
    #[default]
    Required,
    /// A request without a credential is an anonymous caller holding capability discovery plus
    /// [`HttpAccessConfig::anonymous_permissions`]. Only [`HttpListen::Local`] allows it, and even
    /// then every local process of every OS user can reach it, because loopback TCP is not
    /// user-scoped.
    Optional,
}

/// Which peers the HTTP listener can be reached from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpListen {
    /// `127.0.0.1` only.
    #[default]
    Local,
    /// Every IPv4 interface, accepting connections from loopback and from
    /// [`HttpAccessConfig::lan_allow`] only.
    Lan,
    /// Every IPv4 interface, accepting connections from anywhere.
    Any,
}

/// Whether the listener speaks plain HTTP or HTTPS with the generated self-signed certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpTls {
    /// Plain HTTP. Only [`HttpListen::Local`] may use it.
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
    pub auth: HttpAuthentication,
    /// Kept while the switch is off, so turning it on restores the earlier choice.
    pub listen: HttpListen,
    /// The networks [`HttpListen::Lan`] accepts connections from, besides loopback. Kept while
    /// another listen scope is selected.
    #[specta(type = Vec<String>)]
    pub lan_allow: Vec<Ipv4Net>,
    /// `None` until the first start binds a free port and records it. Later starts
    /// reuse the recorded port, so URLs stay valid across restarts.
    pub port: Option<u16>,
    /// Serve the bundled web UI. When false only the API is served.
    pub serve_webui: bool,
    /// What an anonymous caller holds under [`HttpAuthentication::Optional`] beyond capability
    /// discovery, at most the `operator` role. Empty by default; kept while authentication is
    /// required.
    pub anonymous_permissions: Permissions,
    /// Listening beyond loopback requires [`HttpTls::SelfSigned`]; [`HttpListen::Local`] may opt
    /// into it.
    pub tls: HttpTls,
}

impl Default for HttpAccessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auth: HttpAuthentication::default(),
            listen: HttpListen::default(),
            lan_allow: Vec::new(),
            port: None,
            serve_webui: true,
            anonymous_permissions: Permissions::NONE,
            tls: HttpTls::default(),
        }
    }
}

impl HttpAccessConfig {
    /// Whether a request without a credential is an anonymous caller. Requires the loopback
    /// listener as well, so a value that skipped validation never opens the network.
    #[must_use]
    pub fn admits_anonymous(&self) -> bool {
        self.auth == HttpAuthentication::Optional && self.listen == HttpListen::Local
    }
}

/// Local socket listener settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct LocalSocketAccessConfig {
    /// Master switch. Off by default.
    pub enabled: bool,
}

/// An IPv4 network this computer is attached to, offered as a LAN network to allow.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct LocalNetwork {
    /// The interface's name as the OS shows it, such as `Ethernet` or `Tailscale`.
    pub interface: String,
    /// This computer's address on the network.
    pub address: Ipv4Addr,
    /// The network in CIDR notation, `192.168.1.0/24`.
    #[specta(type = String)]
    pub network: Ipv4Net,
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
    /// Only loopback callers may skip the credential.
    #[snafu(display("auth: optional requires listen: local"))]
    OptionalAuthenticationBeyondLocal,
    /// Credentials would cross the network in the clear.
    #[snafu(display("listening beyond loopback requires tls: self_signed"))]
    NetworkRequiresTls,
    /// A LAN listener that allows no network is reachable from loopback only.
    #[snafu(display("listen: lan requires at least one network in lan_allow"))]
    LanAllowEmpty,
}

impl RemoteAccessConfig {
    /// Checks a candidate before it is committed.
    ///
    /// # Errors
    /// Returns the first invalid field.
    pub fn validate(&self) -> Result<(), RemoteAccessError> {
        let http = &self.http;
        if http.port == Some(0) {
            return Err(RemoteAccessError::PortZero);
        }
        if http.listen != HttpListen::Local {
            if http.auth == HttpAuthentication::Optional {
                return Err(RemoteAccessError::OptionalAuthenticationBeyondLocal);
            }
            if http.tls != HttpTls::SelfSigned {
                return Err(RemoteAccessError::NetworkRequiresTls);
            }
        }
        if http.listen == HttpListen::Lan && http.lan_allow.is_empty() {
            return Err(RemoteAccessError::LanAllowEmpty);
        }
        validate_anonymous_permissions(&http.anonymous_permissions).map_err(
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
    fn a_fresh_install_is_off_local_and_authenticated() {
        let config = RemoteAccessConfig::default();
        assert!(!config.http.enabled);
        assert_eq!(config.http.auth, HttpAuthentication::Required);
        assert_eq!(config.http.listen, HttpListen::Local);
        assert!(config.http.lan_allow.is_empty());
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
    fn every_choice_is_stored_and_unknown_ones_are_not() {
        let open: RemoteAccessConfig =
            serde_json::from_str(r#"{"http":{"auth":"optional"}}"#).unwrap();
        assert_eq!(open.http.auth, HttpAuthentication::Optional);
        assert_eq!(open.http.anonymous_permissions, Permissions::NONE);
        let any: RemoteAccessConfig = serde_json::from_str(r#"{"http":{"listen":"any"}}"#).unwrap();
        assert_eq!(any.http.listen, HttpListen::Any);
        for unknown in [
            r#"{"http":{"auth":"none"}}"#,
            r#"{"http":{"listen":"internet"}}"#,
            r#"{"http":{"lan_allow":["192.168.1.0/33"]}}"#,
        ] {
            assert!(
                serde_json::from_str::<RemoteAccessConfig>(unknown).is_err(),
                "{unknown}"
            );
        }
    }

    #[test]
    fn a_fresh_install_listens_on_plain_http() {
        assert_eq!(RemoteAccessConfig::default().http.tls, HttpTls::Http);
    }

    #[test]
    fn lan_settings_round_trip_through_serde() {
        let mut config = RemoteAccessConfig::default();
        config.http.listen = HttpListen::Lan;
        config.http.tls = HttpTls::SelfSigned;
        config.http.lan_allow = vec![
            "192.168.1.0/24".parse().unwrap(),
            "100.64.0.0/10".parse().unwrap(),
        ];
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains(r#""lan_allow":["192.168.1.0/24","100.64.0.0/10"]"#),
            "{json}"
        );
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
    fn listening_beyond_loopback_requires_tls_and_a_credential() {
        let mut config = RemoteAccessConfig::default();
        config.http.lan_allow = vec!["192.168.1.0/24".parse().unwrap()];
        for listen in [HttpListen::Lan, HttpListen::Any] {
            config.http.listen = listen;
            config.http.tls = HttpTls::Http;
            assert_eq!(
                config.validate(),
                Err(RemoteAccessError::NetworkRequiresTls)
            );

            config.http.tls = HttpTls::SelfSigned;
            assert_eq!(config.validate(), Ok(()));

            config.http.auth = HttpAuthentication::Optional;
            assert_eq!(
                config.validate(),
                Err(RemoteAccessError::OptionalAuthenticationBeyondLocal)
            );
            assert!(!config.http.admits_anonymous());
            config.http.auth = HttpAuthentication::Required;
        }

        // The loopback listener may skip the credential and use either scheme.
        config.http.listen = HttpListen::Local;
        config.http.auth = HttpAuthentication::Optional;
        assert_eq!(config.validate(), Ok(()));
        assert!(config.http.admits_anonymous());
        config.http.tls = HttpTls::Http;
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn a_lan_listener_allows_at_least_one_network() {
        let mut config = RemoteAccessConfig::default();
        config.http.listen = HttpListen::Lan;
        config.http.tls = HttpTls::SelfSigned;
        assert_eq!(config.validate(), Err(RemoteAccessError::LanAllowEmpty));

        config.http.lan_allow = vec!["10.147.17.0/24".parse().unwrap()];
        assert_eq!(config.validate(), Ok(()));

        // The list is kept, and not required, under another scope.
        config.http.listen = HttpListen::Any;
        config.http.lan_allow.clear();
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
