//! Network automation policy: rules that switch each capture mechanism on or
//! off for the network the machine is on.
//!
//! Everything here is a pure service: the policy model, its validation and the
//! evaluation of a policy against one observed network. Reading the network
//! and writing the switches belong to their owners.

use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};
use specta::Type;

/// The most rules a policy holds; a guard against a runaway document, not a
/// product limit.
pub const MAX_NETWORK_RULES: usize = 64;

/// What a rule does to one capture mechanism when it starts to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MechanismAction {
    /// Leaves the switch as the user set it.
    #[default]
    Keep,
    Enable,
    Disable,
}

/// One action per capture mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct MechanismActions {
    pub system_proxy: MechanismAction,
    pub tun: MechanismAction,
    pub transparent_proxy: MechanismAction,
}

/// The link type of the primary network interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    Wifi,
    Ethernet,
    Other,
}

/// The name of the Wi-Fi network behind the primary interface.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WifiName {
    /// The primary interface is not Wi-Fi.
    NotWifi,
    /// The interface is Wi-Fi, but the name was not read: no enabled rule
    /// asks for it, and reading it is what makes macOS and Windows ask the
    /// user for location access.
    NotRead,
    Known {
        ssid: String,
    },
    /// The interface is Wi-Fi and its name was asked for, but could not be
    /// read.
    Unavailable {
        reason: WifiUnavailable,
    },
}

/// Why the name of a Wi-Fi network could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WifiUnavailable {
    /// The platform gates the name behind location access the user has not
    /// decided on yet. Asking can show the system prompt.
    PermissionRequired,
    /// The user, the device policy or a global location switch refused
    /// location access. Only the system settings can change that.
    PermissionDenied,
    /// The platform answered without a name: the interface is not
    /// associated, or the name service is missing or failed.
    Unreadable,
    /// This host has no way to read a Wi-Fi name.
    Unsupported,
}

/// The primary network: the physical interface that carries the default
/// route, never this application's own TUN device.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct NetworkObservation {
    pub interface_kind: InterfaceKind,
    pub wifi: WifiName,
    /// The default gateway's link-layer address, as lowercase
    /// `aa:bb:cc:dd:ee:ff`, when the neighbour table has it.
    pub gateway_mac: Option<String>,
}

/// What the host reports about its network right now.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NetworkState {
    /// No physical interface has a default gateway.
    Offline,
    Online {
        network: NetworkObservation,
    },
}

/// The facts a rule requires of the primary network. Every present field must
/// hold, and a field whose fact the observation lacks never holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct NetworkConditions {
    /// Exact Wi-Fi name.
    pub wifi_ssid: Option<String>,
    /// Default gateway MAC address, `aa:bb:cc:dd:ee:ff`.
    pub gateway_mac: Option<String>,
    pub interface_kind: Option<InterfaceKind>,
}

impl NetworkConditions {
    fn is_empty(&self) -> bool {
        self.wifi_ssid.is_none() && self.gateway_mac.is_none() && self.interface_kind.is_none()
    }

    fn holds(&self, network: &NetworkObservation) -> bool {
        let ssid_holds = self.wifi_ssid.as_deref().is_none_or(
            |expected| matches!(&network.wifi, WifiName::Known { ssid } if ssid == expected),
        );
        let gateway_holds = self.gateway_mac.as_deref().is_none_or(|expected| {
            network
                .gateway_mac
                .as_deref()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        });
        let kind_holds = self
            .interface_kind
            .is_none_or(|expected| network.interface_kind == expected);

        ssid_holds && gateway_holds && kind_holds
    }
}

/// A named network and what arriving on it does to each mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct NetworkRule {
    /// Stable identity, so renaming or reordering a rule is not a new match.
    pub id: String,
    pub name: String,
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    pub conditions: NetworkConditions,
    #[serde(default)]
    pub actions: MechanismActions,
}

fn default_rule_enabled() -> bool {
    true
}

/// The persisted policy. Disabled and empty by default, so an installation
/// that never opens the feature behaves as before.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct NetworkAutomation {
    pub enabled: bool,
    /// Applied on a network no rule matches.
    pub default_actions: MechanismActions,
    /// Tested in order; the first enabled match decides.
    pub rules: Vec<NetworkRule>,
}

/// Which part of the policy decided the actions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetworkMatch {
    Rule {
        id: String,
    },
    /// No rule matched; the default actions apply.
    Default,
}

/// The policy's answer for one observed network.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct NetworkEvaluation {
    pub matched: NetworkMatch,
    pub actions: MechanismActions,
}

/// Why a policy cannot be saved.
#[derive(Debug, Snafu, PartialEq, Eq)]
pub enum NetworkAutomationError {
    #[snafu(display("a policy holds at most {MAX_NETWORK_RULES} rules, not {count}"))]
    TooManyRules { count: usize },
    #[snafu(display("rule {index} has no id"))]
    MissingId { index: usize },
    #[snafu(display("rule id {id:?} is used more than once"))]
    DuplicateId { id: String },
    #[snafu(display("rule {index} has no name"))]
    MissingName { index: usize },
    #[snafu(display("rule {name:?} has no condition"))]
    MissingConditions { name: String },
    #[snafu(display("rule {name:?} has an empty Wi-Fi name"))]
    EmptyWifiSsid { name: String },
    #[snafu(display("rule {name:?} has an invalid gateway MAC address {value:?}"))]
    InvalidGatewayMac { name: String, value: String },
    #[snafu(display("{scope} enables both TUN and transparent proxy, which exclude each other"))]
    ConflictingCapture { scope: String },
}

impl NetworkAutomation {
    /// Checks the whole policy before it is committed.
    ///
    /// # Errors
    ///
    /// Returns the first rule or action set that could never be applied.
    pub fn validate(&self) -> Result<(), NetworkAutomationError> {
        ensure!(
            self.rules.len() <= MAX_NETWORK_RULES,
            TooManyRulesSnafu {
                count: self.rules.len()
            }
        );
        validate_actions(self.default_actions, "the default")?;

        for (index, rule) in self.rules.iter().enumerate() {
            ensure!(!rule.id.is_empty(), MissingIdSnafu { index });
            ensure!(
                !self.rules[..index].iter().any(|other| other.id == rule.id),
                DuplicateIdSnafu { id: &rule.id }
            );
            ensure!(!rule.name.trim().is_empty(), MissingNameSnafu { index });
            ensure!(
                !rule.conditions.is_empty(),
                MissingConditionsSnafu { name: &rule.name }
            );
            ensure!(
                rule.conditions.wifi_ssid.as_deref() != Some(""),
                EmptyWifiSsidSnafu { name: &rule.name }
            );
            if let Some(value) = rule.conditions.gateway_mac.as_deref() {
                ensure!(
                    is_mac_address(value),
                    InvalidGatewayMacSnafu {
                        name: &rule.name,
                        value
                    }
                );
            }
            validate_actions(rule.actions, &format!("rule {:?}", rule.name))?;
        }

        Ok(())
    }

    /// Whether the policy needs the Wi-Fi name: it is enabled and an enabled
    /// rule matches on it. Nothing else justifies reading it, because the
    /// platforms that gate the name ask the user for location access.
    #[must_use]
    pub fn reads_wifi_name(&self) -> bool {
        self.enabled
            && self
                .rules
                .iter()
                .any(|rule| rule.enabled && rule.conditions.wifi_ssid.is_some())
    }

    /// The actions for `network`: the first enabled rule whose conditions all
    /// hold, otherwise the default. The caller decides whether the policy is
    /// enabled and whether the answer is news.
    #[must_use]
    pub fn evaluate(&self, network: &NetworkObservation) -> NetworkEvaluation {
        self.rules
            .iter()
            .find(|rule| rule.enabled && rule.conditions.holds(network))
            .map_or(
                NetworkEvaluation {
                    matched: NetworkMatch::Default,
                    actions: self.default_actions,
                },
                |rule| NetworkEvaluation {
                    matched: NetworkMatch::Rule {
                        id: rule.id.clone(),
                    },
                    actions: rule.actions,
                },
            )
    }
}

fn validate_actions(actions: MechanismActions, scope: &str) -> Result<(), NetworkAutomationError> {
    ensure!(
        !(actions.tun == MechanismAction::Enable
            && actions.transparent_proxy == MechanismAction::Enable),
        ConflictingCaptureSnafu { scope }
    );
    Ok(())
}

/// Six colon-separated hex octets that name one station: not the zero
/// address, which is what an unresolved neighbour reads as.
fn is_mac_address(value: &str) -> bool {
    let octets: Vec<&str> = value.split(':').collect();
    octets.len() == 6
        && octets
            .iter()
            .all(|octet| octet.len() == 2 && octet.bytes().all(|byte| byte.is_ascii_hexdigit()))
        && octets.iter().any(|octet| *octet != "00")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME_GATEWAY: &str = "aa:bb:cc:dd:ee:ff";

    fn wifi(ssid: &str, gateway_mac: Option<&str>) -> NetworkObservation {
        NetworkObservation {
            interface_kind: InterfaceKind::Wifi,
            wifi: WifiName::Known { ssid: ssid.into() },
            gateway_mac: gateway_mac.map(Into::into),
        }
    }

    fn rule(id: &str, conditions: NetworkConditions, actions: MechanismActions) -> NetworkRule {
        NetworkRule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            conditions,
            actions,
        }
    }

    fn ssid(ssid: &str) -> NetworkConditions {
        NetworkConditions {
            wifi_ssid: Some(ssid.into()),
            ..Default::default()
        }
    }

    const PROXY_OFF: MechanismActions = MechanismActions {
        system_proxy: MechanismAction::Disable,
        tun: MechanismAction::Keep,
        transparent_proxy: MechanismAction::Keep,
    };
    const PROXY_ON: MechanismActions = MechanismActions {
        system_proxy: MechanismAction::Enable,
        tun: MechanismAction::Keep,
        transparent_proxy: MechanismAction::Keep,
    };

    fn policy(rules: Vec<NetworkRule>) -> NetworkAutomation {
        NetworkAutomation {
            enabled: true,
            default_actions: PROXY_ON,
            rules,
        }
    }

    #[test]
    fn first_enabled_match_decides() {
        let mut skipped = rule("skipped", ssid("Home"), MechanismActions::default());
        skipped.enabled = false;
        let policy = policy(vec![
            skipped,
            rule("home", ssid("Home"), PROXY_OFF),
            rule("later", ssid("Home"), PROXY_ON),
        ]);

        assert_eq!(
            policy.evaluate(&wifi("Home", None)),
            NetworkEvaluation {
                matched: NetworkMatch::Rule { id: "home".into() },
                actions: PROXY_OFF,
            }
        );
    }

    #[test]
    fn unmatched_network_gets_the_default_actions() {
        let policy = policy(vec![rule("home", ssid("Home"), PROXY_OFF)]);

        assert_eq!(
            policy.evaluate(&wifi("Cafe", None)),
            NetworkEvaluation {
                matched: NetworkMatch::Default,
                actions: PROXY_ON,
            }
        );
    }

    #[test]
    fn every_present_condition_must_hold() {
        let conditions = NetworkConditions {
            wifi_ssid: Some("Home".into()),
            gateway_mac: Some("AA:BB:CC:DD:EE:FF".into()),
            interface_kind: Some(InterfaceKind::Wifi),
        };

        assert!(conditions.holds(&wifi("Home", Some(HOME_GATEWAY))));
        assert!(!conditions.holds(&wifi("Home", Some("11:22:33:44:55:66"))));
        assert!(!conditions.holds(&wifi("home", Some(HOME_GATEWAY))));
        assert!(!conditions.holds(&NetworkObservation {
            interface_kind: InterfaceKind::Ethernet,
            ..wifi("Home", Some(HOME_GATEWAY))
        }));
    }

    #[test]
    fn a_condition_over_a_missing_fact_never_holds() {
        let unnamed = NetworkObservation {
            interface_kind: InterfaceKind::Wifi,
            wifi: WifiName::Unavailable {
                reason: WifiUnavailable::PermissionDenied,
            },
            gateway_mac: None,
        };

        assert!(!ssid("Home").holds(&unnamed));
        assert!(!ssid("Home").holds(&NetworkObservation {
            wifi: WifiName::NotRead,
            ..unnamed.clone()
        }));
        assert!(
            !NetworkConditions {
                gateway_mac: Some(HOME_GATEWAY.into()),
                ..Default::default()
            }
            .holds(&unnamed)
        );
    }

    #[test]
    fn validation_names_the_rejected_part() {
        let both = MechanismActions {
            system_proxy: MechanismAction::Keep,
            tun: MechanismAction::Enable,
            transparent_proxy: MechanismAction::Enable,
        };
        let cases = [
            (
                policy(vec![rule("", ssid("Home"), PROXY_OFF)]),
                NetworkAutomationError::MissingId { index: 0 },
            ),
            (
                policy(vec![
                    rule("home", ssid("Home"), PROXY_OFF),
                    rule("home", ssid("Work"), PROXY_OFF),
                ]),
                NetworkAutomationError::DuplicateId { id: "home".into() },
            ),
            (
                policy(vec![NetworkRule {
                    name: "  ".into(),
                    ..rule("home", ssid("Home"), PROXY_OFF)
                }]),
                NetworkAutomationError::MissingName { index: 0 },
            ),
            (
                policy(vec![rule("home", NetworkConditions::default(), PROXY_OFF)]),
                NetworkAutomationError::MissingConditions {
                    name: "home".into(),
                },
            ),
            (
                policy(vec![rule("home", ssid(""), PROXY_OFF)]),
                NetworkAutomationError::EmptyWifiSsid {
                    name: "home".into(),
                },
            ),
            (
                policy(vec![rule(
                    "home",
                    NetworkConditions {
                        gateway_mac: Some("00:00:00:00:00:00".into()),
                        ..Default::default()
                    },
                    PROXY_OFF,
                )]),
                NetworkAutomationError::InvalidGatewayMac {
                    name: "home".into(),
                    value: "00:00:00:00:00:00".into(),
                },
            ),
            (
                policy(vec![rule("home", ssid("Home"), both)]),
                NetworkAutomationError::ConflictingCapture {
                    scope: "rule \"home\"".into(),
                },
            ),
            (
                NetworkAutomation {
                    default_actions: both,
                    ..Default::default()
                },
                NetworkAutomationError::ConflictingCapture {
                    scope: "the default".into(),
                },
            ),
            (
                policy(
                    (0..=MAX_NETWORK_RULES)
                        .map(|index| rule(&index.to_string(), ssid("Home"), PROXY_OFF))
                        .collect(),
                ),
                NetworkAutomationError::TooManyRules {
                    count: MAX_NETWORK_RULES + 1,
                },
            ),
        ];

        for (policy, expected) in cases {
            assert_eq!(policy.validate(), Err(expected));
        }
    }

    #[test]
    fn a_complete_policy_is_valid_and_keeps_its_order() {
        let policy = policy(vec![
            rule(
                "home",
                NetworkConditions {
                    gateway_mac: Some(HOME_GATEWAY.into()),
                    ..ssid("Home")
                },
                PROXY_OFF,
            ),
            rule("work", ssid("Work"), PROXY_ON),
        ]);

        assert_eq!(policy.validate(), Ok(()));
        let saved = serde_json::to_value(&policy).unwrap();
        assert_eq!(
            serde_json::from_value::<NetworkAutomation>(saved).unwrap(),
            policy
        );
    }

    #[test]
    fn only_an_enabled_policy_with_an_enabled_name_rule_reads_the_wifi_name() {
        let gateway_only = NetworkConditions {
            gateway_mac: Some(HOME_GATEWAY.into()),
            ..Default::default()
        };
        let mut disabled_rule = rule("home", ssid("Home"), PROXY_OFF);
        disabled_rule.enabled = false;

        assert!(policy(vec![rule("home", ssid("Home"), PROXY_OFF)]).reads_wifi_name());
        assert!(!policy(vec![rule("home", gateway_only, PROXY_OFF)]).reads_wifi_name());
        assert!(!policy(vec![disabled_rule]).reads_wifi_name());
        assert!(
            !NetworkAutomation {
                enabled: false,
                ..policy(vec![rule("home", ssid("Home"), PROXY_OFF)])
            }
            .reads_wifi_name()
        );
    }

    #[test]
    fn an_absent_policy_decodes_as_disabled() {
        let policy: NetworkAutomation = serde_json::from_str("{}").unwrap();

        assert_eq!(policy, NetworkAutomation::default());
        assert!(!policy.enabled);
    }
}
