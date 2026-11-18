//! Capability discovery over `rpc_describe`.
//!
//! The response is decoded into the canonical [`RpcDescribeResponse`] of the wire crate,
//! with the canonical permission types, so the CLI and the shell agree on one contract.

use nyanpasu_contracts::auth::Principal;
pub use nyanpasu_contracts::auth::{Access, Permission, Permissions, Resource, Role};
use nyanpasu_rpc_wire::{DescribedCommand, RpcDescribeResponse, frame::PROTOCOL_VERSION};
use serde_json::Value;

use crate::error::CliError;

/// The RPC method that returns the caller's own capabilities.
pub const DESCRIBE_METHOD: &str = "rpc_describe";

/// How a method relates to what the principal may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Exposed on this transport and permitted for the principal.
    Allowed,
    /// Exposed, but the principal's permissions do not permit it.
    Denied,
    /// Not exposed on this transport.
    NotExposed,
}

/// The resolved principal's capabilities on one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub(crate) response: RpcDescribeResponse,
}

impl Capabilities {
    /// Decodes a describe response and refuses an incompatible protocol.
    ///
    /// The protocol revision is read first, so a shell speaking another revision is
    /// `Unsupported` even when its response no longer has this revision's shape.
    ///
    /// # Errors
    /// An undecodable response is a failure; a different `protocol` is unsupported.
    pub fn from_json(value: Value) -> Result<Self, CliError> {
        let protocol = value
            .get("protocol")
            .and_then(Value::as_u64)
            .ok_or_else(|| CliError::failed("invalid rpc_describe response: no `protocol`"))?;
        if protocol != u64::from(PROTOCOL_VERSION) {
            return Err(CliError::Unsupported {
                message: format!(
                    "the shell speaks describe protocol {protocol}, this CLI speaks \
                     {PROTOCOL_VERSION}; update nyanpasu-cli or the app so their versions match"
                ),
            });
        }
        serde_json::from_value(value)
            .map(|response| Self { response })
            .map_err(|error| CliError::failed(format!("invalid rpc_describe response: {error}")))
    }

    /// The permissions the principal holds.
    pub fn permissions(&self) -> &Permissions {
        &self.response.permissions
    }

    /// The role the principal's permissions expand from; `None` for a custom map.
    pub fn role(&self) -> Option<Role> {
        self.response.role
    }

    /// The principal the shell resolved this connection to.
    pub fn principal(&self) -> &Principal {
        &self.response.principal
    }

    /// Every command exposed on this transport.
    pub fn commands(&self) -> &[DescribedCommand] {
        &self.response.commands
    }

    /// Whether the principal holds `permission`.
    pub fn permits(&self, permission: Permission) -> bool {
        self.response.permissions.permits(permission)
    }

    /// The exposed command named `method`, if the transport exposes it.
    pub fn command(&self, method: &str) -> Option<&DescribedCommand> {
        self.commands()
            .iter()
            .find(|command| command.name == method)
    }

    /// How `method` relates to the exposed commands and the principal's permission.
    pub fn availability(&self, method: &str) -> Availability {
        match self.command(method) {
            None => Availability::NotExposed,
            Some(command) if command.allowed => Availability::Allowed,
            Some(_) => Availability::Denied,
        }
    }

    /// Returns the command, or fails with the exit status that explains why `method`
    /// cannot be called.
    ///
    /// # Errors
    /// `Unsupported` when the transport does not expose the method, and
    /// `Unauthenticated` when the principal lacks its permission.
    pub fn require(&self, method: &str) -> Result<&DescribedCommand, CliError> {
        match self.command(method) {
            Some(command) if command.allowed => Ok(command),
            Some(_) => Err(CliError::Unauthenticated {
                message: format!("the principal may not call `{method}`"),
            }),
            None => Err(CliError::Unsupported {
                message: format!("`{method}` is not exposed on this transport"),
            }),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use nyanpasu_rpc_wire::{CommandEffect, RpcTarget, ServerInfo};
    use serde_json::Value;

    use super::{
        Capabilities, DescribedCommand, Permission, Permissions, Principal, RpcDescribeResponse,
    };

    /// A describe response holding `held` where every listed method is allowed.
    pub fn describe(held: Permissions, allowed: &[(&str, Permission)]) -> RpcDescribeResponse {
        RpcDescribeResponse {
            protocol: nyanpasu_rpc_wire::frame::PROTOCOL_VERSION,
            server: ServerInfo::desktop("2.0.0"),
            transport: RpcTarget::Http,
            principal: Principal::Token {
                id: "a1b2".into(),
                name: "test".into(),
            },
            role: None,
            permissions: held,
            commands: allowed
                .iter()
                .map(|(name, permission)| DescribedCommand {
                    name: (*name).into(),
                    effect: CommandEffect::Query,
                    permission: *permission,
                    sensitive: false,
                    allowed: true,
                })
                .collect(),
        }
    }

    pub fn describe_json(held: Permissions, allowed: &[(&str, Permission)]) -> Value {
        serde_json::to_value(describe(held, allowed)).expect("fixture serializes")
    }

    pub fn capabilities(held: Permissions, allowed: &[(&str, Permission)]) -> Capabilities {
        Capabilities::from_json(describe_json(held, allowed)).expect("fixture is valid")
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_rpc_wire::{CommandEffect, RpcTarget, ServerKind};
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_the_canonical_response() {
        let capabilities = Capabilities::from_json(json!({
            "protocol": 2,
            "server": { "kind": "desktop", "version": "2.0.0" },
            "transport": "http",
            "principal": { "kind": "token", "id": "a1b2", "name": "laptop-cli" },
            "role": null,
            "permissions": { "proxy": "write", "logs": "read" },
            "commands": [
                { "name": "select_proxy", "effect": "mutation",
                  "permission": { "resource": "proxy", "access": "write" },
                  "sensitive": false, "allowed": true },
                { "name": "restart_sidecar", "effect": "mutation",
                  "permission": { "resource": "system", "access": "write" },
                  "sensitive": false, "allowed": false }
            ]
        }))
        .unwrap();

        assert_eq!(capabilities.response.server.kind, ServerKind::Desktop);
        assert_eq!(capabilities.response.transport, RpcTarget::Http);
        assert_eq!(
            capabilities.principal(),
            &Principal::Token {
                id: "a1b2".into(),
                name: "laptop-cli".into()
            }
        );
        assert_eq!(capabilities.role(), None);
        assert!(capabilities.permits(Permission::new(Resource::Proxy, Access::Read)));
        assert!(capabilities.permits(Permission::new(Resource::Logs, Access::Read)));
        assert!(!capabilities.permits(Permission::new(Resource::Logs, Access::Write)));
        assert!(!capabilities.permits(Permission::new(Resource::System, Access::Read)));
        assert_eq!(
            capabilities
                .command("select_proxy")
                .map(|command| command.effect),
            Some(CommandEffect::Mutation)
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
    fn an_incompatible_protocol_is_unsupported_whatever_its_shape() {
        let error = Capabilities::from_json(json!({ "protocol": 1 })).unwrap_err();
        assert!(matches!(error, CliError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn a_malformed_response_is_a_failure() {
        for malformed in [
            json!({ "permissions": {} }),
            json!({ "protocol": 2 }),
            json!({ "protocol": 2, "server": { "kind": "desktop", "version": "2" },
                    "transport": "http", "principal": { "kind": "anonymous" },
                    "role": null, "permissions": { "secrets": "read" }, "commands": [] }),
        ] {
            let error = Capabilities::from_json(malformed).unwrap_err();
            assert!(matches!(error, CliError::Failed { .. }), "{error}");
        }
    }

    #[test]
    fn require_distinguishes_denied_from_unexposed() {
        let system_read = Permission::new(Resource::System, Access::Read);
        let capabilities = fixtures::capabilities(
            Permissions::NONE.with(Resource::System, Access::Read),
            &[("get_core_status", system_read)],
        );
        assert!(capabilities.require("get_core_status").is_ok());
        assert!(matches!(
            capabilities.require("restart_sidecar"),
            Err(CliError::Unsupported { .. })
        ));

        let mut denied = capabilities;
        denied.response.commands[0].allowed = false;
        assert!(matches!(
            denied.require("get_core_status"),
            Err(CliError::Unauthenticated { .. })
        ));
    }
}
