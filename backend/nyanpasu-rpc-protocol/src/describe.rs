//! Shared RPC self-description contract (`rpc_describe`).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CommandEffect, Principal, RpcTarget, Scope, ScopeSet};

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
    pub scope: Scope,
    pub allowed: bool,
}

/// Canonical caller-scoped discovery document returned by `rpc_describe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RpcDescribeResponse {
    pub protocol: u32,
    pub server: ServerInfo,
    pub transport: RpcTarget,
    pub principal: Principal,
    pub scopes: ScopeSet,
    pub commands: Vec<DescribedCommand>,
}

#[cfg(test)]
mod tests {
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
            scopes: ScopeSet::from([Scope::Proxy, Scope::Read]),
            commands: vec![
                DescribedCommand {
                    name: "get_proxies".to_owned(),
                    effect: CommandEffect::Query,
                    scope: Scope::Read,
                    allowed: true,
                },
                DescribedCommand {
                    name: "set_system_proxy".to_owned(),
                    effect: CommandEffect::Mutation,
                    scope: Scope::System,
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
                "scopes": ["read", "proxy"],
                "commands": [
                    { "name": "get_proxies", "effect": "query", "scope": "read", "allowed": true },
                    { "name": "set_system_proxy", "effect": "mutation", "scope": "system", "allowed": false }
                ]
            })
        );
        let roundtrip: RpcDescribeResponse = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, response);
    }
}
