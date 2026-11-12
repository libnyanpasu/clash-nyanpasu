//! The permission classes a caller can hold over the application's commands.
//!
//! This is the single definition of a scope. Config validates the scopes an anonymous caller may
//! hold, core stores the scopes of a token, and the RPC protocol's command metadata uses the same
//! value, so no layer needs its own copy.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use specta::Type;

/// What a command needs, and what a principal holds.
///
/// A command declares exactly one scope. Scopes are independent: holding `proxy` grants no `read`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Queries without secrets: status, listings, logs, traffic and connection views.
    Read,
    /// Queries that return secrets: the Clash secret, subscription URLs, profile and runtime
    /// contents, and generic storage.
    SensitiveRead,
    /// Select a proxy, latency tests, switch the proxy mode, close connections.
    Proxy,
    /// Profile and subscription changes, transforms, provider updates.
    Profile,
    /// Ordinary application and Clash preferences.
    Settings,
    /// Core lifecycle, system proxy, TUN, transparent proxy, service, the Clash control plane,
    /// listening exposure and every shell or OS operation.
    System,
    /// Remote access settings, tokens and sessions.
    RemoteAdmin,
}

impl Scope {
    /// Every scope, in declaration order.
    pub const ALL: [Scope; 7] = [
        Scope::Read,
        Scope::SensitiveRead,
        Scope::Proxy,
        Scope::Profile,
        Scope::Settings,
        Scope::System,
        Scope::RemoteAdmin,
    ];

    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An unknown scope name.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
#[snafu(display("unknown scope {name:?}"))]
pub struct UnknownScope {
    pub name: String,
}

impl FromStr for Scope {
    type Err = UnknownScope;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|scope| scope.as_str() == name)
            .ok_or_else(|| UnknownScope { name: name.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_match_the_wire() {
        for scope in Scope::ALL {
            assert_eq!(scope.as_str().parse::<Scope>().unwrap(), scope);
            assert_eq!(
                serde_json::to_value(scope).unwrap(),
                serde_json::json!(scope.as_str())
            );
        }
        assert!("everything".parse::<Scope>().is_err());
    }
}
