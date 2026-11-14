//! Pending external actions settle before another runtime write is submitted.
use super::{
    ScriptedWaitEndpoint, WaitScript,
    mutations::{commit_source, fixture, overrides, scripted_fixture, settled},
    workflow_graph_with_clients,
};
use crate::{
    actor_v2::{CoreClient, service_actor::ServiceClient},
    client::{
        application_workflow::{ApplicationWorkflowClient, Message},
        test_support::TestControlEndpoint,
    },
};
use nyanpasu_core_manager::OperationId;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use struct_patch::Patch as _;
fn unknown(client: &ApplicationWorkflowClient) -> bool {
    client.reconciler_status().conclusion.label() == "Unknown"
}
async fn tick(client: &ApplicationWorkflowClient) {
    client.0.actor.cast(Message::Tick).unwrap();
    super::barrier(client).await;
}
fn pending_submission(endpoint: &ScriptedWaitEndpoint) -> OperationId {
    *endpoint.operations().last().unwrap()
}
struct ParkedInstall {
    delegate: crate::actor_v2::test_support::HostTransitionServiceAdapter,
    parked: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl nyanpasu_contracts::service::ServiceProcessControl for ParkedInstall {
    async fn probe(&self) -> Result<String, nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.parked.notify_one();
        self.release.notified().await;
        self.delegate.install().await
    }
    async fn uninstall(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.start_daemon().await
    }
    async fn stop_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.stop_daemon().await
    }
    async fn update(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.update().await
    }
}

impl crate::actor_v2::service_actor::ServiceEndpointFactory for ParkedInstall {
    fn endpoint(&self) -> crate::actor_v2::endpoint::EndpointHandle {
        self.delegate.endpoint()
    }
}

async fn a_lost_handoff() -> super::startup::Graph {
    use super::startup::{DaemonState, Setup, graph};
    let g = graph(Setup {
        daemon: DaemonState::Running,
        residual: true,
        owner_on_service: true,
        handoff_budget: Some(std::time::Duration::from_millis(300)),
        ..Setup::default()
    })
    .await;
    g.service_host.hold_stop.store(true, Ordering::SeqCst);

    let report = g.start().await;

    assert!(
        matches!(
            report.outcome,
            crate::client::application_workflow::startup::StartupOutcome::RecoveryRequired { .. }
        ),
        "{report:?}"
    );
    assert!(unknown(&g.client));
    let error = g.client.retry_runtime().await.unwrap_err();
    assert!(matches!(
        error,
        nyanpasu_runtime_values::runtime_error::RuntimeError::RecoveryUnresolved { .. }
    ));
    assert!(
        error.to_string().contains("handoff to Local"),
        "the router is still handing off: {}",
        error
    );
    assert!(unknown(&g.client));
    g
}

// Guards continuing the latest committed target after the facade consumes a settled action.
#[tokio::test(start_paused = true)]
async fn a_tick_applies_the_latest_target_after_an_unknown_action_settles() {
    let f = fixture().await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.reconcile().await.is_err());
    assert!(unknown(&f.client));
    tick(&f.client).await;
    assert_eq!(f.endpoint.reconciled_bytes().len(), 1);
    f.endpoint.set_result_missing(false);
    tick(&f.client).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Converged");
    assert_eq!(f.endpoint.reconciled_bytes().len(), 2);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Converged"
    );
}

// Guards counting only real submissions, rather than pending-action observations, toward the unknown limit.
#[tokio::test(start_paused = true)]
async fn a_second_unknown_submission_blocks_after_its_action_settles() {
    let f = scripted_fixture().await;
    let endpoint = f.scripted.as_ref().unwrap();
    endpoint.queue(WaitScript::Missing);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Unknown"
    );
    let first = pending_submission(endpoint);
    tick(&f.client).await;
    assert_eq!(f.endpoint.reconciled_bytes().len(), 1);
    endpoint.rescript(first, WaitScript::Deliver);
    // The first submission changed the unreceipted instance, so it is retired before another apply.
    endpoint.queue(WaitScript::Deliver);
    endpoint.queue(WaitScript::Missing);
    tick(&f.client).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Unknown");
    assert_eq!(f.endpoint.reconciled_bytes().len(), 2);
    let second = pending_submission(endpoint);
    endpoint.rescript(second, WaitScript::Deliver);
    tick(&f.client).await;
    assert_eq!(
        f.client.reconciler_status().conclusion.label(),
        "NotInEffect"
    );
    tokio::time::advance(Duration::from_secs(60)).await;
    tick(&f.client).await;
    assert_eq!(f.endpoint.reconciled_bytes().len(), 2);
}

// Guards restoring confirmed ports when an unknown forced apply left the original revision running.
#[tokio::test(start_paused = true)]
async fn an_unknown_forced_apply_reconfirms_the_unchanged_running_instance() {
    let f = scripted_fixture().await;
    f.client.reconcile().await.unwrap();
    let ports = f.ports.confirmed().unwrap();
    let endpoint = f.scripted.as_ref().unwrap();
    f.endpoint.set_failure(Some("apply_failed"));
    endpoint.queue(WaitScript::Missing);
    assert!(f.client.reconcile().await.is_err());
    assert!(f.ports.confirmed().is_none());
    endpoint.rescript(pending_submission(endpoint), WaitScript::Deliver);
    f.endpoint.set_failure(None);
    let submitted = f.endpoint.submissions();
    tick(&f.client).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Converged");
    assert_eq!(f.ports.confirmed(), Some(ports));
    assert_eq!(f.endpoint.submissions(), submitted);
}

#[tokio::test]
async fn a_service_command_still_running_defers_runtime_work() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Arc::new(ParkedInstall {
        delegate: crate::actor_v2::test_support::HostTransitionServiceAdapter {
            endpoint: TestControlEndpoint::succeeding_on(
                nyanpasu_runtime_values::execution_host::ExecutionHost::Service,
            ),
            calls: Arc::default(),
            stopped: false.into(),
            installed_for: Default::default(),
        },
        parked: Default::default(),
        release: Default::default(),
    });
    let local = TestControlEndpoint::succeeding();
    local.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
        None,
    );
    let core = CoreClient::spawn(local.clone()).await.unwrap();
    let service =
        ServiceClient::spawn_bounded(daemon.clone(), daemon.clone(), 0, Duration::from_millis(50))
            .await
            .unwrap();
    let (client, builder, _, clash) = workflow_graph_with_clients(
        &dir,
        core,
        service.clone(),
        false,
        Arc::default(),
        Default::default(),
    )
    .await;
    builder.release.notify_one();
    assert!(client.install_service().await.is_err());
    daemon.parked.notified().await;
    assert!(unknown(&client));
    let mut config = clash.snapshot().state;
    config
        .overrides
        .apply(serde_json::from_value(serde_json::json!({"mode":"global"})).unwrap());
    clash.replace(config).await.unwrap();
    let version = clash.snapshot().version;
    client.source_committed(
        nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
        version,
    );
    assert_eq!(
        client
            .settled(
                nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
                version
            )
            .await
            .conclusion
            .label(),
        "Unknown"
    );
    assert!(local.reconciled_bytes().is_empty());
    daemon.release.notify_one();
    while !service.command_settled().await.unwrap() {
        tokio::task::yield_now().await;
    }
    tick(&client).await;
    assert_eq!(client.reconciler_status().conclusion.label(), "Converged");
    assert_eq!(local.reconciled_bytes().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_handoff_whose_answer_was_lost_is_recovered_once_the_router_settles() {
    let g = a_lost_handoff().await;
    g.service_host.release.notify_one();
    let mut status = g.core.subscribe();
    status
        .wait_for(|s| {
            matches!(
                s.connectivity,
                nyanpasu_runtime_values::core_status::EndpointConnectivity::Connected
            ) && s.host == nyanpasu_runtime_values::execution_host::ExecutionHost::Local
        })
        .await
        .unwrap();
    tick(&g.client).await;
    assert_eq!(g.client.reconciler_status().conclusion.label(), "Converged");
    assert_eq!(g.log(), ["service:stop", "local:reconcile"]);
}

#[tokio::test(start_paused = true)]
async fn a_lost_handoff_that_failed_into_a_degraded_router_is_resolved() {
    use nyanpasu_runtime_values::{
        core_status::EndpointConnectivity, execution_host::ExecutionHost,
    };
    let g = a_lost_handoff().await;
    let mut status = g.core.subscribe();
    g.core
        .report_endpoint_down("service endpoint stopped answering");
    g.service_host.delegate.set_failure(Some("apply_failed"));
    g.service_host.release.notify_one();
    status
        .wait_for(|s| {
            matches!(
                s.connectivity,
                EndpointConnectivity::Degraded {
                    desired: ExecutionHost::Service,
                    ..
                }
            )
        })
        .await
        .unwrap();
    tick(&g.client).await;
    assert_eq!(g.client.reconciler_status().conclusion.label(), "Waiting");
    assert_eq!(g.log(), ["service:stop"]);
}

#[tokio::test(start_paused = true)]
async fn a_lost_stop_is_recovered_by_a_confirmed_stop_and_nothing_else() {
    let f = fixture().await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.stop_core().await.is_err());
    assert!(unknown(&f.client));
    f.endpoint.set_result_missing(false);
    f.endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
        None,
    );
    f.endpoint.set_failure(Some("apply_failed"));
    tick(&f.client).await;
    assert_eq!(
        f.client.reconciler_status().conclusion.label(),
        "NotInEffect"
    );
    f.endpoint.set_failure(None);
    f.client.retry_runtime().await.unwrap();
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Inactive");
    assert!(f.endpoint.reconciled_bytes().is_empty());
}

// Guards publishing Waiting and the sampled versions while status remains unreadable.
#[tokio::test(start_paused = true)]
async fn an_unreadable_core_keeps_waiting_without_submitting() {
    let f = fixture().await;
    f.endpoint.set_status_fails(true);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Waiting"
    );
    assert_eq!(f.endpoint.submissions(), 0);
    let sequence = f.client.reconciler_status().seq;
    tokio::time::advance(Duration::from_secs(5)).await;
    tick(&f.client).await;
    let status = f.client.reconciler_status();
    assert_eq!(status.conclusion.label(), "Waiting");
    assert!(status.seq > sequence);
    assert_eq!(status.seen.clash, f.clash.snapshot().version);
    assert_eq!(f.endpoint.submissions(), 0);
}

// Guards retiring a running instance when its effective revision no longer matches the receipt.
#[tokio::test]
async fn a_different_effective_revision_is_stopped_before_the_target_is_applied() {
    let f = fixture().await;
    f.endpoint.set_effective_hash("another-instance");
    let version = f.clash.snapshot().version;
    f.client.source_committed(
        nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
        version,
    );
    super::barrier(&f.client).await;
    let status = f
        .client
        .settled(
            nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
            version,
        )
        .await;
    assert_eq!(status.conclusion.label(), "Converged");
    assert_eq!(f.endpoint.submissions(), 2);
    assert_eq!(f.endpoint.reconciled_bytes().len(), 1);
}
