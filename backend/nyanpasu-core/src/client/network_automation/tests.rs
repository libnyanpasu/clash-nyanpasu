//! The automation actor through its typed client with fake ports, and the
//! capture switches against real configuration owners.

use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use nyanpasu_config::clash::config::clash_strategy::{PortStrategy, PortStrategyKind};
use nyanpasu_config::{
    application::{
        InterfaceKind, MechanismAction, MechanismActions, NetworkAutomation, NetworkConditions,
        NetworkMatch, NetworkObservation, NetworkRule, NetworkState, NyanpasuAppConfigPatch,
        WifiName,
    },
    clash::config::{ClashConfig, TransparentProxyMode},
};
use nyanpasu_contracts::network_environment::NetworkEnvironmentPort;
use nyanpasu_runtime_values::network_automation::{
    MechanismOutcome, MechanismOutcomes, NetworkAutomationStatus,
};
use tempfile::TempDir;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    ConfigCaptureSwitches, NetworkAutomationArgs, NetworkAutomationClient, ports::CaptureSwitches,
};
use crate::{
    actor_v2::test_support::test_v2_clients,
    client::{
        ClientError,
        test_support::{facade::test_client, test_typed_config_clients},
    },
    state::config_error::ConfigError,
};

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

fn wifi(ssid: &str) -> NetworkState {
    NetworkState::Online {
        network: NetworkObservation {
            interface_kind: InterfaceKind::Wifi,
            wifi: WifiName::Known { ssid: ssid.into() },
            gateway_mac: None,
        },
    }
}

fn home_rule(actions: MechanismActions) -> NetworkRule {
    NetworkRule {
        id: "home".into(),
        name: "Home".into(),
        enabled: true,
        conditions: NetworkConditions {
            wifi_ssid: Some("Home".into()),
            ..Default::default()
        },
        actions,
    }
}

/// Home switches the proxy off; every other network switches it on.
fn policy() -> NetworkAutomation {
    NetworkAutomation {
        enabled: true,
        default_actions: PROXY_ON,
        rules: vec![home_rule(PROXY_OFF)],
    }
}

fn home() -> NetworkMatch {
    NetworkMatch::Rule { id: "home".into() }
}

/// The network the test says the machine is on.
struct FakeEnvironment(Mutex<NetworkState>);

impl NetworkEnvironmentPort for FakeEnvironment {
    fn current(&self) -> NetworkState {
        self.0.lock().unwrap().clone()
    }
}

/// Records every action set it is asked to apply.
#[derive(Default)]
struct RecordingSwitches(Mutex<Vec<MechanismActions>>);

#[async_trait::async_trait]
impl CaptureSwitches for RecordingSwitches {
    async fn apply(&self, actions: MechanismActions) -> MechanismOutcomes {
        self.0.lock().unwrap().push(actions);
        MechanismOutcomes {
            system_proxy: MechanismOutcome::Enabled,
            tun: MechanismOutcome::Unchanged,
            transparent_proxy: MechanismOutcome::Unchanged,
        }
    }
}

struct Fixture {
    client: NetworkAutomationClient,
    environment: Arc<FakeEnvironment>,
    switches: Arc<RecordingSwitches>,
}

impl Fixture {
    async fn spawn(policy: NetworkAutomation, network: NetworkState) -> Self {
        let environment = Arc::new(FakeEnvironment(Mutex::new(network)));
        let switches = Arc::new(RecordingSwitches::default());
        let client = NetworkAutomationClient::spawn(
            NetworkAutomationArgs {
                environment: environment.clone(),
                switches: switches.clone(),
                policy,
                schedule_polls: false,
                shutdown: CancellationToken::new(),
            },
            &TaskTracker::new(),
        )
        .await
        .unwrap();

        Self {
            client,
            environment,
            switches,
        }
    }

    /// Moves the machine and polls until the reading settles.
    async fn arrive(&self, network: NetworkState) {
        *self.environment.0.lock().unwrap() = network;
        self.client.poll().await;
        self.client.poll().await;
    }

    async fn set_policy(&self, policy: NetworkAutomation) -> NetworkAutomationStatus {
        self.client.set_policy(policy);
        self.client.status().await
    }

    fn applied(&self) -> Vec<MechanismActions> {
        self.switches.0.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn a_network_applies_once_after_two_agreeing_polls() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.client.poll().await;
    assert_eq!(fixture.applied(), [], "one reading has not settled");

    fixture.client.poll().await;
    fixture.client.poll().await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    let status = fixture.client.status().await;
    assert!(status.supported);
    assert_eq!(status.network, Some(wifi("Home")));
    assert_eq!(status.applied.unwrap().matched, home());
}

#[tokio::test]
async fn readings_that_keep_changing_apply_nothing() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    for network in [wifi("Cafe"), wifi("Home"), wifi("Cafe"), wifi("Home")] {
        *fixture.environment.0.lock().unwrap() = network;
        fixture.client.poll().await;
    }

    assert_eq!(fixture.applied(), []);
}

#[tokio::test]
async fn each_arrival_on_a_different_match_applies_its_actions() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    fixture.arrive(wifi("Cafe")).await;
    // Another unmatched network is the same match: nothing is written, so a
    // manual switch made at the first one stands.
    fixture.arrive(wifi("Airport")).await;
    fixture.arrive(wifi("Home")).await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON, PROXY_OFF]);
}

#[tokio::test]
async fn offline_holds_the_switches_until_a_different_network() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    fixture.arrive(NetworkState::Offline).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    fixture.arrive(NetworkState::Offline).await;
    fixture.arrive(wifi("Cafe")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON]);
}

#[tokio::test]
async fn a_policy_edit_applies_only_a_changed_answer() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;

    let mut renamed = policy();
    renamed.rules[0].name = "My flat".into();
    renamed.default_actions = MechanismActions::default();
    fixture.set_policy(renamed).await;
    assert_eq!(
        fixture.applied(),
        [PROXY_OFF],
        "the answer for Home is the same"
    );

    let mut tun_too = policy();
    tun_too.rules[0].actions.tun = MechanismAction::Disable;
    fixture.set_policy(tun_too.clone()).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, tun_too.rules[0].actions]);
}

#[tokio::test]
async fn a_disabled_policy_observes_without_writing() {
    let disabled = NetworkAutomation {
        enabled: false,
        ..policy()
    };
    let fixture = Fixture::spawn(disabled.clone(), wifi("Home")).await;

    fixture.arrive(wifi("Home")).await;
    let status = fixture.client.apply_now().await;
    assert_eq!(fixture.applied(), []);
    assert_eq!(
        status.network,
        Some(wifi("Home")),
        "the editor still sees it"
    );
    assert_eq!(status.applied, None);

    fixture.set_policy(policy()).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF]);

    // Disabling forgets the application, so enabling again applies again.
    let status = fixture.set_policy(disabled).await;
    assert_eq!(status.applied, None);
    fixture.set_policy(policy()).await;
    fixture.arrive(wifi("Home")).await;
    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_OFF]);
}

#[tokio::test]
async fn apply_now_writes_the_same_answer_again() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;

    let status = fixture.client.apply_now().await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_OFF]);
    assert_eq!(
        status.applied.unwrap().outcomes.system_proxy,
        MechanismOutcome::Enabled
    );
}

#[tokio::test]
async fn apply_now_reads_the_network_without_waiting_to_settle() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.arrive(wifi("Home")).await;
    *fixture.environment.0.lock().unwrap() = wifi("Cafe");

    let status = fixture.client.apply_now().await;

    assert_eq!(fixture.applied(), [PROXY_OFF, PROXY_ON]);
    assert_eq!(status.applied.unwrap().matched, NetworkMatch::Default);
}

#[tokio::test]
async fn a_stopped_actor_reports_an_unsupported_status() {
    let fixture = Fixture::spawn(policy(), wifi("Home")).await;
    fixture.client.actor.stop(None);
    fixture.client.actor.wait(None).await.unwrap();

    assert_eq!(
        fixture.client.status().await,
        NetworkAutomationStatus::unsupported()
    );
}

async fn config_switches(dir: &TempDir) -> ConfigCaptureSwitches {
    let (application, _session_state, clash_config) = test_typed_config_clients(dir).await;
    let (core, _service) = test_v2_clients().await;
    ConfigCaptureSwitches {
        application,
        clash_config,
        core,
    }
}

#[tokio::test]
async fn system_proxy_action_writes_the_switch_once() {
    let dir = TempDir::new().unwrap();
    let switches = config_switches(&dir).await;

    let outcomes = switches.apply(PROXY_ON).await;
    assert_eq!(outcomes.system_proxy, MechanismOutcome::Enabled);
    assert!(switches.application.snapshot().state.enable_system_proxy);

    let outcomes = switches.apply(PROXY_ON).await;
    assert_eq!(outcomes.system_proxy, MechanismOutcome::Unchanged);

    let outcomes = switches.apply(PROXY_OFF).await;
    assert_eq!(outcomes.system_proxy, MechanismOutcome::Disabled);
    assert!(!switches.application.snapshot().state.enable_system_proxy);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn enabling_tun_turns_a_kept_transparent_proxy_off() {
    let dir = TempDir::new().unwrap();
    let switches = config_switches(&dir).await;
    let mut config: ClashConfig = switches.clash_config.snapshot().state;
    config.transparent_proxy.mode = TransparentProxyMode::Tproxy;
    config.tproxy_port = Some(PortStrategy {
        kind: PortStrategyKind::Fixed,
        start_port: 7894,
    });
    switches.clash_config.replace(config).await.unwrap();

    let outcomes = switches
        .apply(MechanismActions {
            tun: MechanismAction::Enable,
            ..Default::default()
        })
        .await;

    assert_eq!(
        outcomes,
        MechanismOutcomes {
            system_proxy: MechanismOutcome::Unchanged,
            tun: MechanismOutcome::Enabled,
            transparent_proxy: MechanismOutcome::Disabled,
        }
    );
    let config = switches.clash_config.snapshot().state;
    assert!(config.enable_tun_mode);
    assert_eq!(
        config.transparent_proxy.mode,
        TransparentProxyMode::Disabled
    );
}

#[tokio::test]
async fn an_unsupported_transparent_proxy_fails_alone_and_keeps_tun() {
    let dir = TempDir::new().unwrap();
    let switches = config_switches(&dir).await;
    let mut config: ClashConfig = switches.clash_config.snapshot().state;
    config.enable_tun_mode = true;
    switches.clash_config.replace(config).await.unwrap();

    let outcomes = switches
        .apply(MechanismActions {
            system_proxy: MechanismAction::Enable,
            tun: MechanismAction::Keep,
            transparent_proxy: MechanismAction::Enable,
        })
        .await;

    assert_eq!(outcomes.system_proxy, MechanismOutcome::Enabled);
    assert_eq!(outcomes.tun, MechanismOutcome::Unchanged);
    assert!(matches!(
        outcomes.transparent_proxy,
        MechanismOutcome::Failed { .. }
    ));
    let config = switches.clash_config.snapshot().state;
    assert!(config.enable_tun_mode, "nothing replaced TUN");
    assert_eq!(
        config.transparent_proxy.mode,
        TransparentProxyMode::Disabled
    );
}

#[tokio::test]
async fn a_host_without_an_observer_rejects_enabling_and_saves_a_disabled_policy() {
    let dir = TempDir::new().unwrap();
    let client = test_client(&dir).await;
    let patch = |policy: NetworkAutomation| NyanpasuAppConfigPatch {
        network_automation: Some(policy),
        ..Default::default()
    };

    let error = client.patch_app_config(patch(policy())).await.unwrap_err();
    assert!(matches!(
        error,
        ClientError::Config(ConfigError::InvalidNetworkAutomation { .. })
    ));
    assert_eq!(
        client.get_network_automation_status().await,
        NetworkAutomationStatus::unsupported()
    );
    assert_eq!(
        client.apply_network_automation().await,
        NetworkAutomationStatus::unsupported()
    );

    let disabled = NetworkAutomation {
        enabled: false,
        ..policy()
    };
    client
        .patch_app_config(patch(disabled.clone()))
        .await
        .unwrap();
    assert_eq!(
        client.get_app_config().await.unwrap().network_automation,
        disabled
    );

    let mut unnamed = disabled;
    unnamed.rules[0].name.clear();
    let error = client.patch_app_config(patch(unnamed)).await.unwrap_err();
    assert!(matches!(
        error,
        ClientError::Config(ConfigError::InvalidNetworkAutomation { reason })
            if reason == "rule 0 has no name"
    ));
}
