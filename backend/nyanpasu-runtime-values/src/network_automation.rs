//! What network automation reports: the network it sees and what its last
//! application did to each capture mechanism.

use nyanpasu_config::application::{NetworkMatch, NetworkState};
use serde::{Deserialize, Serialize};

/// What applying one action did to its mechanism's switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MechanismOutcome {
    /// The action was `keep`, or the switch already had the requested value.
    Unchanged,
    Enabled,
    /// Switched off, by its own action or because the rule enabled the
    /// mechanism that excludes it.
    Disabled,
    /// The switch was not written; `message` is the refusal.
    Failed {
        message: String,
    },
}

/// One outcome per capture mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MechanismOutcomes {
    pub system_proxy: MechanismOutcome,
    pub tun: MechanismOutcome,
    pub transparent_proxy: MechanismOutcome,
}

/// The last time the policy wrote the switches, and for which match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NetworkAutomationApplication {
    pub matched: NetworkMatch,
    pub outcomes: MechanismOutcomes,
}

/// A full value: the receiver replaces what it held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NetworkAutomationStatus {
    /// Whether this host can observe its network at all.
    pub supported: bool,
    /// The latest reading; `None` before the first one and on an unsupported
    /// host.
    pub network: Option<NetworkState>,
    /// `None` until the enabled policy has applied once.
    pub applied: Option<NetworkAutomationApplication>,
}

impl NetworkAutomationStatus {
    /// The status of a host without a network observer.
    #[must_use]
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            network: None,
            applied: None,
        }
    }
}
