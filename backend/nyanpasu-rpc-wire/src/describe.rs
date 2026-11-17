//! Shared RPC self-description contract (`rpc_describe`).

use nyanpasu_contracts::auth::{Permission, Permissions, Principal, Role};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A command's explicitly enabled transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RpcTarget {
    /// The complete desktop command surface.
    Desktop,
    /// Browser access, explicitly opted into by the declaration.
    Http,
    /// Local socket access, independently opted into by the declaration.
    Local,
}

/// What a command does, independent of how a transport dispatches it.
///
/// Queries observe application or network state; refreshing a read model or measuring latency
/// does not change application configuration. Mutations perform application operations, including
/// allocating caller-owned resources. The permission independently controls who may request either
/// effect.
/// The declaration is the source of truth; the generated frontend's split is checked against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandEffect {
    /// Returns data and changes nothing.
    Query,
    /// Everything else.
    Mutation,
}

/// Which runtime hosts the RPC endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    /// Native Tauri desktop application.
    Desktop,
    /// Headless service daemon (`nyanpasu-server`).
    Headless,
}

/// Non-secret host metadata disclosed to resolved callers via `rpc_describe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ServerInfo {
    pub kind: ServerKind,
    pub version: String,
}

impl ServerInfo {
    /// Build server metadata for the desktop GUI shell.
    pub fn desktop(version: impl Into<String>) -> Self {
        Self {
            kind: ServerKind::Desktop,
            version: version.into(),
        }
    }

    /// Build server metadata for the headless daemon.
    pub fn headless(version: impl Into<String>) -> Self {
        Self {
            kind: ServerKind::Headless,
            version: version.into(),
        }
    }
}

/// One known command on the active transport and whether this caller may invoke it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DescribedCommand {
    pub name: String,
    pub effect: CommandEffect,
    /// The one permission a caller must hold.
    pub permission: Permission,
    /// Responses may carry credentials, subscription URLs, configuration text or diagnostics.
    pub sensitive: bool,
    pub allowed: bool,
}

/// Canonical caller-specific discovery document returned by `rpc_describe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RpcDescribeResponse {
    pub protocol: u32,
    pub server: ServerInfo,
    pub transport: RpcTarget,
    pub principal: Principal,
    /// The role the caller's permissions expand from; absent for a custom permission map.
    pub role: Option<Role>,
    pub permissions: Permissions,
    pub commands: Vec<DescribedCommand>,
}

#[cfg(test)]
mod tests {
    use nyanpasu_contracts::auth::{Access, Resource};
    use serde_json::json;

    use super::*;

    #[test]
    fn describe_response_serializes_with_canonical_wire_shape() {
        let response = RpcDescribeResponse {
            protocol: 1,
            server: ServerInfo::desktop("2.0.0"),
            transport: RpcTarget::Http,
            principal: Principal::Token {
                id: "t_01J".to_owned(),
                name: "skill-cli".to_owned(),
            },
            role: None,
            permissions: Permissions::NONE
                .with(Resource::Proxy, Access::Write)
                .with(Resource::Profile, Access::Read),
            commands: vec![
                DescribedCommand {
                    name: "get_proxies".to_owned(),
                    effect: CommandEffect::Query,
                    permission: Permission::new(Resource::Proxy, Access::Read),
                    sensitive: false,
                    allowed: true,
                },
                DescribedCommand {
                    name: "set_system_proxy".to_owned(),
                    effect: CommandEffect::Mutation,
                    permission: Permission::new(Resource::System, Access::Write),
                    sensitive: false,
                    allowed: false,
                },
            ],
        };

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(
            value,
            json!({
                "protocol": 1,
                "server": { "kind": "desktop", "version": "2.0.0" },
                "transport": "http",
                "principal": { "kind": "token", "id": "t_01J", "name": "skill-cli" },
                "role": null,
                "permissions": { "proxy": "write", "profile": "read" },
                "commands": [
                    {
                        "name": "get_proxies",
                        "effect": "query",
                        "permission": { "resource": "proxy", "access": "read" },
                        "sensitive": false,
                        "allowed": true
                    },
                    {
                        "name": "set_system_proxy",
                        "effect": "mutation",
                        "permission": { "resource": "system", "access": "write" },
                        "sensitive": false,
                        "allowed": false
                    }
                ]
            })
        );
        let roundtrip: RpcDescribeResponse = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, response);
    }
}
