//! Post-commit isolation and coalescing, using explicit actor acknowledgements.
use super::{
    actor::{EffectsArgs, EffectsClient, EffectsSnapshot},
    plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    NyanpasuClient, UiEventSink,
    tests::{TestControlEndpoint, test_client_args_with_endpoint},
};
use nyanpasu_config::{
    application::{I18nLanguage, NyanpasuAppConfig},
    clash::config::ClashConfig,
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

#[derive(Default)]
struct Ui;
impl UiEventSink for Ui {
    fn state_changed(&self, _: crate::core::handle::StateChanged) {}
    fn notice_message(&self, _: &crate::core::handle::Message) {}
    fn update_systray(&self) -> crate::client::Result<()> {
        Ok(())
    }
    fn update_systray_part(&self) -> crate::client::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Port {
    calls: Mutex<Vec<(EffectRevision, Vec<ApplicationEffect>)>>,
    block: Option<EffectKind>,
    entered: Notify,
    release: Notify,
    blocked: AtomicBool,
    fail: AtomicBool,
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
                    EffectHealth::Degraded { code: "system_proxy_port_unresolved", message: "no binding".into(), retryable: true }
                } else if self.fail.load(Ordering::SeqCst) {
                    EffectHealth::Degraded {
                        code: "injected_failure",
                        message: "failed".into(),
                        retryable: self.retryable,
                    }
                } else {
                    EffectHealth::Healthy
                },
            })
            .collect()
    }
    async fn shutdown(&self) -> Vec<EffectStatus> {
        Vec::new()
    }
}
fn inputs() -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(&NyanpasuAppConfig::default(), &ClashConfig::default(), None)
}
async fn graph(port: Arc<Port>) -> EffectsClient {
    EffectsClient::spawn(EffectsArgs {
        port,
        ui: Arc::new(Ui),
        initial: inputs(),
    })
    .await
    .unwrap()
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
    let client = graph(port.clone()).await;
    client.reconcile(inputs());
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
    client.shutdown().await;
}

#[tokio::test]
async fn blocked_gui_does_not_block_proxy_group() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Tray),
        ..Default::default()
    });
    let client = graph(port.clone()).await;
    client.reconcile(inputs());
    port.entered.notified().await;
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::SystemProxy && s.health == EffectHealth::Healthy)
    })
    .await;
    port.release.notify_one();
    client.shutdown().await;
}

#[tokio::test]
async fn queued_visual_targets_coalesce_and_full_subsumes_part() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Locale),
        ..Default::default()
    });
    let client = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.committed(desired.clone(), false, Vec::new());
    port.entered.notified().await;
    desired.app.language = I18nLanguage::English;
    client.committed(desired.clone(), false, Vec::new());
    desired.app.enable_tray_text = !desired.app.enable_tray_text;
    client.committed(desired.clone(), false, Vec::new());
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
            ApplicationEffect::Tray(TrayRefresh::Full)
        ]
    );
    drop(calls);
    client.shutdown().await;
}

#[tokio::test]
async fn stale_completion_does_not_publish_success_for_new_desired() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let client = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.system_proxy_bypass = "old".into();
    client.committed(desired.clone(), false, Vec::new());
    port.entered.notified().await;
    desired.app.system_proxy_bypass = "new".into();
    client.committed(desired, false, Vec::new());
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
    client.shutdown().await;
}

#[tokio::test]
async fn independent_graphs_and_shutdown_admission() {
    let a = Arc::new(Port::default());
    let b = Arc::new(Port::default());
    let left = graph(a.clone()).await;
    let right = graph(b.clone()).await;
    left.shutdown().await;
    left.reconcile(inputs());
    left.barrier().await;
    assert!(a.calls.lock().unwrap().is_empty());
    right.reconcile(inputs());
    wait(&right, |s| {
        s.effects.len() == 8
            && s.effects
                .iter()
                .map(|e| &e.status)
                .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    assert_eq!(b.calls.lock().unwrap().len(), 3);
    right.shutdown().await;
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
        client.shutdown_application_effects().await;
        client.shutdown_core().await;
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
        client.shutdown_application_effects().await;
        client.shutdown_core().await;
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
            .patch_session_state(nyanpasu_config::state::PersistentState::new_empty_patch())
            .await
            .unwrap();
        client.inner.effects.barrier().await;
        assert!(port.calls.lock().unwrap().is_empty());
        client.shutdown_application_effects().await;
        client.shutdown_core().await;
    });
}

struct RejectMirror(AtomicBool);
impl crate::state::mirror::VergeLegacyBridge for RejectMirror {
    fn prepare(
        &self,
        _: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
        anyhow::ensure!(
            !self.0.load(Ordering::SeqCst),
            "injected application mirror"
        );
        Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
    }
    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        Ok(Default::default())
    }
}

#[test]
fn rejected_source_never_dispatches() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    let mirror = Arc::new(RejectMirror(AtomicBool::new(false)));
    args.bridges.verge = mirror.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    mirror.0.store(true, Ordering::SeqCst);
    tauri::async_runtime::block_on(async {
        assert!(
            client
                .patch_app_config(language(I18nLanguage::Korean))
                .await
                .is_err()
        );
        client.inner.effects.barrier().await;
        assert!(port.calls.lock().unwrap().is_empty());
        assert_ne!(
            client.get_app_config().await.unwrap().language,
            I18nLanguage::Korean
        );
        client.shutdown_application_effects().await;
        client.shutdown_core().await;
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
    let client = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.committed(desired.clone(), false, Vec::new());
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
    client.committed(desired, false, Vec::new());
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
    client.shutdown().await;
}

#[tokio::test]
async fn missing_binding_waits_without_spending_apply_budget() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port::default());
    let client = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.enable_system_proxy = true;
    desired.app.enable_proxy_guard = true;
    client.committed(desired, false, Vec::new());
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
    client.shutdown().await;
}

#[tokio::test]
async fn same_named_failed_target_gets_one_probe_without_budget_reset() {
    use crate::client::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        ..Default::default()
    });
    let client = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.committed(desired.clone(), false, vec![EffectKind::Locale]);
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
    client.committed(desired, false, vec![EffectKind::Locale]);
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale
                && p.attempts == 2
                && p.health == ConvergenceHealth::Blocked
        })
    })
    .await;
    client.shutdown().await;
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
    let pending = client.configuration_status();
    assert!(pending.event_seq > before.event_seq);
    assert_eq!(
        wire["commits"][0]["operation_id"],
        pending.recent_operations[0].operation_id
    );
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
    client.shutdown_application_effects().await;
}
