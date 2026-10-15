//! Post-commit isolation and coalescing, using explicit actor acknowledgements.
use super::{
    actor::{EffectsArgs, EffectsClient, EffectsSnapshot},
    plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
        SystemProxyDesired, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    NyanpasuClient, UiEventSink,
    tests::{TestControlEndpoint, test_client_args_with_endpoint},
};
use nyanpasu_config::{
    application::{I18nLanguage, NyanpasuAppConfig},
    clash::config::ClashConfig,
    runtime::executor::ResolvedPortBindings,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use struct_patch::Patch;
use tokio::sync::Notify;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Default)]
struct Ui;
impl UiEventSink for Ui {
    fn state_changed(&self, _: crate::client::StateChanged) {}
}

#[derive(Default)]
struct Port {
    calls: Mutex<Vec<(EffectRevision, Vec<ApplicationEffect>)>>,
    block: Option<EffectKind>,
    entered: Notify,
    release: Notify,
    blocked: AtomicBool,
    fail: AtomicBool,
    fail_kind: Option<EffectKind>,
    retryable: bool,
}
#[async_trait::async_trait]
impl ApplicationEffectsPort for Port {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        self.calls
            .lock()
            .unwrap()
            .push((revision, plan.effects().to_vec()));
        if plan.effects().iter().any(|e| Some(e.kind()) == self.block)
            && !self.blocked.swap(true, Ordering::SeqCst)
        {
            self.entered.notify_one();
            self.release.notified().await;
        }
        plan.effects()
            .iter()
            .map(|e| EffectStatus {
                kind: e.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: if matches!(e, ApplicationEffect::SystemProxy(desired) if desired.enabled && desired.port.is_none()) {
                    EffectHealth::Degraded { code: EffectFailureCode::SystemProxyPortUnresolved, message: "no binding".into(), retryable: true }
                } else if self.fail.load(Ordering::SeqCst) && self.fail_kind.is_none_or(|kind| kind == e.kind()) {
                    EffectHealth::Degraded {
                        code: EffectFailureCode::LoggerRefreshFailed,
                        message: "failed".into(),
                        retryable: self.retryable,
                    }
                } else {
                    EffectHealth::Healthy
                },
            })
            .collect()
    }
}
fn inputs() -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(&NyanpasuAppConfig::default(), &ClashConfig::default(), None)
}
/// The effects owner's share of the root shutdown.
struct Shutdown {
    token: CancellationToken,
    tasks: TaskTracker,
}
impl Shutdown {
    fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }
    fn request(&self) {
        self.token.cancel();
        self.tasks.close();
    }
    async fn run(&self) {
        self.request();
        self.tasks.wait().await;
    }
}
async fn graph(port: Arc<dyn ApplicationEffectsPort>) -> (EffectsClient, Shutdown) {
    let shutdown = Shutdown::new();
    let client = EffectsClient::spawn(
        EffectsArgs {
            port,
            ui: Arc::new(Ui),
            initial: inputs(),
            shutdown: shutdown.token.clone(),
        },
        &shutdown.tasks,
    )
    .await
    .unwrap();
    (client, shutdown)
}
async fn wait(
    client: &EffectsClient,
    predicate: impl Fn(&EffectsSnapshot) -> bool,
) -> EffectsSnapshot {
    let mut status = client.subscribe();
    tokio::time::timeout(Duration::from_secs(5), status.wait_for(predicate))
        .await
        .unwrap()
        .unwrap()
        .clone()
}
fn language(language: I18nLanguage) -> nyanpasu_config::application::NyanpasuAppConfigPatch {
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.language = Some(language);
    patch
}

#[tokio::test]
async fn blocked_pac_does_not_block_visual_or_hotkey_group() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    let state = wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::Tray && s.health == EffectHealth::Healthy)
            && s.effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == EffectKind::Hotkeys && s.health == EffectHealth::Healthy)
    })
    .await;
    for kind in [EffectKind::SystemProxy, EffectKind::AutoLaunch] {
        assert!(
            state
                .effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == kind && s.health == EffectHealth::Pending),
            "{kind:?} shares the held system proxy owner"
        );
    }
    port.release.notify_one();
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    shutdown.run().await;
}

#[tokio::test]
async fn blocked_gui_does_not_block_proxy_group() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Tray),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::SystemProxy && s.health == EffectHealth::Healthy)
    })
    .await;
    port.release.notify_one();
    shutdown.run().await;
}

#[tokio::test]
async fn queued_visual_targets_coalesce_and_full_subsumes_part() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Locale),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), Vec::new());
    port.entered.notified().await;
    desired.app.language = I18nLanguage::English;
    client.application_committed(desired.app.clone(), Vec::new());
    desired.app.enable_tray_text = !desired.app.enable_tray_text;
    client.application_committed(desired.app.clone(), Vec::new());
    client.barrier().await;
    assert_eq!(port.calls.lock().unwrap().len(), 1);
    port.release.notify_one();
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    let calls = port.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].1,
        vec![
            ApplicationEffect::Locale(I18nLanguage::English),
            // Widened to the queued rebuild, rendered from the newest view.
            ApplicationEffect::Tray(TrayRefresh::Full, desired.tray_view())
        ]
    );
    drop(calls);
    shutdown.run().await;
}

/// The newest `kind` effect the owners were handed, once every slice sent so
/// far has been planned and the newest plan for `kind` applied.
async fn last_applied(client: &EffectsClient, port: &Port, kind: EffectKind) -> ApplicationEffect {
    client.barrier().await;
    wait(client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == kind && s.health == EffectHealth::Healthy)
    })
    .await;
    let applied = port
        .calls
        .lock()
        .unwrap()
        .iter()
        .rev()
        .flat_map(|(_, effects)| effects)
        .find(|effect| effect.kind() == kind)
        .cloned();
    applied.expect("the effect was planned")
}

/// V29: the application and the Runtime each send their own slice, and the
/// two interleave. Each replaces only its own part of what the effects owner
/// holds, so the system proxy, which reads both, gets the newest of each.
#[tokio::test]
async fn interleaved_slices_from_two_owners_each_keep_their_newest_value() {
    let port = Arc::new(Port::default());
    let (client, shutdown) = graph(port.clone()).await;
    let bound = |mixed_port| {
        Some(ResolvedPortBindings {
            mixed_port,
            ..ResolvedPortBindings::default()
        })
    };
    let mut app = inputs().app;
    app.enable_system_proxy = true;
    app.system_proxy_bypass = "first".into();
    client.application_committed(app.clone(), Vec::new());
    client.runtime_bound(bound(7890), false);
    app.system_proxy_bypass = "second".into();
    client.application_committed(app.clone(), Vec::new());
    assert_eq!(
        last_applied(&client, &port, EffectKind::SystemProxy).await,
        ApplicationEffect::SystemProxy(SystemProxyDesired {
            enabled: true,
            bypass: "second".into(),
            port: Some(7890),
            pac_url: None,
        }),
        "the application's slice keeps the ports the Runtime sent"
    );

    client.runtime_bound(bound(7891), false);
    assert_eq!(
        last_applied(&client, &port, EffectKind::SystemProxy).await,
        ApplicationEffect::SystemProxy(SystemProxyDesired {
            enabled: true,
            bypass: "second".into(),
            port: Some(7891),
            pac_url: None,
        }),
        "the Runtime's slice keeps the application's"
    );
    shutdown.run().await;
}

#[tokio::test]
async fn stale_completion_does_not_publish_success_for_new_desired() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.system_proxy_bypass = "old".into();
    client.application_committed(desired.app.clone(), Vec::new());
    port.entered.notified().await;
    desired.app.system_proxy_bypass = "new".into();
    client.application_committed(desired.app, Vec::new());
    client.barrier().await;
    let revision = client.snapshot().revision;
    port.fail.store(true, Ordering::SeqCst);
    port.release.notify_one();
    let state = wait(&client, |s| {
        s.effects.iter().map(|e| &e.status).any(|s| {
            s.kind == EffectKind::SystemProxy && matches!(s.health, EffectHealth::Degraded { .. })
        })
    })
    .await;
    let status = state
        .effects
        .iter()
        .map(|e| &e.status)
        .find(|s| s.kind == EffectKind::SystemProxy)
        .unwrap();
    assert_eq!(status.desired_revision.get(), revision);
    assert_eq!(status.applied_revision.get(), 0);
    assert_eq!(port.calls.lock().unwrap().len(), 2);
    shutdown.run().await;
}

#[tokio::test]
async fn independent_graphs_and_shutdown_admission() {
    let a = Arc::new(Port::default());
    let b = Arc::new(Port::default());
    let (left, left_shutdown) = graph(a.clone()).await;
    let (right, right_shutdown) = graph(b.clone()).await;
    left_shutdown.request();
    // Queued before this test yields, so ahead of the drain: only the token
    // keeps it from reaching the owners.
    left.publish_full(None);
    left_shutdown.tasks.wait().await;
    assert!(a.calls.lock().unwrap().is_empty());
    right.publish_full(None);
    wait(&right, |s| {
        s.effects.len() == 11
            && s.effects
                .iter()
                .map(|e| &e.status)
                .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    assert_eq!(b.calls.lock().unwrap().len(), 4);
    right_shutdown.run().await;
}

#[test]
fn source_commit_and_second_save_do_not_wait_for_gui() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        block: Some(EffectKind::Tray),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            client.patch_app_config(language(I18nLanguage::Korean)),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(first.degradations().is_empty());
        port.entered.notified().await;
        tokio::time::timeout(
            Duration::from_secs(5),
            client.patch_app_config(language(I18nLanguage::English)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            client.get_app_config().await.unwrap().language,
            I18nLanguage::English
        );
        port.release.notify_one();
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[test]
fn asynchronous_failure_is_status_and_never_cancels_source() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port;
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let result = client
            .patch_app_config(language(I18nLanguage::Korean))
            .await
            .unwrap();
        assert!(result.degradations().is_empty());
        let status = wait(&client.inner.effects, |s| {
            s.effects
                .iter()
                .map(|e| &e.status)
                .any(|s| matches!(s.health, EffectHealth::Degraded { .. }))
        })
        .await;
        assert!(
            status
                .effects
                .iter()
                .map(|e| &e.status)
                .all(|s| s.applied_revision.get() == 0)
        );
        assert_eq!(
            client.get_app_config().await.unwrap().language,
            I18nLanguage::Korean
        );
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[test]
fn no_op_and_session_saves_do_not_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let current = client.get_app_config().await.unwrap().language;
        client.patch_app_config(language(current)).await.unwrap();
        client
            .save_main_window_geometry(nyanpasu_config::state::window::WindowState {
                width: 800,
                height: 600,
                x: 0,
                y: 0,
                maximized: false,
                fullscreen: false,
            })
            .await
            .unwrap();
        client.inner.effects.barrier().await;
        assert!(port.calls.lock().unwrap().is_empty());
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[test]
fn rejected_source_never_dispatches() {
    use crate::{bundle::Channel, client::ClientError, state::config_error::ConfigError};
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    args.bundle_metadata.release_channel = Channel::Nightly;
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let mut nightly = NyanpasuAppConfig::new_empty_patch();
        nightly.release_channel = Some(Some(Channel::Nightly));
        client.patch_app_config(nightly).await.unwrap();
        client.inner.effects.barrier().await;
        port.calls.lock().unwrap().clear();
        // The nightly build refuses to leave the nightly channel, so the
        // language change riding on the same patch is never committed.
        let mut rejected = language(I18nLanguage::Korean);
        rejected.release_channel = Some(Some(Channel::Stable));
        assert!(matches!(
            client.patch_app_config(rejected).await,
            Err(ClientError::Config(ConfigError::LeaveNightlyChannel {
                to: Channel::Stable
            }))
        ));
        client.inner.effects.barrier().await;
        assert!(port.calls.lock().unwrap().is_empty());
        assert_ne!(
            client.get_app_config().await.unwrap().language,
            I18nLanguage::Korean
        );
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

/// The clash config and profiles owners hand the effects owner their own
/// slices: a mode change reaches the tray from the clash config owner, and a
/// profiles commit asks the tray for a partial refresh.
#[test]
fn clash_and_profiles_owners_hand_their_own_slices_to_the_tray() {
    use crate::client::tests::minimal_file_profile_request;
    use nyanpasu_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let endpoint = TestControlEndpoint::succeeding();
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        client
            .patch_runtime_overrides(ClashGuardOverridesPatch {
                mode: Some(Mode::Global),
                ..Default::default()
            })
            .await
            .unwrap();
        let ApplicationEffect::Tray(_, view) =
            last_applied(&client.inner.effects, &port, EffectKind::Tray).await
        else {
            unreachable!("filtered by kind")
        };
        assert_eq!(view.part.mode, Mode::Global);

        // Both of the profiles owner's commit paths: one with a materialized
        // resource, one that writes the document alone.
        port.calls.lock().unwrap().clear();
        let uid = client
            .add_profile(
                minimal_file_profile_request(),
                Some("proxies: []\nmode: rule\n".into()),
            )
            .await
            .unwrap()
            .into_value();
        assert!(matches!(
            last_applied(&client.inner.effects, &port, EffectKind::Tray).await,
            ApplicationEffect::Tray(TrayRefresh::Part, _)
        ));
        port.calls.lock().unwrap().clear();
        client.reorder_profiles_by_list(vec![uid]).await.unwrap();
        assert!(matches!(
            last_applied(&client.inner.effects, &port, EffectKind::Tray).await,
            ApplicationEffect::Tray(TrayRefresh::Part, _)
        ));
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[tokio::test(start_paused = true)]
async fn automatic_retries_are_bounded_and_manual_probe_does_not_refill_budget() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        retryable: true,
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), Vec::new());
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale && p.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    for (delay, remaining) in [(1, 2), (5, 1), (30, 0)] {
        tokio::time::advance(Duration::from_secs(delay)).await;
        wait(&client, |s| {
            s.effects.iter().any(|p| {
                p.status.kind == EffectKind::Locale
                    && p.automatic_remaining == remaining
                    && p.health != ConvergenceHealth::Pending
            })
        })
        .await;
    }
    let exhausted = client.snapshot();
    assert!(
        exhausted
            .effects
            .iter()
            .filter(|p| p.status.kind == EffectKind::Locale)
            .all(|p| p.health == ConvergenceHealth::Blocked && p.attempts == 4)
    );
    let count = port.calls.lock().unwrap().len();
    tokio::time::advance(Duration::from_secs(100)).await;
    client.barrier().await;
    assert_eq!(port.calls.lock().unwrap().len(), count);
    // An unrelated save neither probes Locale nor creates a fresh budget.
    desired.app.system_proxy_bypass = "unrelated".into();
    client.application_committed(desired.app, Vec::new());
    client.barrier().await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .automatic_remaining,
        0
    );
    client.retry_now(EffectKind::Locale).unwrap();
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale
                && p.attempts == 5
                && p.health == ConvergenceHealth::Blocked
        })
    })
    .await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .automatic_remaining,
        0
    );
    shutdown.run().await;
}

#[tokio::test(start_paused = true)]
async fn stale_retry_callback_keeps_the_replacement_timer_and_shutdown_cancels_it() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        fail_kind: Some(EffectKind::Locale),
        retryable: true,
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app, Vec::new());
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    let first = client.retry_deadline().await.unwrap();
    tokio::time::advance(Duration::from_millis(500)).await;
    client.retry_now(EffectKind::Locale).unwrap();
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.attempts == 2
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    let replacement = client.retry_deadline().await.unwrap();
    assert!(replacement > first);
    let identity = client.retry_timer_id().await;
    client.stale_tick(first);
    client.barrier().await;
    assert_eq!(client.retry_deadline().await, Some(replacement));
    assert_eq!(client.retry_timer_id().await, identity);
    tokio::time::advance(replacement.saturating_duration_since(tokio::time::Instant::now())).await;
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.attempts == 3
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    assert!(client.retry_deadline().await.is_some());
    shutdown.run().await;
    let calls = port.calls.lock().unwrap().len();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(port.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn missing_binding_waits_without_spending_apply_budget() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port::default());
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.enable_system_proxy = true;
    desired.app.enable_proxy_guard = true;
    client.application_committed(desired.app, Vec::new());
    let state = wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::SystemProxy
                && p.health == ConvergenceHealth::WaitingDependency
        })
    })
    .await;
    let proxy = state
        .effects
        .iter()
        .find(|p| p.status.kind == EffectKind::SystemProxy)
        .unwrap();
    assert_eq!((proxy.attempts, proxy.automatic_remaining), (0, 3));

    client.retry_now(EffectKind::SystemProxy).unwrap();
    client.barrier().await;
    let state = wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::SystemProxy
                && p.health == ConvergenceHealth::WaitingDependency
        })
    })
    .await;
    assert_eq!(
        state
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::SystemProxy)
            .unwrap()
            .attempts,
        0
    );
    shutdown.run().await;
}

#[tokio::test]
async fn same_port_runtime_binding_reapplies_transparent_proxy_for_new_generation() {
    let port = Arc::new(Port::default());
    let (client, shutdown) = graph(port.clone()).await;
    let binding = Some(ResolvedPortBindings {
        mixed_port: 7890,
        ..ResolvedPortBindings::default()
    });
    client.publish_full(binding.clone());
    wait(&client, |snapshot| {
        snapshot.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::TransparentProxy
                && effect.health == crate::client::convergence::ConvergenceHealth::Healthy
        })
    })
    .await;
    let first_count = port
        .calls
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(_, effects)| effects)
        .filter(|effect| effect.kind() == EffectKind::TransparentProxy)
        .count();

    client.runtime_bound(binding, false);
    wait(&client, |snapshot| {
        snapshot.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::TransparentProxy
                && effect.attempts >= 2
                && effect.health == crate::client::convergence::ConvergenceHealth::Healthy
        })
    })
    .await;
    let second_count = port
        .calls
        .lock()
        .unwrap()
        .iter()
        .flat_map(|(_, effects)| effects)
        .filter(|effect| effect.kind() == EffectKind::TransparentProxy)
        .count();
    assert_eq!(second_count, first_count + 1);
    shutdown.run().await;
}

#[tokio::test]
async fn same_named_failed_target_gets_one_probe_without_budget_reset() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), vec![EffectKind::Locale]);
    wait(&client, |s| {
        s.effects
            .iter()
            .any(|p| p.status.kind == EffectKind::Locale && p.health == ConvergenceHealth::Blocked)
    })
    .await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .attempts,
        1
    );
    client.application_committed(desired.app, vec![EffectKind::Locale]);
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale
                && p.attempts == 2
                && p.health == ConvergenceHealth::Blocked
        })
    })
    .await;
    shutdown.run().await;
}

#[tokio::test]
async fn commit_receipt_and_status_keep_source_separate_from_pending_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        block: Some(EffectKind::Locale),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    let client = tokio::task::spawn_blocking(move || NyanpasuClient::try_new_with_args(args))
        .await
        .unwrap()
        .unwrap();
    let before = client.configuration_status();
    let outcome = client
        .patch_app_config(language(I18nLanguage::Korean))
        .await
        .unwrap();
    port.entered.notified().await;
    let wire = serde_json::to_value(outcome).unwrap();
    assert_eq!(wire["status"], "committed");
    assert_eq!(wire["notifications_pending"], true);
    assert_eq!(
        wire["commits"][0]["source_version"],
        before.source_versions.application + 1
    );
    // A language is no runtime input, so the save ran no operation.
    assert!(wire["commits"][0]["operation_id"].is_null());
    assert_eq!(wire["commits"][0]["runtime"], "unchanged");
    let pending = client.configuration_status();
    assert!(pending.event_seq > before.event_seq);
    assert!(pending.effects.iter().any(|e| e.kind == EffectKind::Locale
        && e.health == crate::client::convergence::ConvergenceHealth::Pending));
    port.release.notify_one();
    wait(&client.inner.effects, |state| {
        state
            .effects
            .iter()
            .map(|e| &e.status)
            .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    assert!(client.configuration_status().event_seq > pending.event_seq);
    client.request_shutdown();
    client.wait_shutdown().await;
}

/// V20, V26: once the token is cancelled, a group already running is waited
/// for rather than aborted, and nothing new starts, whatever arrives after the
/// cancel.
#[tokio::test]
async fn the_shutdown_awaits_running_groups_and_starts_no_new_one() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::Tray && s.health == EffectHealth::Healthy)
            && s.effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == EffectKind::Hotkeys && s.health == EffectHealth::Healthy)
    })
    .await;
    let started = port.calls.lock().unwrap().len();

    shutdown.request();
    // Queued before this test yields, so ahead of the drain: only the token
    // keeps them from starting a group.
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app, Vec::new());
    client.retry_now(EffectKind::Locale).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), shutdown.tasks.wait())
            .await
            .is_err(),
        "the blocked group is awaited, not aborted"
    );

    port.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), shutdown.tasks.wait())
        .await
        .expect("the owner stops once its group ended");
    assert_eq!(
        port.calls.lock().unwrap().len(),
        started,
        "no group started after the cancel"
    );
}

#[tokio::test]
async fn core_log_level_reaches_only_the_streams_owner() {
    use nyanpasu_config::clash::config::overrides::LogLevel;
    let port = Arc::new(Port::default());
    let core = crate::core::actor_v2::CoreClient::spawn(
        crate::core::actor_v2::api::tests::endpoint("http://127.0.0.1:9".into()),
    )
    .await
    .unwrap();
    let core_logs = crate::core::logs::CoreLogsClient::test_client().await;
    let streams = crate::core::clash::ws::StreamsClient::spawn(
        core.clone(),
        core_logs.clone(),
        LogLevel::Info,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    let effects = super::executor::CoreRuntimeEffects::new(port.clone(), streams, core_logs, core);
    let locale = ApplicationEffect::Locale(I18nLanguage::Russian);

    let revision = EffectRevision::new(1);
    let statuses = effects
        .apply(
            revision,
            ApplicationEffectPlan::from_effects(vec![
                locale.clone(),
                ApplicationEffect::CoreLogLevel(LogLevel::Error),
            ]),
        )
        .await;
    assert_eq!(*port.calls.lock().unwrap(), [(revision, vec![locale])]);
    assert_eq!(
        statuses
            .iter()
            .map(|status| (status.kind, status.health.clone()))
            .collect::<Vec<_>>(),
        [
            (EffectKind::Locale, EffectHealth::Healthy),
            (EffectKind::CoreLogLevel, EffectHealth::Healthy)
        ]
    );

    let statuses = effects
        .apply(
            EffectRevision::new(2),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogLevel(
                LogLevel::Silent,
            )]),
        )
        .await;
    assert_eq!(statuses.len(), 1);
    assert_eq!(port.calls.lock().unwrap().len(), 1);

    let mut settings = nyanpasu_config::application::CoreLogSettings::default();
    settings.compression = nyanpasu_config::application::CoreLogCompression::Trained;
    let statuses = effects
        .apply(
            EffectRevision::new(3),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogStorage(settings)]),
        )
        .await;
    assert_eq!(statuses[0].kind, EffectKind::CoreLogStorage);
    assert_eq!(statuses[0].health, EffectHealth::Healthy);
    assert_eq!(port.calls.lock().unwrap().len(), 1);

    settings.shard_size_mib = 0;
    let statuses = effects
        .apply(
            EffectRevision::new(4),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogStorage(settings)]),
        )
        .await;
    assert!(matches!(
        statuses[0].health,
        EffectHealth::Degraded {
            code: EffectFailureCode::CoreLogStorageFailed,
            retryable: false,
            ..
        }
    ));
}
