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

/// What applying one action did to its mechanism's switch, without the refusal text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MechanismOutcomeKind {
    Unchanged,
    Enabled,
    Disabled,
    Failed,
}

impl From<&MechanismOutcome> for MechanismOutcomeKind {
    fn from(outcome: &MechanismOutcome) -> Self {
        match outcome {
            MechanismOutcome::Unchanged => Self::Unchanged,
            MechanismOutcome::Enabled => Self::Enabled,
            MechanismOutcome::Disabled => Self::Disabled,
            MechanismOutcome::Failed { message: _ } => Self::Failed,
        }
    }
}

/// One outcome kind per capture mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MechanismOutcomeKinds {
    pub system_proxy: MechanismOutcomeKind,
    pub tun: MechanismOutcomeKind,
    pub transparent_proxy: MechanismOutcomeKind,
}

/// The last application without the rule's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NetworkAutomationApplicationSummary {
    /// Whether a rule decided, rather than the default actions. The rule's id is the user's
    /// own label for a network, so it stays with `sensitive_read`.
    pub matched_rule: bool,
    pub outcomes: MechanismOutcomeKinds,
}

impl From<&NetworkAutomationApplication> for NetworkAutomationApplicationSummary {
    fn from(application: &NetworkAutomationApplication) -> Self {
        let NetworkAutomationApplication { matched, outcomes } = application;
        let MechanismOutcomes {
            system_proxy,
            tun,
            transparent_proxy,
        } = outcomes;
        Self {
            matched_rule: matches!(matched, NetworkMatch::Rule { id: _ }),
            outcomes: MechanismOutcomeKinds {
                system_proxy: system_proxy.into(),
                tun: tun.into(),
                transparent_proxy: transparent_proxy.into(),
            },
        }
    }
}

/// Safe network automation summary for callers without `Scope::SensitiveRead`.
///
/// The network reading holds the Wi-Fi name and the gateway's MAC address, and the matched rule
/// names one of the user's networks, so none of them is part of this view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NetworkAutomationSummary {
    pub supported: bool,
    pub applied: Option<NetworkAutomationApplicationSummary>,
}

impl From<&NetworkAutomationStatus> for NetworkAutomationSummary {
    fn from(status: &NetworkAutomationStatus) -> Self {
        let NetworkAutomationStatus {
            supported,
            network: _,
            applied,
        } = status;
        Self {
            supported: *supported,
            applied: applied.as_ref().map(Into::into),
        }
    }
}
