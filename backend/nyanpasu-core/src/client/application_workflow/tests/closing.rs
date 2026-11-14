//! The workflow's side of the shutdown: once its token is cancelled it admits
//! nothing new, lets the running operation reach its own end, and only then
//! stops the core and itself (X2–X4, V16).

use std::{sync::Arc, time::Duration};

use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::mutations::fixture;

/// Long enough that only a stuck operation reaches it.
const SETTLE: Duration = Duration::from_secs(5);

/// Cancels the workflow's token, as the root shutdown does.
fn request_shutdown(shutdown: &CancellationToken, tasks: &TaskTracker) {
    shutdown.cancel();
    tasks.close();
}

/// Whether the workflow has stopped within `wait`.
async fn stopped_within(tasks: &TaskTracker, wait: Duration) -> bool {
    tokio::time::timeout(wait, tasks.wait()).await.is_ok()
}

#[tokio::test]
async fn the_shutdown_refuses_new_work_and_stops_the_core_once() {
    let f = fixture().await;
    let submitted = f.endpoint.submissions();

    request_shutdown(&f.shutdown, &f.tasks);

    assert!(f.client.reconcile().await.is_err(), "closed to new work");
    assert!(stopped_within(&f.tasks, SETTLE).await);
    assert_eq!(f.endpoint.submissions(), submitted + 1, "one stop");
}

/// Once every client is gone, the actor is drained and stops the core in
/// `post_stop`, with no token cancelled.
#[tokio::test]
async fn an_abandoned_workflow_still_stops_the_core() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = crate::client::test_support::TestControlEndpoint::succeeding();
    let submitted = endpoint.submissions();
    let core = crate::actor_v2::CoreClient::spawn(endpoint.clone())
        .await
        .unwrap();
    let daemon = Arc::new(crate::actor_v2::test_support::IdleServiceAdapter);
    let service = crate::actor_v2::service_actor::ServiceClient::spawn(daemon.clone(), daemon, 0)
        .await
        .unwrap();
    let (client, _, _, _) = super::workflow_graph_with_clients(
        &dir,
        core,
        service,
        false,
        Arc::new(crate::client::SessionPortResolver::default()),
        CancellationToken::new(),
    )
    .await;
    let actor = client.0.actor.get_cell();

    drop(client);

    actor
        .wait(Some(SETTLE))
        .await
        .expect("the abandoned workflow stops after its shutdown");
    assert_eq!(endpoint.submissions(), submitted + 1, "one stop");
}

// Guards the handler completing a submitted application before post_stop stops the core.
#[tokio::test]
async fn shutdown_waits_for_an_application_already_submitted() {
    use futures_util::FutureExt as _;
    use nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch;

    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, scripted) =
        super::commit_first::client_with_scripted_endpoint(&dir).await;
    scripted.queue(super::WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_runtime_overrides(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..Default::default()
                })
                .await
        }
    });
    scripted.held().await;
    let submitted = endpoint.submissions();
    client.request_shutdown();
    assert!(client.wait_shutdown().now_or_never().is_none());
    assert_eq!(
        endpoint.submissions(),
        submitted,
        "shutdown awaits the application"
    );
    scripted.release();
    applying.await.unwrap().unwrap();
    client.wait_shutdown().await;
    assert_eq!(endpoint.reconciled_bytes().len(), 1);
    assert_eq!(
        endpoint.submissions(),
        submitted + 1,
        "the core stops after applying"
    );
}

struct HeldCheck {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    rejection: Option<Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>>,
}

#[async_trait::async_trait]
impl crate::state::mutation::ports::RuntimeGate for HeldCheck {
    async fn check(
        &self,
        _: crate::state::mutation::ports::RuntimeCandidate,
    ) -> Result<(), Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>> {
        self.entered.notify_one();
        self.release.notified().await;
        self.rejection.clone().map_or(Ok(()), Err)
    }

    fn committed(
        &self,
        _: nyanpasu_runtime_values::mutation::ConfigDomain,
        _: nyanpasu_state_engine::Version,
    ) {
    }
}

// Guards owner-started checking and persistence finishing even when shutdown begins during checking.
#[tokio::test]
async fn shutdown_finishes_an_admitted_source_and_refuses_a_queued_source() {
    use crate::state::config_error::ConfigError;
    use futures_util::FutureExt as _;
    use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};

    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(HeldCheck {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        rejection: None,
    });
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let manager = super::mutations::application_manager(
        super::mutations::temp_path(&dir, "application.yaml"),
        NyanpasuAppConfig::default(),
    )
    .await;
    let application =
        super::mutations::application_owner(manager, gate.clone(), &shutdown, &tasks).await;
    let enabled = !application.snapshot().state.enable_builtin_enhanced;
    let saving = tokio::spawn({
        let application = application.clone();
        async move {
            application
                .patch(NyanpasuAppConfigPatch {
                    enable_builtin_enhanced: Some(enabled),
                    ..Default::default()
                })
                .await
        }
    });
    gate.entered.notified().await;
    let mut queued = Box::pin(application.patch(NyanpasuAppConfigPatch {
        enable_silent_start: Some(!application.snapshot().state.enable_silent_start),
        ..Default::default()
    }));
    assert!(queued.as_mut().now_or_never().is_none());
    request_shutdown(&shutdown, &tasks);
    assert!(tasks.wait().now_or_never().is_none());
    gate.release.notify_one();
    let saved = saving.await.unwrap().unwrap();
    assert_eq!(
        application.snapshot().state.enable_builtin_enhanced,
        enabled
    );
    assert!(matches!(
        queued.await,
        Err(ConfigError::ShuttingDown { .. } | ConfigError::OwnerStopped { .. })
    ));
    tasks.wait().await;
    assert_eq!(
        application.snapshot().version,
        saved.version,
        "queued work never commits"
    );
}

// Guards refusal before persistence: shutdown never creates a runtime compensation to wait for.
#[tokio::test]
async fn shutdown_keeps_a_refused_source_unchanged() {
    use crate::state::config_error::ConfigError;
    use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
    use nyanpasu_runtime_values::{
        mutation::{CommitAborted, RuntimeAftermath},
        runtime_error::RuntimeError,
    };

    let dir = tempfile::tempdir().unwrap();
    let path = super::mutations::temp_path(&dir, "application.yaml");
    let manager =
        super::mutations::application_manager(path.clone(), NyanpasuAppConfig::default()).await;
    let bytes = std::fs::read(&path).ok();
    let gate = Arc::new(HeldCheck {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        rejection: Some(Arc::new(RuntimeError::ShuttingDown)),
    });
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let application =
        super::mutations::application_owner(manager, gate.clone(), &shutdown, &tasks).await;
    let before = application.snapshot();
    let saving = tokio::spawn({
        let application = application.clone();
        async move {
            application
                .patch(NyanpasuAppConfigPatch {
                    enable_builtin_enhanced: Some(!before.state.enable_builtin_enhanced),
                    ..Default::default()
                })
                .await
        }
    });
    gate.entered.notified().await;
    request_shutdown(&shutdown, &tasks);
    gate.release.notify_one();
    assert!(matches!(
        saving.await.unwrap(),
        Err(ConfigError::Commit {
            source: CommitAborted::RuntimeRefused {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
            ..
        })
    ));
    tasks.wait().await;
    assert_eq!(application.snapshot().version, before.version);
    assert_eq!(std::fs::read(path).ok(), bytes);
}

// Guards SourceCommitted publishing Closed for a version already committed behind another application.
#[tokio::test]
async fn shutdown_reports_closed_for_a_committed_change_waiting_to_apply() {
    use nyanpasu_config::{
        application::NyanpasuAppConfigPatch, clash::config::overrides::ClashGuardOverridesPatch,
    };
    use nyanpasu_runtime_values::runtime::RuntimeCommitStatus;

    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, scripted) =
        super::commit_first::client_with_scripted_endpoint(&dir).await;
    scripted.queue(super::WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_runtime_overrides(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..Default::default()
                })
                .await
        }
    });
    scripted.held().await;
    let enabled = !client
        .inner
        .application
        .snapshot()
        .state
        .enable_builtin_enhanced;
    let mut changes = client.inner.application.subscribe_settings_changes();
    changes.borrow_and_update();
    let saving = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_app_config(NyanpasuAppConfigPatch {
                    enable_builtin_enhanced: Some(enabled),
                    ..Default::default()
                })
                .await
        }
    });
    changes.changed().await.unwrap();
    let committed = client.inner.application.snapshot();
    client.request_shutdown();
    scripted.release();
    applying.await.unwrap().unwrap();
    let outcome = saving.await.unwrap().unwrap();
    assert_eq!(
        super::commit_first::receipt(&outcome).runtime,
        RuntimeCommitStatus::RecoveryRequired
    );
    assert!(matches!(&outcome.degradations()[0].reason,
        nyanpasu_runtime_values::runtime::DegradationReason::RuntimeRecoveryRequired { cause: Some(cause), .. }
        if matches!(cause.as_ref(), nyanpasu_runtime_values::runtime_error::RuntimeError::ShuttingDown)));
    client.wait_shutdown().await;
    assert_eq!(
        client.inner.application.snapshot().version,
        committed.version
    );
    assert_eq!(
        client
            .get_app_config()
            .await
            .unwrap()
            .enable_builtin_enhanced,
        enabled
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        1,
        "the queued target was not applied"
    );
}
