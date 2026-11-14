//! Capability discovery over `rpc_describe`.
//!
//! The CLI consumes the JSON shape of the design's capability discovery response
//! (`protocol`, `server`, `transport`, `principal`, `scopes`, `commands`). Every
//! other module depends only on [`Scope`] and [`Capabilities`] from here, so that
//! the canonical `nyanpasu_rpc_protocol::Scope` and a typed describe response can
//! replace [`Scope`] and [`Capabilities::from_json`] without touching callers.

use std::{collections::BTreeSet, fmt, str::FromStr};

use serde::Deserialize;
use serde_json::Value;

use crate::error::CliError;

/// The describe protocol revision this CLI speaks.
pub const SUPPORTED_PROTOCOL: u64 = 1;

/// The RPC method that returns the caller's own capabilities.
pub const DESCRIBE_METHOD: &str = "rpc_describe";

/// A command scope. Mirrors the canonical scope enum of the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    /// Queries without secrets.
    Read,
    /// Queries that return secrets.
    SensitiveRead,
    /// Proxy selection, latency tests, mode, connection closing.
    Proxy,
    /// Profile and subscription changes.
    Profile,
    /// Application and Clash settings that are not system or remote access.
    Settings,
    /// Core control, system proxy, TUN.
    System,
    /// Remote-access settings, tokens and sessions. Never offered by the CLI tools.
    RemoteAdmin,
}

impl Scope {
    /// Every scope, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Read,
        Self::SensitiveRead,
        Self::Proxy,
        Self::Profile,
        Self::Settings,
        Self::System,
        Self::RemoteAdmin,
    ];

    /// The wire and `--scope` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::SensitiveRead => "sensitive_read",
            Self::Proxy => "proxy",
            Self::Profile => "profile",
            Self::Settings => "settings",
            Self::System => "system",
            Self::RemoteAdmin => "remote_admin",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The input was not a known scope name.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownScope(pub String);

impl fmt::Display for UnknownScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown scope `{}`", self.0)
    }
}

impl std::error::Error for UnknownScope {}

impl FromStr for Scope {
    type Err = UnknownScope;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|scope| scope.as_str() == value)
            .ok_or_else(|| UnknownScope(value.to_owned()))
    }
}

/// Whether a described command is a query or a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescribedKind {
    /// Reads state.
    Query,
    /// Changes state.
    Mutation,
}

/// One entry of the describe response's `commands`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedCommand {
    /// The wire method name.
    pub name: String,
    /// Query or mutation, when the shell reported a known kind.
    pub kind: Option<DescribedKind>,
    /// The command's scope, when the shell reported a known one.
    pub scope: Option<Scope>,
    /// Whether the resolved principal may call it on this transport.
    pub allowed: bool,
}

/// How a method relates to what the principal may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Exposed on this transport and permitted for the principal.
    Allowed,
    /// Exposed, but the principal's scopes do not permit it.
    Denied,
    /// Not exposed on this transport.
    NotExposed,
}

#[derive(Deserialize)]
struct RawCommand {
    name: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    allowed: bool,
}

#[derive(Deserialize)]
struct RawDescribe {
    protocol: u64,
    #[serde(default)]
    server: Option<RawServer>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    principal: Option<Value>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    commands: Vec<RawCommand>,
}

#[derive(Deserialize)]
struct RawServer {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

/// The resolved principal's capabilities on one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// The describe protocol revision the shell reported.
    pub protocol: u64,
    /// `desktop` or `server`, when reported.
    pub server_kind: Option<String>,
    /// The shell's version, when reported.
    pub server_version: Option<String>,
    /// The transport the shell resolved the call on, when reported.
    pub transport: Option<String>,
    /// The resolved principal, as the shell reported it.
    pub principal: Option<Value>,
    /// The principal's scopes. Scope names this CLI does not know are dropped.
    pub scopes: BTreeSet<Scope>,
    /// Every command exposed on this transport.
    pub commands: Vec<DescribedCommand>,
}

impl Capabilities {
    /// Decodes a describe response and refuses an incompatible protocol.
    ///
    /// # Errors
    /// An undecodable response is a failure; a different `protocol` is unsupported.
    pub fn from_json(value: Value) -> Result<Self, CliError> {
        let raw: RawDescribe = serde_json::from_value(value)
            .map_err(|error| CliError::failed(format!("invalid rpc_describe response: {error}")))?;
        if raw.protocol != SUPPORTED_PROTOCOL {
            return Err(CliError::Unsupported {
                message: format!(
                    "the shell speaks describe protocol {}, this CLI speaks {SUPPORTED_PROTOCOL}; \
                     update nyanpasu-cli or the app so their versions match",
                    raw.protocol
                ),
            });
        }
        let (server_kind, server_version) = raw
            .server
            .map_or((None, None), |server| (server.kind, server.version));
        Ok(Self {
            protocol: raw.protocol,
            server_kind,
            server_version,
            transport: raw.transport,
            principal: raw.principal,
            scopes: raw
                .scopes
                .iter()
                .filter_map(|scope| scope.parse().ok())
                .collect(),
            commands: raw
                .commands
                .into_iter()
                .map(|command| DescribedCommand {
                    kind: match command.kind.as_deref() {
                        Some("query") => Some(DescribedKind::Query),
                        Some("mutation") => Some(DescribedKind::Mutation),
                        _ => None,
                    },
                    scope: command.scope.and_then(|scope| scope.parse().ok()),
                    name: command.name,
                    allowed: command.allowed,
                })
                .collect(),
        })
    }

    /// Whether the principal holds `scope`.
    pub fn holds(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }

    /// How `method` relates to the exposed commands and the principal's permission.
    pub fn availability(&self, method: &str) -> Availability {
        match self.commands.iter().find(|command| command.name == method) {
            None => Availability::NotExposed,
            Some(command) if command.allowed => Availability::Allowed,
            Some(_) => Availability::Denied,
        }
    }

    /// Fails with the exit status that explains why `method` cannot be called.
    ///
    /// # Errors
    /// `Unsupported` when the transport does not expose the method, and
    /// `Unauthenticated` when the principal lacks its scope.
    pub fn require(&self, method: &str) -> Result<(), CliError> {
        match self.availability(method) {
            Availability::Allowed => Ok(()),
            Availability::Denied => Err(CliError::Unauthenticated {
                message: format!("the principal may not call `{method}`"),
            }),
            Availability::NotExposed => Err(CliError::Unsupported {
                message: format!("`{method}` is not exposed on this transport"),
            }),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::{Value, json};

    use super::Capabilities;

    /// A describe response where every listed method is allowed.
    pub fn describe_json(scopes: &[&str], allowed: &[(&str, &str)]) -> Value {
        json!({
            "protocol": 1,
            "server": { "kind": "desktop", "version": "2.0.0" },
            "transport": "http",
            "principal": { "kind": "token", "id": "a1b2", "name": "test" },
            "scopes": scopes,
            "commands": allowed
                .iter()
                .map(|(name, scope)| json!({
                    "name": name,
                    "kind": "query",
                    "scope": scope,
                    "allowed": true,
                }))
                .collect::<Vec<_>>(),
        })
    }

    pub fn capabilities(scopes: &[&str], allowed: &[(&str, &str)]) -> Capabilities {
        Capabilities::from_json(describe_json(scopes, allowed)).expect("fixture is valid")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_the_design_example() {
        let capabilities = Capabilities::from_json(json!({
            "protocol": 1,
            "server": { "kind": "desktop", "version": "2.0.0" },
            "transport": "http",
            "principal": { "kind": "token", "id": "a1b2", "name": "laptop-cli" },
            "scopes": ["read", "proxy", "future_scope"],
            "commands": [
                { "name": "select_proxy", "kind": "mutation", "scope": "proxy", "allowed": true },
                { "name": "restart_sidecar", "kind": "mutation", "scope": "system", "allowed": false }
            ]
        }))
        .unwrap();

        assert_eq!(capabilities.server_kind.as_deref(), Some("desktop"));
        assert_eq!(
            capabilities.scopes,
            BTreeSet::from([Scope::Read, Scope::Proxy]),
            "scope names this CLI does not know are dropped"
        );
        assert_eq!(
            capabilities.availability("select_proxy"),
            Availability::Allowed
        );
        assert_eq!(
            capabilities.availability("restart_sidecar"),
            Availability::Denied
        );
        assert_eq!(
            capabilities.availability("get_profiles"),
            Availability::NotExposed
        );
    }

    #[test]
    fn an_incompatible_protocol_is_unsupported() {
        let error = Capabilities::from_json(json!({ "protocol": 2 })).unwrap_err();
        assert!(matches!(error, CliError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_malformed_response_is_a_failure() {
        let error = Capabilities::from_json(json!({ "scopes": [] })).unwrap_err();
        assert!(matches!(error, CliError::Failed { .. }), "{error}");
    }

    #[test]
    fn require_distinguishes_denied_from_unexposed() {
        let capabilities = fixtures::capabilities(&["read"], &[("get_core_status", "read")]);
        assert!(capabilities.require("get_core_status").is_ok());
        assert!(matches!(
            capabilities.require("restart_sidecar"),
            Err(CliError::Unsupported { .. })
        ));

        let mut denied = capabilities;
        denied.commands[0].allowed = false;
        assert!(matches!(
            denied.require("get_core_status"),
            Err(CliError::Unauthenticated { .. })
        ));
    }

    #[test]
    fn scope_names_round_trip() {
        for scope in Scope::ALL {
            assert_eq!(scope.as_str().parse::<Scope>(), Ok(scope));
        }
        assert_eq!(
            "nope".parse::<Scope>(),
            Err(UnknownScope("nope".to_owned()))
        );
    }
}
