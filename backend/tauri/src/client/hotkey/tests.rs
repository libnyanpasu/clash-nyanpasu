//! Pure parsing tests plus actor tests against fake ports: no OS call, no
//! window, no sleep. Every synchronisation point is an RPC reply or a recorded
//! call, never a clock.

use std::sync::{Arc, Mutex};

use super::{
    HotkeyArgs, HotkeyClient,
    ports::{
        HotkeyAction, HotkeyActionSink, HotkeyBindings, HotkeyOp, HotkeyParseError,
        MockHotkeyActionSink, ShortcutRegistrar,
    },
};
use crate::client::effects::status::{EffectHealth, EffectRevision};

fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

#[test]
fn parse_accepts_the_legacy_func_comma_key_format() {
    let bindings = HotkeyBindings::parse(&entries(&[
        "open_or_close_dashboard,Control+Q",
        " toggle_tun_mode , Control+Shift+T ",
    ]))
    .expect("both entries are well formed");

    assert_eq!(
        bindings.as_map().get("Control+Q"),
        Some(&HotkeyAction::OpenOrCloseDashboard)
    );
    assert_eq!(
        bindings.as_map().get("Control+Shift+T"),
        Some(&HotkeyAction::ToggleTunMode),
        "surrounding whitespace is not part of the accelerator"
    );
}

#[test]
fn parse_rejects_malformed_unknown_invalid_and_missing_super() {
    assert_eq!(
        HotkeyBindings::parse(&entries(&["open_or_close_dashboard"])),
        Err(HotkeyParseError::MalformedEntry(
            "open_or_close_dashboard".to_owned()
        ))
    );
    assert_eq!(
        HotkeyBindings::parse(&entries(&["make_coffee,Control+Q"])),
        Err(HotkeyParseError::UnknownFunction("make_coffee".to_owned()))
    );
    assert_eq!(
        HotkeyBindings::parse(&entries(&["toggle_tun_mode,Control++"])),
        Err(HotkeyParseError::InvalidAccelerator("Control++".to_owned()))
    );
    assert_eq!(
        HotkeyBindings::parse(&entries(&["toggle_tun_mode,Q"])),
        Err(HotkeyParseError::MissingSuperKey("Q".to_owned())),
        "a bare key would swallow ordinary typing"
    );
}

#[test]
fn parse_rejects_duplicate_accelerator() {
    assert_eq!(
        HotkeyBindings::parse(&entries(&[
            "enable_tun_mode,Control+Q",
            "disable_tun_mode,Control+Q",
        ])),
        Err(HotkeyParseError::DuplicateAccelerator(
            "Control+Q".to_owned()
        ))
    );
}

#[test]
fn diff_emits_unbind_rebind_and_bind() {
    let before = HotkeyBindings::from_pairs([
        ("Control+A", HotkeyAction::EnableTunMode),
        ("Control+B", HotkeyAction::EnableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
    ]);
    let after = HotkeyBindings::from_pairs([
        ("Control+B", HotkeyAction::DisableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
        ("Control+D", HotkeyAction::ToggleTunMode),
    ]);

    let ops = before.diff(&after);

    assert_eq!(
        ops,
        vec![
            HotkeyOp::Unbind {
                accelerator: "Control+A".to_owned()
            },
            HotkeyOp::Rebind {
                accelerator: "Control+B".to_owned(),
                action: HotkeyAction::DisableSystemProxy,
            },
            HotkeyOp::Bind {
                accelerator: "Control+D".to_owned(),
                action: HotkeyAction::ToggleTunMode,
            },
        ],
        "an unchanged binding produces nothing at all"
    );
}

/// Records every registrar call in order, so a test can assert on the sequence
/// rather than on a single expectation.
#[derive(Default)]
struct RecordingRegistrar {
    calls: Mutex<Vec<String>>,
    /// Accelerators whose `register` must fail.
    register_failures: Mutex<Vec<String>>,
    unregister_failures: Mutex<Vec<String>>,
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
            register_failures: Mutex::new(entries(accelerators)),
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

impl ShortcutRegistrar for RecordingRegistrar {
    fn validate(&self, _accelerator: &str) -> Result<(), HotkeyParseError> {
        Ok(())
    }

    fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> anyhow::Result<()> {
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
            anyhow::bail!("the os refused the shortcut");
        }
        *self.last_sink.lock().expect("sink") = Some(sink);
        *self.last_action.lock().expect("action") = Some(action);
        Ok(())
    }

    fn unregister(&self, accelerator: &str) -> anyhow::Result<()> {
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
            anyhow::bail!("the os kept the shortcut");
        }
        Ok(())
    }

    fn unregister_all(&self) -> anyhow::Result<()> {
        self.calls
            .lock()
            .expect("call log")
            .push("unregister_all".to_owned());
        Ok(())
    }
}

async fn client_with(registrar: Arc<RecordingRegistrar>) -> HotkeyClient {
    HotkeyClient::spawn(HotkeyArgs {
        registrar,
        sink: Arc::new(MockHotkeyActionSink::new()),
    })
    .await
    .expect("the hotkey actor should start")
}

fn bindings(raw: &[&str]) -> HotkeyBindings {
    HotkeyBindings::parse(&entries(raw)).expect("the fixture must parse")
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
            assert_eq!(code, "hotkey_partial_registration");
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
    let client = client_with(registrar.clone()).await;
    client
        .reconcile(
            EffectRevision::new(1),
            bindings(&["enable_tun_mode,Control+A"]),
        )
        .await;
    registrar.calls.lock().expect("call log").clear();

    let status = client.unregister_all().await;

    assert_eq!(status.health, EffectHealth::Healthy);
    assert_eq!(registrar.calls(), vec!["unregister_all".to_owned()]);
    assert!(client.status().await.registered.is_empty());
}

#[tokio::test]
async fn callback_dispatches_action_to_sink() {
    let registrar = RecordingRegistrar::new();
    let mut sink = MockHotkeyActionSink::new();
    sink.expect_dispatch()
        .withf(|action| *action == HotkeyAction::ToggleSystemProxy)
        .times(1)
        .return_const(());
    let client = HotkeyClient::spawn(HotkeyArgs {
        registrar: registrar.clone(),
        sink: Arc::new(sink),
    })
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

mod facade {
    //! What each action does once it reaches the client. The effect port is the
    //! no-op one, so these assert on committed configuration rather than on the
    //! OS.

    use std::sync::Arc;

    use nyanpasu_config::application::NyanpasuAppConfig;
    use tempfile::{TempDir, tempdir};

    use super::super::ports::{HotkeyAction, MockWindowControl};
    use crate::client::{
        NyanpasuClient,
        tests::{test_client_args_with_endpoint, test_idle_endpoint},
    };

    fn client_with_window(dir: &TempDir, window: MockWindowControl) -> NyanpasuClient {
        let mut args = test_client_args_with_endpoint(dir, test_idle_endpoint());
        args.window = Arc::new(window);
        NyanpasuClient::try_new_with_args(args).expect("client should construct")
    }

    fn client(dir: &TempDir) -> NyanpasuClient {
        client_with_window(dir, MockWindowControl::new())
    }

    /// A mode change would otherwise ask the core to drop connections, which
    /// the stub endpoint cannot answer.
    async fn disable_mode_interruption(client: &NyanpasuClient) {
        let mut config = client.get_clash_config().await.unwrap();
        config.break_connection.on_mode_change = false;
        client.replace_clash_config(config).await.unwrap();
    }

    #[test]
    fn dispatch_toggle_system_proxy_flips_app_config() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir);

        tauri::async_runtime::block_on(async {
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleSystemProxy)
                .await
                .expect("the toggle should commit");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleSystemProxy)
                .await
                .expect("the toggle should commit");
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::EnableSystemProxy)
                .await
                .expect("the explicit enable should commit");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::DisableSystemProxy)
                .await
                .expect("the explicit disable should commit");
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);
        });
    }

    #[test]
    fn dispatch_toggle_tun_mode_flips_clash_config() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir);

        tauri::async_runtime::block_on(async {
            let before = client.get_clash_config().await.unwrap().enable_tun_mode;

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleTunMode)
                .await
                .expect("the toggle should commit");
            assert_eq!(
                client.get_clash_config().await.unwrap().enable_tun_mode,
                !before
            );

            client
                .dispatch_hotkey_action(HotkeyAction::DisableTunMode)
                .await
                .expect("the explicit disable should commit");
            assert!(!client.get_clash_config().await.unwrap().enable_tun_mode);

            client
                .dispatch_hotkey_action(HotkeyAction::EnableTunMode)
                .await
                .expect("the explicit enable should commit");
            assert!(client.get_clash_config().await.unwrap().enable_tun_mode);
        });
    }

    #[test]
    fn dispatch_clash_mode_patches_runtime_overrides() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir);

        tauri::async_runtime::block_on(async {
            disable_mode_interruption(&client).await;

            client
                .dispatch_hotkey_action(HotkeyAction::ClashModeGlobal)
                .await
                .expect("the mode change should commit");

            assert_eq!(
                serde_json::to_value(client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
                "global"
            );
        });
    }

    #[test]
    fn dispatch_dashboard_goes_to_the_injected_window_control() {
        let dir = tempdir().expect("tempdir should be created");
        let mut window = MockWindowControl::new();
        window
            .expect_toggle_dashboard()
            .times(1)
            .returning(|| Ok(()));
        let client = client_with_window(&dir, window);

        tauri::async_runtime::block_on(async {
            client
                .dispatch_hotkey_action(HotkeyAction::OpenOrCloseDashboard)
                .await
                .expect("the dashboard toggle should succeed");
        });
    }

    #[test]
    fn invalid_hotkey_is_rejected_before_commit() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir);

        tauri::async_runtime::block_on(async {
            let mut patch = <NyanpasuAppConfig as struct_patch::Patch<_>>::new_empty_patch();
            patch.hotkeys = Some(vec!["toggle_tun_mode,Q".to_owned()]);

            let error = client
                .patch_app_config(patch)
                .await
                .expect_err("a hotkey without a modifier must not be persisted");
            assert!(
                error.to_string().contains("super key"),
                "unexpected error: {error}"
            );
            assert!(
                client.get_app_config().await.unwrap().hotkeys.is_empty(),
                "nothing may be written when validation fails"
            );
        });
    }
}
