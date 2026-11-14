//! T4: a restore is verified against the receipt, not against the name the
//! lower control plane gave its own transaction result (v2 C4, V04).

use std::sync::Arc;

use super::workflow_graph_with_clients;
use crate::{
    actor_v2::{CoreClient, service_actor::ServiceClient, test_support::IdleServiceAdapter},
    client::{
        runtime,
        runtime_recovery::{
            ObservedRuntime, RecoveryMismatch, RecoveryVerification, verify_recovery_target,
        },
        test_support::TestControlEndpoint,
    },
};

/// The audited scenario: the app was running A, applied B, then asked to be
/// put back on A. The runtime answers `RolledBack`, which only says that *its*
/// restore request failed and it returned to its own previous state -- still B.
/// Reading that as "recovered to A" is the bug; the receipt for A is what
/// settles it.
#[tokio::test]
async fn a_rolled_back_restore_that_left_the_core_on_b_is_not_a_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = runtime::RuntimeSnapshotStore::default();
    let endpoint = TestControlEndpoint::succeeding();
    // The host publishes a running core of the kind the app builds for, so the
    // only thing left for the verification to decide is the document identity.
    endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 4, pid: 91 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    endpoint.set_source_hash("source-a");
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let service = {
        let process_control = Arc::new(IdleServiceAdapter);
        ServiceClient::spawn(process_control.clone(), process_control, 0)
    }
    .await
    .unwrap();
    let (client, builder, _, _) = workflow_graph_with_clients(
        &dir,
        core.clone(),
        service,
        false,
        Arc::new(crate::client::SessionPortResolver::new(store.clone())),
        tokio_util::sync::CancellationToken::new(),
    )
    .await;
    builder.release.notify_one();

    client.reconcile().await.unwrap();
    let baseline = store
        .last_confirmed_runtime_receipt()
        .expect("the core confirmed the apply we will later restore");
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    // The apply that produced this receipt is the submission that confirmed the
    // settings it carries, which is the third input the verification needs: no
    // host publishes local-IPC settings back, so only a confirmation of the
    // submission that carried them can settle that half of the target.
    assert_eq!(
        verify_recovery_target(&baseline, &observed, Some(&baseline.binding)),
        RecoveryVerification::Verified,
        "the receipt describes what is actually running right now"
    );

    // A restart of the very same configuration lands in a new epoch, and the
    // runtime stamps that epoch into the managed controller endpoint of the
    // effective document it builds. The restored core therefore reports a
    // different effective hash for the document the receipt describes, and the
    // receipt is still what is running.
    endpoint.set_effective_hash("effective-a-epoch-2");
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    assert_eq!(
        verify_recovery_target(&baseline, &observed, Some(&baseline.binding)),
        RecoveryVerification::Mismatch {
            reasons: vec![RecoveryMismatch::Submission {
                confirmed: Some(baseline.binding.clone())
            }]
        },
        "a historical binding cannot prove a newly observed instance"
    );

    // The core moves to another document and answers every further request
    // with its own rollback.
    endpoint.set_source_hash("source-b");
    endpoint.set_rolls_back(true);
    let error = client
        .reconcile()
        .await
        .expect_err("a rolled back restore is not an applied one");
    assert_eq!(
        error.core_kind(),
        Some(nyanpasu_core_manager::CoreErrorKind::ApplyFailed)
    );

    // A rolled back restore confirmed nothing, so it is passed no confirmation.
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    let RecoveryVerification::Mismatch { reasons } =
        verify_recovery_target(&baseline, &observed, None)
    else {
        panic!("the core is still running the other document, so nothing was recovered");
    };
    assert_eq!(
        reasons,
        vec![
            RecoveryMismatch::Content {
                expected: baseline.binding.revision.source_hash.clone(),
                observed: Some("source-b".into()),
            },
            RecoveryMismatch::Submission { confirmed: None },
        ]
    );

    // The IPC-only case. Local-IPC settings travel with the submission rather
    // than inside the YAML, so the document the core is left on can share the
    // receipt's source hash, its core, its host and its run intent while
    // listening on a different control channel. Every observable comparison
    // passes; the restore that rolled back is what refuses it.
    endpoint.set_source_hash(&baseline.binding.revision.source_hash);
    let observed = ObservedRuntime::from_status(&core.refresh_status().await.unwrap());
    let RecoveryVerification::Mismatch { reasons } =
        verify_recovery_target(&baseline, &observed, None)
    else {
        panic!("a rolled back restore never proves the settings it carried");
    };
    assert_eq!(
        reasons,
        vec![RecoveryMismatch::Submission { confirmed: None }],
        "the document matches and the control channel it was submitted with does not"
    );
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .expect("a baseline still exists")
            .revision
            .get(),
        baseline.revision.get(),
        "a failed apply does not move the recovery baseline"
    );
}

// -- T10 §1.11/§4: a live attempt and a write-ahead action -------------------
//
// Every test below isolates the execution domain the way it happens for real
// — a lost reply, an operation still running — and recovers it only through
// the explicit retry, reading what the two slots kept. The interruptions are
// scripted fakes and barriers; nothing here sleeps for an ordering.

use crate::actor_v2::endpoint::ControlEndpoint as _;
use nyanpasu_core_manager::OperationId;

use std::sync::atomic::Ordering;

use super::{
    ScriptedWaitEndpoint, WaitScript,
    mutations::{app_with_core, commit_source, fixture, overrides, scripted_fixture, settled},
};
use crate::client::application_workflow::{
    ApplicationWorkflowClient, Command, attempt::RecoveryView, mutation::DEFERRED_RETRY_BUDGET,
};

fn recovery(client: &ApplicationWorkflowClient) -> RecoveryView {
    client
        .mutation_journal()
        .recovery
        .expect("an isolated execution domain names its attempt")
}

fn isolated(client: &ApplicationWorkflowClient) -> bool {
    client.status().uncertain
}

/// The one operation the pending action names: the latest one submitted, as
/// nothing is submitted while an earlier action is unresolved.
fn pending_submission(scripted: &ScriptedWaitEndpoint) -> OperationId {
    *scripted
        .operations()
        .last()
        .expect("the pending action is a submission")
}

/// L9 (N4): an automatic retry of a committed target loses its receipt. It is
/// not an unconfirmed source transaction: once the lost operation is seen to
/// finish and the runtime is still on the baseline the retry found, the
/// target goes back as it was — charged once, with its health and schedule
/// untouched — and a later explicit retry applies it.
#[tokio::test]
async fn a_committed_target_retry_that_lost_its_receipt_goes_back_charged_once() {
    let f = scripted_fixture().await;
    let scripted = f.scripted.clone().expect("a scripted fixture");
    f.endpoint.set_failure(Some("queue_full"));
    let id = commit_source(&f.clash, overrides(serde_json::json!({"mode": "direct"})))
        .await
        .unwrap();
    assert_eq!(settled(&f.client, id).await.conclusion.label(), "Waiting");
    let scheduled = f.client.mutation_journal().deferred.unwrap();

    scripted.queue(WaitScript::Missing);
    f.client
        .call(Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    assert!(isolated(&f.client));
    let retried = pending_submission(&scripted);

    scripted.rescript(retried, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    let target = f.client.mutation_journal().deferred.expect("put back");
    assert_eq!(target.operation_id, scheduled.operation_id);
    assert_eq!(target.attempts_remaining, DEFERRED_RETRY_BUDGET - 1);
    assert_eq!(target.attempts, 1);
    assert_eq!(target.health, scheduled.health);
    assert_eq!(target.next_attempt, scheduled.next_attempt);

    f.endpoint.set_failure(None);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.mutation_journal().deferred.is_none());
    assert!(
        f.endpoint
            .reconciled_bytes()
            .last()
            .is_some_and(|bytes| String::from_utf8_lossy(bytes).contains("mode: direct"))
    );
}

/// L12: an automatic retry never recovers an isolated domain, even once the
/// evidence to do so exists; only the explicit one consumes the action.
#[tokio::test]
async fn an_automatic_retry_never_recovers() {
    let f = fixture().await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.reconcile().await.is_err());
    f.endpoint.set_result_missing(false);

    let refused = f
        .client
        .call(Command::RetryRuntime { explicit: false })
        .await
        .err();
    assert!(matches!(
        refused,
        Some(nyanpasu_runtime_values::runtime_error::RuntimeError::Isolated)
    ));
    assert!(isolated(&f.client));

    f.client.retry_runtime().await.unwrap();
    assert!(
        f.client.mutation_journal().recovery.is_none(),
        "the explicit retry consumed the action and re-established the runtime"
    );
    assert!(!isolated(&f.client));
}

/// A lifecycle attempt has no source decision and no committed target to
/// continue from. It stays isolated while its action is unobserved; once the
/// action is resolved, recovery re-establishes the runtime under a proven
/// owner from the committed configuration (§4.2), and admission reopens.
#[tokio::test]
async fn a_lifecycle_attempt_is_re_established_once_its_action_is_resolved() {
    let f = fixture().await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.reconcile().await.is_err());
    assert!(isolated(&f.client));
    let error = f.client.retry_runtime().await.unwrap_err();
    assert!(
        error.to_string().contains("core operation"),
        "still unobserved: {}",
        error
    );

    f.endpoint.set_result_missing(false);
    let submitted = f.endpoint.reconciled_bytes().len();
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert!(f.client.mutation_journal().recovery.is_none());
    assert!(f.client.mutation_journal().deferred.is_none());
    // The lost reconcile left a core running no receipt describes: it was
    // retired, and the committed configuration applied once, from a stop.
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted + 1);
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .binding
            .revision
            .source_hash,
        f.endpoint.status().await.unwrap().source_hash.unwrap(),
    );

    commit_source(
        &f.application,
        app_with_core(nyanpasu_config::application::ClashCore::ClashRs),
    )
    .await
    .unwrap();
}

/// The ordinary daemon, except that its next stop can be held past the
/// service actor's bound, so the workflow never hears how it ended.
/// A service daemon whose install is held at a barrier past the actor's
/// bound, while its probe keeps answering `Ready`.
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

/// L8 (N3): an install whose helper outlives its bound isolates the domain
/// although the daemon probes `Ready`. The pending service command resolves
/// only once the helper has ended, and recovery then re-establishes the
/// runtime.
#[tokio::test]
async fn a_service_command_still_running_keeps_the_domain_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Arc::new(ParkedInstall {
        delegate: crate::actor_v2::test_support::HostTransitionServiceAdapter {
            endpoint: TestControlEndpoint::succeeding_on(
                nyanpasu_runtime_values::execution_host::ExecutionHost::Service,
            ),
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            stopped: std::sync::atomic::AtomicBool::new(false),
            installed_for: Default::default(),
        },
        parked: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let local = TestControlEndpoint::succeeding();
    local.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
        None,
    );
    let core = CoreClient::spawn(local.clone()).await.unwrap();
    let service = ServiceClient::spawn_bounded(
        daemon.clone(),
        daemon.clone(),
        0,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();
    let (client, builder, _, _) = workflow_graph_with_clients(
        &dir,
        core,
        service.clone(),
        false,
        Arc::new(crate::client::SessionPortResolver::default()),
        tokio_util::sync::CancellationToken::new(),
    )
    .await;
    builder.release.notify_one();

    let error = client.install_service().await.unwrap_err();
    assert!(
        error.to_string().contains("still running"),
        "{}",
        error.to_string()
    );
    daemon.parked.notified().await;
    assert_eq!(
        service.probe().await.unwrap().phase,
        nyanpasu_runtime_values::core_status::ServicePhase::Ready,
        "the daemon probes a stable Ready while the helper runs"
    );
    assert!(client.status().uncertain);
    assert!(matches!(
        client.start_service().await.unwrap_err(),
        nyanpasu_runtime_values::runtime_error::RuntimeError::Isolated
    ));
    let error = client.retry_runtime().await.unwrap_err();
    assert!(
        error.to_string().contains("service Install command"),
        "the helper still runs: {}",
        error
    );

    assert!(local.reconciled_bytes().is_empty());

    daemon.release.notify_one();
    let resolved = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while client.retry_runtime().await.is_err() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(resolved.is_ok(), "the finished helper settles the command");
    assert!(!client.status().uncertain);
    assert!(client.mutation_journal().recovery.is_none());
    assert_eq!(
        local.reconciled_bytes().len(),
        1,
        "the runtime was re-established once the helper had ended"
    );
}

/// The router drives a service host whose core no receipt describes, and
/// Local is asked for. The handoff back is held at its stop past its caller's
/// budget, so startup ends with the handoff pending, and while the router is
/// still handing off a retry leaves it pending and the domain isolated. That
/// handoff is the only one the workflow makes, and it cannot finish before
/// its stop is released, so the caller's short budget decides nothing else.
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
    assert!(isolated(&g.client));
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
    assert!(isolated(&g.client));
    g
}

/// L10: the lost handoff completes. Once the router settles, recovery
/// resolves the handoff and re-establishes the runtime.
#[tokio::test]
async fn a_handoff_whose_answer_was_lost_is_recovered_once_the_router_settles() {
    let g = a_lost_handoff().await;

    g.service_host.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while g.client.retry_runtime().await.is_err() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the settled router lets recovery through");

    assert!(!isolated(&g.client));
    assert!(g.client.mutation_journal().deferred.is_none());
    assert_eq!(g.log(), ["service:stop", "local:reconcile"]);
}

/// L10, degraded (T10 §1.11): the lost handoff fails after its source
/// stopped answering, so the router is left degraded on the service host and
/// refuses every status read. The handoff is over all the same: recovery
/// resolves it and goes on to re-establish, which proves no owner through a
/// degraded router and leaves a waiting target rather than an isolated
/// domain.
#[tokio::test]
async fn a_lost_handoff_that_failed_into_a_degraded_router_is_resolved() {
    use nyanpasu_runtime_values::{
        core_status::EndpointConnectivity, execution_host::ExecutionHost,
    };
    let g = a_lost_handoff().await;
    let mut status = g.core.subscribe();

    g.core
        .report_endpoint_down("scripted: the service host stopped answering");
    g.service_host.delegate.set_failure(Some("apply_failed"));
    g.service_host.release.notify_one();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        status.wait_for(|status| {
            matches!(
                status.connectivity,
                EndpointConnectivity::Degraded {
                    desired: ExecutionHost::Service,
                    ..
                }
            )
        }),
    )
    .await
    .expect("the failed handoff leaves the router degraded")
    .unwrap();
    assert!(g.core.refresh_status().await.is_err());

    g.client.retry_runtime().await.unwrap();

    assert!(!isolated(&g.client));
    assert!(g.client.mutation_journal().recovery.is_none());
    let target = g
        .client
        .mutation_journal()
        .deferred
        .expect("the runtime still waits for a proven owner");
    assert_eq!(
        target.health,
        nyanpasu_runtime_values::configuration::ConvergenceHealth::RecoveryRequired
    );
    assert_eq!(
        g.log(),
        ["service:stop"],
        "nothing started without an owner"
    );
}

/// L11: a stop whose result was lost, over a core that still reports
/// running. Recovery honours the stop before anything else: it does not
/// settle while the stop fails, and once the core is seen stopped it settles
/// without starting anything.
#[tokio::test]
async fn a_lost_stop_is_recovered_by_a_confirmed_stop_and_nothing_else() {
    let f = fixture().await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.stop_core().await.is_err());
    assert!(isolated(&f.client));
    f.endpoint.set_result_missing(false);
    f.endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    f.endpoint.set_failure(Some("apply_failed"));
    let submitted = f.endpoint.reconciled_bytes().len();

    let error = f.client.retry_runtime().await.unwrap_err();

    assert!(matches!(
        error,
        nyanpasu_runtime_values::runtime_error::RuntimeError::RecoveryUnresolved { .. }
    ));
    assert!(isolated(&f.client));
    let view = recovery(&f.client);
    assert!(
        view.reason.contains("stop intent not satisfied"),
        "{}",
        view.reason
    );

    f.endpoint.set_failure(None);
    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    assert!(f.client.mutation_journal().recovery.is_none());
    assert!(f.client.mutation_journal().deferred.is_none());
    assert_eq!(
        f.endpoint.reconciled_bytes().len(),
        submitted,
        "the stop is all recovery owed"
    );
    assert!(matches!(
        crate::actor_v2::endpoint::ControlEndpoint::status(f.endpoint.as_ref())
            .await
            .unwrap()
            .state,
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
    ));
}
