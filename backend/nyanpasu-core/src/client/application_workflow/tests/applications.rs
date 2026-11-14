//! Submission outcomes preserve confirmed products and expose only confirmed ports.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use struct_patch::Patch as _;

use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::core::v2::OperationInfo;
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost, mutation::ConfigDomain, runtime::RuntimeCommitStatus,
};
use tokio_util::sync::CancellationToken;

use super::{ScriptedWaitEndpoint, WaitScript};
use crate::{
    actor_v2::{
        CoreClient,
        endpoint::{
            CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
        },
        service_actor::ServiceClient,
    },
    client::{
        SessionPortResolver,
        test_support::{self, TestControlEndpoint},
    },
};

// Guards publication after acceptance, transition closure on success/rollback, and invalidation on Unknown.
#[tokio::test]
async fn lifecycle_start_publishes_only_after_acceptance_and_preserves_ports_on_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = super::stopped_endpoint();
    let scripted = ScriptedWaitEndpoint::new(endpoint.clone());
    let core = CoreClient::spawn(scripted.clone()).await.unwrap();
    let process = Arc::new(crate::actor_v2::test_support::IdleServiceAdapter);
    let service = ServiceClient::spawn(process.clone(), process, 0)
        .await
        .unwrap();
    let ports = Arc::new(SessionPortResolver::default());
    let shutdown = CancellationToken::new();
    let (client, builder, _, clash) = super::workflow_graph_with_clients(
        &dir,
        core,
        service,
        false,
        ports.clone(),
        shutdown.clone(),
    )
    .await;
    builder.release.notify_one();
    scripted.queue(WaitScript::Held);
    let starting = tokio::spawn({
        let client = client.clone();
        async move { client.reconcile().await }
    });
    scripted.held().await;
    let product = builder.delegate.paths.product();
    assert!(
        !product.exists(),
        "a start with no accepted receipt must not publish a product"
    );
    assert!(ports.confirmed().is_none());
    scripted.release();
    starting.await.unwrap().unwrap();
    let accepted = std::fs::read(product).unwrap();
    let confirmed = ports
        .confirmed()
        .expect("successful apply closes the transition");
    let revision = client
        .0
        .runtime
        .last_confirmed_runtime_receipt()
        .unwrap()
        .revision;
    let mut config = clash.snapshot().state;
    config.overrides.apply(
        nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
            mode: Some(nyanpasu_config::clash::config::overrides::Mode::Global),
            ..Default::default()
        },
    );
    clash.replace(config).await.unwrap();
    endpoint.set_rolls_back(true);
    client
        .reconcile()
        .await
        .expect_err("rolled-back configuration is refused");
    assert_eq!(ports.confirmed(), Some(confirmed.clone()));
    assert_eq!(std::fs::read(product).unwrap(), accepted);
    assert_eq!(
        client
            .0
            .runtime
            .last_confirmed_runtime_receipt()
            .unwrap()
            .revision,
        revision
    );
    builder.fail.store(true, Ordering::SeqCst);
    let error = client
        .reconcile()
        .await
        .expect_err("a failed build is refused before submission");
    assert!(matches!(
        error,
        nyanpasu_runtime_values::runtime_error::RuntimeError::BuildRuntime { .. }
    ));
    assert_eq!(ports.confirmed(), Some(confirmed));
    assert_eq!(std::fs::read(product).unwrap(), accepted);
    assert_eq!(
        client.0.reconciler.borrow().conclusion.label(),
        "NotInEffect"
    );
    builder.fail.store(false, Ordering::SeqCst);
    endpoint.set_rolls_back(false);
    scripted.queue(WaitScript::Missing);
    client
        .reconcile()
        .await
        .expect_err("unobserved submission is unknown");
    assert!(ports.confirmed().is_none());
    assert_eq!(std::fs::read(product).unwrap(), accepted);
    shutdown.cancel();
}

struct InvalidRevisionEndpoint {
    delegate: Arc<TestControlEndpoint>,
    invalid_epoch: AtomicBool,
}

#[async_trait::async_trait]
impl ControlEndpoint for InvalidRevisionEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        let mut status = self.delegate.status().await?;
        if self.invalid_epoch.load(Ordering::SeqCst) {
            status.revision.as_mut().unwrap().epoch = 0;
        }
        Ok(status)
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.delegate.submit(submission).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.delegate.wait_operation(id, timeout).await
    }
}

// Guards treating the facade's invalid-epoch Internal refusal as blocked, with no automatic replay.
#[tokio::test(start_paused = true)]
async fn not_submitted_internal_is_refused_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Arc::new(InvalidRevisionEndpoint {
        delegate: TestControlEndpoint::succeeding(),
        invalid_epoch: AtomicBool::new(false),
    });
    let args = test_support::test_client_args_with_endpoint(
        &dir,
        endpoint.clone(),
        test_support::profile_test_adapters(&dir),
    )
    .await;
    let client = crate::client::NyanpasuClient::try_new(args).await.unwrap();
    endpoint.delegate.prime(&client).await;
    assert!(client.inner.ports.confirmed().is_some());
    let previous = client.promoted_runtime().await.unwrap().revision;
    let mut receipt = client
        .inner
        .application_workflow
        .0
        .runtime
        .last_confirmed_runtime_receipt()
        .unwrap()
        .as_ref()
        .clone();
    receipt.binding.revision.epoch = 0;
    client
        .inner
        .application_workflow
        .0
        .runtime
        .confirm_applied(Arc::new(receipt));
    let confirmed_ports = client.inner.ports.confirmed();
    endpoint.delegate.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 0, pid: 42 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    endpoint.invalid_epoch.store(true, Ordering::SeqCst);
    let outcome = client
        .patch_runtime_overrides(
            nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
                ipv6: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        super::commit_first::receipt(&outcome).runtime,
        RuntimeCommitStatus::Deferred
    );
    let status = client
        .inner
        .application_workflow
        .settled(
            ConfigDomain::Clash,
            client.inner.clash_config.snapshot().version,
        )
        .await;
    let super::super::Conclusion::NotInEffect { cause } = status.conclusion else {
        panic!("an invalid epoch must refuse the apply")
    };
    assert_eq!(
        cause.core_kind(),
        Some(nyanpasu_core_manager::CoreErrorKind::Internal)
    );
    assert_eq!(client.inner.ports.confirmed(), confirmed_ports);
    assert_eq!(client.promoted_runtime().await.unwrap().revision, previous);
    let sequence = status.seq;
    assert_eq!(endpoint.delegate.submissions(), 0);
    // An unchanged refused target is not resubmitted by ticks.
    tokio::time::advance(Duration::from_secs(60)).await;
    let workflow = &client.inner.application_workflow;
    workflow.0.actor.cast(super::super::Message::Tick).unwrap();
    super::barrier(workflow).await;
    assert_eq!(endpoint.delegate.submissions(), 0);
    assert_eq!(workflow.0.reconciler.borrow().seq, sequence);
    client.request_shutdown();
    client.wait_shutdown().await;
}
