//! Desktop tray projection and concrete invalidation mapping.
use crate::desktop::{
    StateChanged, UiEventSink,
    test_support::{
        TestControlEndpoint, minimal_file_profile_request, test_client_args_with_endpoint,
    },
};
use nyanpasu_core::{
    NyanpasuClient,
    effects::{
        EffectKind,
        actor::EffectsSnapshot,
        plan::{ApplicationEffect, ApplicationEffectPlan, TrayRefresh},
        ports::{ApplicationEffectsPort, EffectInvalidationSink},
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
    },
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
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

/// Waits for a newer recorded tray request, then its exact healthy completion.
async fn last_applied(
    client: &tokio::sync::watch::Receiver<EffectsSnapshot>,
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
            .clone()
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

/// The clash config and profiles owners hand the effects owner their own
/// slices: a mode change reaches the tray from the clash config owner, and a
/// profiles commit asks the tray for a partial refresh.
#[tokio::test(flavor = "multi_thread")]
async fn clash_and_profiles_owners_hand_their_own_slices_to_the_tray() {
    use nyanpasu_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
    use nyanpasu_core::effects::plan::{ApplicationEffectFields, ClashEffectFields};
    let dir = tempfile::tempdir().unwrap();
    let port = Arc::new(Port::default());
    let endpoint = TestControlEndpoint::succeeding();
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone()).await;
    args.effects = port.clone();
    let client = NyanpasuClient::try_new_with_args(args)
        .await
        .unwrap()
        .client;
    let (_, effects, _) = client.subscribe_configuration_changes();
    endpoint.prime(&client).await;
    client
        .patch_runtime_overrides(ClashGuardOverridesPatch {
            mode: Some(Mode::Global),
            ..Default::default()
        })
        .await
        .unwrap();
    let expected_application = ApplicationEffectFields::from(&client.app_config_snapshot());
    let expected_clash = ClashEffectFields::from(&client.get_clash_config().await.unwrap());
    let (revision, effect) = last_applied(&effects, &port, EffectRevision::default(), |effect| {
        matches!(effect, ApplicationEffect::Tray(_, application, clash)
                if application == &expected_application && clash == &expected_clash)
    })
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
    let expected_application = ApplicationEffectFields::from(&client.app_config_snapshot());
    let expected_clash = ClashEffectFields::from(&client.get_clash_config().await.unwrap());
    let (revision, effect) = last_applied(&effects, &port, revision, |effect| {
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
    let expected_application = ApplicationEffectFields::from(&client.app_config_snapshot());
    let expected_clash = ClashEffectFields::from(&client.get_clash_config().await.unwrap());
    let (_, effect) = last_applied(&effects, &port, revision, |effect| {
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
}

/// Counts the ClashConfig notifications the client sends the UI.
struct CountingUi {
    refreshed: tokio::sync::watch::Sender<usize>,
}
impl UiEventSink for CountingUi {
    fn state_changed(&self, state: StateChanged) {
        if matches!(state, StateChanged::ClashConfig) {
            self.refreshed.send_modify(|count| *count += 1);
        }
    }
}

#[test]
fn before_visual_apply_refreshes_the_clash_config_view() {
    let ui = Arc::new(CountingUi {
        refreshed: tokio::sync::watch::Sender::new(0),
    });
    let sink = super::presentation::TauriEffectInvalidationSink::new(ui.clone());
    assert_eq!(*ui.refreshed.borrow(), 0);
    sink.before_visual_apply();
    assert_eq!(*ui.refreshed.borrow(), 1);
}
