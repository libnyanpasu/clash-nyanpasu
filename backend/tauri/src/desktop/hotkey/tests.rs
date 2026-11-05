//! Shortcut owner tests against recording ports, without OS calls.

use std::sync::{Arc, Mutex};

use nyanpasu_core::client::{
    effects::status::{EffectFailureCode, EffectHealth, EffectRevision},
    hotkey::ports::{HotkeyAction, HotkeyBindings, HotkeyParseError},
    test_support::AnyAccelerator,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    HotkeyArgs, HotkeyClient,
    ports::{HotkeyActionSink, MockHotkeyActionSink, ShortcutError, ShortcutRegistrar},
};

/// Records every registrar call in order, so a test can assert on the sequence
/// rather than on a single expectation.
#[derive(Default)]
struct RecordingRegistrar {
    calls: Mutex<Vec<String>>,
    /// Accelerators whose `register` must fail.
    register_failures: Mutex<Vec<String>>,
    unregister_failures: Mutex<Vec<String>>,
    /// Accelerators the platform parser refuses.
    invalid: Mutex<Vec<String>>,
    /// The sink handed to the last successful `register`.
    last_sink: Mutex<Option<Arc<dyn HotkeyActionSink>>>,
    last_action: Mutex<Option<HotkeyAction>>,
}

impl RecordingRegistrar {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn failing_register(accelerators: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            register_failures: Mutex::new(accelerators.iter().map(ToString::to_string).collect()),
            ..Self::default()
        })
    }

    fn refusing(accelerators: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            invalid: Mutex::new(accelerators.iter().map(ToString::to_string).collect()),
            ..Self::default()
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("call log").clone()
    }

    fn fire_last_shortcut(&self) {
        let sink = self.last_sink.lock().expect("sink").clone();
        let action = self.last_action.lock().expect("action").expect("an action");
        sink.expect("a registered sink").dispatch(action);
    }
}

#[async_trait::async_trait]
impl ShortcutRegistrar for RecordingRegistrar {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        if self
            .invalid
            .lock()
            .expect("invalid")
            .iter()
            .any(|refused| refused == accelerator)
        {
            return Err(HotkeyParseError::EmptyKeySegment {
                accelerator: accelerator.to_owned(),
            });
        }
        Ok(())
    }

    async fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> Result<(), ShortcutError> {
        self.calls
            .lock()
            .expect("call log")
            .push(format!("register:{accelerator}"));
        if self
            .register_failures
            .lock()
            .expect("failures")
            .iter()
            .any(|failing| failing == accelerator)
        {
            return Err(ShortcutError::RegisterShortcut {
                accelerator: accelerator.to_owned(),
                source: "the os refused the shortcut".into(),
            });
        }
        *self.last_sink.lock().expect("sink") = Some(sink);
        *self.last_action.lock().expect("action") = Some(action);
        Ok(())
    }

    async fn unregister(&self, accelerator: &str) -> Result<(), ShortcutError> {
        self.calls
            .lock()
            .expect("call log")
            .push(format!("unregister:{accelerator}"));
        if self
            .unregister_failures
            .lock()
            .expect("failures")
            .iter()
            .any(|failing| failing == accelerator)
        {
            return Err(ShortcutError::ReleaseShortcut {
                accelerator: accelerator.to_owned(),
                source: "the os kept the shortcut".into(),
            });
        }
        Ok(())
    }

    async fn unregister_all(&self) -> Result<(), ShortcutError> {
        self.calls
            .lock()
            .expect("call log")
            .push("unregister_all".to_owned());
        Ok(())
    }
}

async fn client_with(registrar: Arc<RecordingRegistrar>) -> HotkeyClient {
    owned_client(registrar, &CancellationToken::new(), &TaskTracker::new()).await
}

/// The same client, which releases its grabs once `shutdown` is cancelled.
async fn owned_client(
    registrar: Arc<RecordingRegistrar>,
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
) -> HotkeyClient {
    HotkeyClient::spawn(
        HotkeyArgs {
            registrar,
            sink: Arc::new(MockHotkeyActionSink::new()),
            shutdown: shutdown.clone(),
        },
        tasks,
    )
    .await
    .expect("the hotkey actor should start")
}

fn bindings(raw: &[&str]) -> HotkeyBindings {
    HotkeyBindings::parse(
        &raw.iter().map(ToString::to_string).collect::<Vec<_>>(),
        &AnyAccelerator,
    )
    .expect("the fixture must parse")
}

#[tokio::test]
async fn update_unregisters_before_registering() {
    let registrar = RecordingRegistrar::new();
    let client = client_with(registrar.clone()).await;

    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A", "enable_system_proxy,Control+B"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    // The two accelerators swap functions: registering before releasing would
    // ask the OS for a grab it is already holding.
    client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_tun_mode,Control+B", "enable_system_proxy,Control+A"]),
        )
        .await;

    let calls = registrar.calls();
    let first_register = calls
        .iter()
        .position(|call| call.starts_with("register:"))
        .expect("something was registered");
    let last_unregister = calls
        .iter()
        .rposition(|call| call.starts_with("unregister:"))
        .expect("something was unregistered");
    assert!(
        last_unregister < first_register,
        "every release must precede every grab, got {calls:?}"
    );
}

#[tokio::test]
async fn unchanged_bindings_produce_no_os_calls() {
    let registrar = RecordingRegistrar::new();
    let client = client_with(registrar.clone()).await;
    let desired = bindings(&["enable_tun_mode,Control+A"]);

    client
        .reconcile(EffectRevision::new(1), desired.clone())
        .await;
    registrar.calls.lock().expect("call log").clear();

    let status = client.reconcile(EffectRevision::new(2), desired).await;

    assert_eq!(status.health, EffectHealth::Healthy);
    assert!(
        registrar.calls().is_empty(),
        "an unchanged list must not touch the OS"
    );
}

#[tokio::test]
async fn partial_registration_failure_degrades_and_keeps_successes() {
    let registrar = RecordingRegistrar::failing_register(&["Control+B"]);
    let client = client_with(registrar.clone()).await;

    let status = client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A", "enable_system_proxy,Control+B"]),
        )
        .await;

    match status.health {
        EffectHealth::Degraded {
            code,
            ref message,
            retryable,
        } => {
            assert_eq!(code, EffectFailureCode::HotkeyPartialRegistration);
            assert!(message.contains("1 of 2"), "{message}");
            assert!(message.contains("Control+B"), "{message}");
            assert!(retryable);
        }
        other => panic!("a refused grab must degrade, got {other:?}"),
    }

    let actor = client.status().await;
    assert_eq!(actor.applied_revision, EffectRevision::new(1));
    assert_eq!(actor.health, status.health);
    assert_eq!(
        actor.registered.get("Control+A"),
        Some(&HotkeyAction::EnableTunMode)
    );
    assert!(
        !actor.registered.contains_key("Control+B"),
        "a grab the OS refused is not in place"
    );
}

#[tokio::test]
async fn a_refused_grab_is_retried_by_the_next_reconcile() {
    let registrar = RecordingRegistrar::failing_register(&["Control+B"]);
    let client = client_with(registrar.clone()).await;
    let desired = bindings(&["enable_system_proxy,Control+B"]);

    client
        .reconcile(EffectRevision::new(1), desired.clone())
        .await;
    registrar
        .register_failures
        .lock()
        .expect("failures")
        .clear();
    registrar.calls.lock().expect("call log").clear();

    let status = client.reconcile(EffectRevision::new(2), desired).await;

    assert_eq!(status.health, EffectHealth::Healthy);
    assert_eq!(registrar.calls(), vec!["register:Control+B".to_owned()]);
}

/// The old check sat inside `register`, which runs after every release, so a
/// single unparsable accelerator tore down the shortcuts that did work.
#[tokio::test]
async fn reconcile_validates_every_binding_before_releasing_any() {
    let registrar = RecordingRegistrar::refusing(&["Control+DefinitelyNotAKey"]);
    let client = client_with(registrar.clone()).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    let status = client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_system_proxy,Control+DefinitelyNotAKey"]),
        )
        .await;

    match status.health {
        EffectHealth::Degraded {
            code,
            ref message,
            retryable,
        } => {
            assert_eq!(code, EffectFailureCode::HotkeyInvalidBindings);
            assert!(message.contains("Control+DefinitelyNotAKey"), "{message}");
            assert!(!retryable, "the list has to change before a retry can help");
        }
        other => panic!("an unparsable accelerator must degrade, got {other:?}"),
    }
    assert!(
        registrar.calls().is_empty(),
        "nothing may be released or grabbed, got {:?}",
        registrar.calls()
    );
    assert_eq!(
        client.status().await.registered.get("Control+A"),
        Some(&HotkeyAction::EnableTunMode),
        "the working shortcut must survive a rejected list"
    );
}

#[tokio::test]
async fn stale_revision_is_superseded() {
    let registrar = RecordingRegistrar::new();
    let client = client_with(registrar.clone()).await;

    client
        .reconcile(
            EffectRevision::new(5),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    let status = client
        .reconcile(
            EffectRevision::new(3),
            bindings(&["enable_system_proxy,Control+Z"]),
        )
        .await;

    assert_eq!(status.health, EffectHealth::Superseded);
    assert_eq!(status.applied_revision, EffectRevision::new(5));
    assert!(
        registrar.calls().is_empty(),
        "an older desired state must not reach the OS"
    );
}

#[tokio::test]
async fn exit_unregisters_all() {
    let registrar = RecordingRegistrar::new();
    let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
    let client = owned_client(registrar.clone(), &shutdown, &tasks).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;

    assert_eq!(registrar.calls(), vec!["unregister_all".to_owned()]);
}

#[tokio::test]
async fn reconcile_after_the_cancel_is_rejected() {
    let registrar = RecordingRegistrar::new();
    let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
    let client = owned_client(registrar.clone(), &shutdown, &tasks).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    shutdown.cancel();
    tasks.close();
    // Sent before this test yields, so it is queued ahead of the drain, the
    // way a plan admitted before the shutdown can still be in flight.
    let status = client
        .reconcile(
            EffectRevision::new(2),
            bindings(&["enable_system_proxy,Control+B"]),
        )
        .await;

    assert_eq!(
        status.health,
        EffectHealth::Degraded {
            code: EffectFailureCode::HotkeyShutDown,
            message: "the hotkey owner is shutting down and stopped accepting changes".to_owned(),
            retryable: false,
        }
    );
    tasks.wait().await;
    assert_eq!(
        registrar.calls(),
        vec!["unregister_all".to_owned()],
        "an exiting process must not take a grab it will never give back"
    );
}

#[tokio::test]
async fn callback_dispatches_action_to_sink() {
    let registrar = RecordingRegistrar::new();
    let mut sink = MockHotkeyActionSink::new();
    sink.expect_dispatch()
        .withf(|action| *action == HotkeyAction::ToggleSystemProxy)
        .times(1)
        .return_const(());
    let client = HotkeyClient::spawn(
        HotkeyArgs {
            registrar: registrar.clone(),
            sink: Arc::new(sink),
            shutdown: CancellationToken::new(),
        },
        &TaskTracker::new(),
    )
    .await
    .expect("the hotkey actor should start");

    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["toggle_system_proxy,Control+A"]),
        )
        .await;

    // Stands in for the OS callback the adapter installs.
    registrar.fire_last_shortcut();
}
