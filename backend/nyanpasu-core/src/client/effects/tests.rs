//! Post-commit isolation and coalescing, using explicit actor acknowledgements.
use crate::{
    client::{
        NyanpasuClient,
        tests::{TestControlEndpoint, test_client_args_with_endpoint},
    },
    effects::{
        EffectKind,
        actor::{EffectsClient, EffectsSnapshot},
        plan::{ApplicationEffect, ApplicationEffectPlan},
        ports::ApplicationEffectsPort,
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
    },
};
use nyanpasu_config::application::{I18nLanguage, NyanpasuAppConfig};
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
struct Port {
    calls: Mutex<Vec<(EffectRevision, Vec<ApplicationEffect>)>>,
    recorded: Notify,
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
        self.recorded.notify_waiters();
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

#[tokio::test(flavor = "multi_thread")]
async fn source_commit_and_second_save_do_not_wait_for_gui() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        block: Some(EffectKind::Tray),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding()).await;
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
    {
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
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn asynchronous_failure_is_status_and_never_cancels_source() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding()).await;
    args.effects = port;
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
    {
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
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_op_and_session_saves_persist_source_state() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding()).await;
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
    {
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
        assert_eq!(client.get_app_config().await.unwrap().language, current);
        assert_eq!(client.main_window_geometry().unwrap().width, 800);
        client.request_shutdown();
        client.wait_shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_source_keeps_carried_language_uncommitted() {
    use crate::{client::ClientError, state::config_error::ConfigError};
    use nyanpasu_config::application::ReleaseChannel as Channel;
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding()).await;
    args.effects = port.clone();
    args.installed_channel = Channel::Nightly;
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
    {
        let mut nightly = NyanpasuAppConfig::new_empty_patch();
        nightly.release_channel = Some(Some(Channel::Nightly));
        client.patch_app_config(nightly).await.unwrap();

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

        assert_ne!(
            client.get_app_config().await.unwrap().language,
            I18nLanguage::Korean
        );
        client.request_shutdown();
        client.wait_shutdown().await;
    }
}

#[tokio::test]
async fn commit_receipt_and_status_keep_source_separate_from_pending_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port {
        block: Some(EffectKind::Locale),
        ..Default::default()
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding()).await;
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
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
        && e.health == crate::effects::convergence::ConvergenceHealth::Pending));
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
