//! Post-commit isolation and coalescing, using explicit actor acknowledgements.
use crate::client::{
    NyanpasuClient,
    tests::{TestControlEndpoint, test_client_args_with_endpoint},
};
use nyanpasu_config::application::{I18nLanguage, NyanpasuAppConfig};
use nyanpasu_core::effects::{
    EffectKind,
    actor::{EffectsClient, EffectsSnapshot},
    plan::{ApplicationEffect, ApplicationEffectPlan, TrayRefresh},
    ports::ApplicationEffectsPort,
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
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

/// Waits for a newer recorded tray request, then its exact healthy completion.
async fn last_applied(
    client: &EffectsClient,
    port: &Port,
    after: EffectRevision,
    expected: impl Fn(&ApplicationEffect) -> bool,
) -> (EffectRevision, ApplicationEffect) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (revision, effect) = loop {
            let recorded = port.recorded.notified();
            tokio::pin!(recorded);
            recorded.as_mut().enable();
            let request = port
                .calls
                .lock()
                .unwrap()
                .iter()
                .rev()
                .filter(|(revision, _)| *revision > after)
                .find_map(|(revision, effects)| {
                    effects
                        .iter()
                        .find(|effect| effect.kind() == EffectKind::Tray && expected(effect))
                        .map(|effect| (*revision, effect.clone()))
                });
            if let Some(request) = request {
                break request;
            }
            recorded.await;
        };
        client
            .subscribe()
            .wait_for(|snapshot| {
                snapshot.effects.iter().any(|progress| {
                    let status = &progress.status;
                    status.kind == EffectKind::Tray
                        && status.desired_revision == revision
                        && status.applied_revision == revision
                        && status.health == EffectHealth::Healthy
                })
            })
            .await
            .unwrap();
        (revision, effect)
    })
    .await
    .expect("the recorded tray revision must complete")
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
    let client = NyanpasuClient::try_new_with_args(args).unwrap().client;
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
    let client = NyanpasuClient::try_new_with_args(args).unwrap().client;
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
fn no_op_and_session_saves_persist_source_state() {
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap().client;
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
        assert_eq!(client.get_app_config().await.unwrap().language, current);
        assert_eq!(client.main_window_geometry().unwrap().width, 800);
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[test]
fn rejected_source_keeps_carried_language_uncommitted() {
    use crate::{client::ClientError, state::config_error::ConfigError};
    use nyanpasu_config::application::ReleaseChannel as Channel;
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.effects = port.clone();
    args.installed_channel = Channel::Nightly;
    let client = NyanpasuClient::try_new_with_args(args).unwrap().client;
    tauri::async_runtime::block_on(async {
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
    });
}

/// The clash config and profiles owners hand the effects owner their own
/// slices: a mode change reaches the tray from the clash config owner, and a
/// profiles commit asks the tray for a partial refresh.
#[test]
fn clash_and_profiles_owners_hand_their_own_slices_to_the_tray() {
    use crate::client::tests::minimal_file_profile_request;
    use nyanpasu_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
    use nyanpasu_core::effects::plan::{ApplicationEffectFields, ClashEffectFields};
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let endpoint = TestControlEndpoint::succeeding();
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap().client;
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        client
            .patch_runtime_overrides(ClashGuardOverridesPatch {
                mode: Some(Mode::Global),
                ..Default::default()
            })
            .await
            .unwrap();
        let expected_application =
            ApplicationEffectFields::from(&client.inner.application.snapshot().state);
        let expected_clash = ClashEffectFields::from(&client.inner.clash_config.snapshot().state);
        let (revision, effect) = last_applied(
            &client.inner.effects,
            &port,
            EffectRevision::default(),
            |effect| {
                matches!(effect, ApplicationEffect::Tray(_, application, clash)
                    if application == &expected_application && clash == &expected_clash)
            },
        )
        .await;
        let ApplicationEffect::Tray(_, application, clash) = effect else {
            unreachable!("filtered by kind")
        };
        assert_eq!(
            super::presentation::tray_view(&application, &clash)
                .part
                .mode,
            Mode::Global
        );

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
        let expected_application =
            ApplicationEffectFields::from(&client.inner.application.snapshot().state);
        let expected_clash = ClashEffectFields::from(&client.inner.clash_config.snapshot().state);
        let (revision, effect) = last_applied(&client.inner.effects, &port, revision, |effect| {
            matches!(effect, ApplicationEffect::Tray(TrayRefresh::Part, application, clash)
                if application == &expected_application && clash == &expected_clash)
        })
        .await;
        assert!(matches!(
            effect,
            ApplicationEffect::Tray(TrayRefresh::Part, _, _)
        ));
        port.calls.lock().unwrap().clear();
        client.reorder_profiles_by_list(vec![uid]).await.unwrap();
        let expected_application =
            ApplicationEffectFields::from(&client.inner.application.snapshot().state);
        let expected_clash = ClashEffectFields::from(&client.inner.clash_config.snapshot().state);
        let (_, effect) = last_applied(&client.inner.effects, &port, revision, |effect| {
            matches!(effect, ApplicationEffect::Tray(TrayRefresh::Part, application, clash)
                if application == &expected_application && clash == &expected_clash)
        })
        .await;
        assert!(matches!(
            effect,
            ApplicationEffect::Tray(TrayRefresh::Part, _, _)
        ));
        client.request_shutdown();
        client.wait_shutdown().await;
    });
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
        && e.health == nyanpasu_core::effects::convergence::ConvergenceHealth::Pending));
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
