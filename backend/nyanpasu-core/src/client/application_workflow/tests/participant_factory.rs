//! Factory wiring exercised through source owners and the real workflow.

use std::{
    future::Future,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use camino::Utf8Path;
use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig, ReleaseChannel},
    clash::config::ClashConfig,
};
use nyanpasu_contracts::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
use nyanpasu_core_manager::OperationId;
use nyanpasu_state_engine::{DecisionHandle, StateDecision, StateParticipant, Version};
use struct_patch::Patch as _;
use tokio::sync::{mpsc, oneshot};

use super::mutations::{self, ParkingBuilder};
use crate::{
    client::{
        MutationCoordinators, application::ApplicationClient,
        application_workflow::ApplicationWorkflowClient, clash_config::ClashConfigClient,
        effects::ports::NoopCommitNotifications, profiles::ProfilesClient, test_support,
    },
    state::{
        config_error::ConfigError,
        mutation::{
            MutationCoordinator,
            impact::{ActivationIntent, MutationHints, RequestedRuntimeFields, RuntimeImpact},
            intent::{CommandClass, MutationIntent},
            ports::MutationParticipantFactory,
            value::{MutationConclusion, MutationReceipt},
        },
    },
};
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, ConfigDomain},
    runtime::{CommitReceipt, RuntimeCommitStatus},
    runtime_error::RuntimeError,
};

struct FactoryObservation {
    intent: MutationIntent,
    decision: DecisionHandle,
}

struct RecordingFactory {
    workflow: ApplicationWorkflowClient,
    observations: mpsc::UnboundedSender<FactoryObservation>,
}

impl<T: Clone + Send + Sync + 'static> MutationParticipantFactory<T> for RecordingFactory
where
    ApplicationWorkflowClient: MutationParticipantFactory<T>,
{
    fn make(
        &self,
        intent: MutationIntent,
        decision: DecisionHandle,
        settle: oneshot::Sender<MutationReceipt>,
    ) -> StateParticipant<T> {
        self.observations
            .send(FactoryObservation {
                intent: intent.clone(),
                decision: decision.clone(),
            })
            .unwrap();
        self.workflow.make(intent, decision, settle)
    }
}

async fn assert_source_attempt(
    observations: &mut mpsc::UnboundedReceiver<FactoryObservation>,
    builder: &ParkingBuilder,
    path: &Utf8Path,
    hints: MutationHints,
    class: CommandClass,
    impact: RuntimeImpact,
    mutation: impl Future<Output = CommitReceipt> + Send + 'static,
) {
    let bytes = std::fs::read(path).ok();
    builder.park.store(true, Ordering::SeqCst);
    let work = tokio::spawn(mutation);
    let observation = observations.recv().await.unwrap();
    builder.entered.notified().await;
    assert_eq!(observation.intent.hints, hints);
    assert_eq!(observation.intent.class, class);
    assert_eq!(observation.intent.impact, impact);
    assert_eq!(observation.decision.decision(), StateDecision::Undecided);
    assert_eq!(
        std::fs::read(path).ok(),
        bytes,
        "prepare precedes persistence"
    );
    builder.park.store(false, Ordering::SeqCst);
    builder.release.notify_one();
    let receipt = work.await.unwrap();
    assert_eq!(
        receipt.operation_id,
        Some(observation.intent.operation_id.to_string())
    );
    assert_eq!(receipt.runtime, RuntimeCommitStatus::Applied);
    assert_eq!(
        observation.decision.decision(),
        StateDecision::Committed {
            version: Version::new(receipt.source_version),
        }
    );
}

#[tokio::test]
async fn participant_factory_receives_the_source_decision_and_intent() {
    let f = mutations::fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let (observations, mut observed) = mpsc::unbounded_channel();
    let factory = Arc::new(RecordingFactory {
        workflow: f.client.clone(),
        observations,
    });
    let coordinators = MutationCoordinators::pending();
    coordinators
        .application
        .connect(factory.clone(), Arc::new(NoopCommitNotifications));
    coordinators
        .clash
        .connect(factory.clone(), Arc::new(NoopCommitNotifications));
    coordinators
        .profiles
        .connect(factory, Arc::new(NoopCommitNotifications));
    let app_path = mutations::temp_path(&dir, "application.yaml");
    let clash_path = mutations::temp_path(&dir, "clash.yaml");
    let profiles_path = mutations::temp_path(&dir, "profiles.yaml");
    let application = ApplicationClient::new(
        coordinators.application,
        ReleaseChannel::Stable,
        app_path.clone(),
        f.shutdown.child_token(),
        &f.tasks,
    )
    .await
    .unwrap();
    let clash = ClashConfigClient::new(
        coordinators.clash,
        clash_path.clone(),
        f.shutdown.child_token(),
        &f.tasks,
    )
    .await
    .unwrap();
    let profiles = ProfilesClient::new(
        coordinators.profiles,
        profiles_path.clone(),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_support::test_materialization_port(),
        f.shutdown.child_token(),
        &f.tasks,
    )
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_secs(10), async {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.core = Some(ClashCore::ClashRs);
        assert_source_attempt(
            &mut observed,
            &f.builder,
            &app_path,
            MutationHints {
                requested: RequestedRuntimeFields::runtime(),
                ..Default::default()
            },
            CommandClass::ExplicitSwitch,
            RuntimeImpact::CoreSwap,
            async move { application.patch(patch).await.unwrap().receipt.unwrap() },
        )
        .await;

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_clash_fields = Some(!clash.snapshot().state.enable_clash_fields);
        assert_source_attempt(
            &mut observed,
            &f.builder,
            &clash_path,
            MutationHints {
                requested: RequestedRuntimeFields::runtime(),
                ..Default::default()
            },
            CommandClass::Save,
            RuntimeImpact::Reconcile,
            async move { clash.patch(patch).await.unwrap().receipt.unwrap() },
        )
        .await;

        assert_source_attempt(
            &mut observed,
            &f.builder,
            &profiles_path,
            MutationHints {
                activation: ActivationIntent::Deactivate,
                ..Default::default()
            },
            CommandClass::ExplicitSwitch,
            RuntimeImpact::Reconcile,
            async move { profiles.set_current(None).await.unwrap().receipt },
        )
        .await;
    })
    .await
    .expect("each domain must prepare and settle through its own factory");
    assert!(
        observed.try_recv().is_err(),
        "one factory call per source attempt"
    );
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}

#[tokio::test]
async fn undelivered_mutation_is_rejected_but_lost_verdict_is_failed() {
    for delivered in [false, true] {
        let f = mutations::fixture().await;
        let coordinator = MutationCoordinator::pending();
        coordinator.connect(
            Arc::new(f.client.clone()),
            Arc::new(NoopCommitNotifications),
        );
        let before = f.application.snapshot_handle().load();
        let application = ApplicationClient::from_manager(
            coordinator,
            f.application,
            ReleaseChannel::Stable,
            f.shutdown.child_token(),
            &f.tasks,
        )
        .await
        .unwrap();
        if !delivered {
            f.client.kill().await;
        }
        f.builder.park.store(true, Ordering::SeqCst);
        let writer = application.clone();
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
        let work = tokio::spawn(async move { writer.patch(patch).await });
        if delivered {
            f.builder.entered.notified().await;
            f.client.kill().await;
        }
        let error = work.await.unwrap().unwrap_err();
        match (delivered, error) {
            (
                false,
                ConfigError::Commit {
                    source: CommitAborted::RuntimeRefused { errors, .. },
                    ..
                },
            ) => {
                assert!(matches!(&*errors[0], RuntimeError::OwnerUnavailable));
            }
            (
                true,
                ConfigError::Commit {
                    source: CommitAborted::RuntimeFailed { errors, .. },
                    ..
                },
            ) => {
                assert!(matches!(
                    &*errors[0],
                    RuntimeError::OwnerUnresponsive { .. }
                ));
            }
            (_, error) => panic!("delivered={delivered}: unexpected source error {error:?}"),
        }
        assert_eq!(application.snapshot().version, *before.version.as_ref());
        f.shutdown.cancel();
        f.tasks.close();
        f.tasks.wait().await;
    }
}

struct SettlementObservation {
    operation_id: OperationId,
    receiver: oneshot::Receiver<MutationReceipt>,
    source: oneshot::Sender<MutationReceipt>,
}

struct CapturingSettlementFactory {
    workflow: ApplicationWorkflowClient,
    observations: mpsc::UnboundedSender<SettlementObservation>,
}

impl MutationParticipantFactory<NyanpasuAppConfig> for CapturingSettlementFactory {
    fn make(
        &self,
        intent: MutationIntent,
        decision: DecisionHandle,
        source: oneshot::Sender<MutationReceipt>,
    ) -> StateParticipant<NyanpasuAppConfig> {
        let (settle, receiver) = oneshot::channel();
        self.observations
            .send(SettlementObservation {
                operation_id: intent.operation_id,
                receiver,
                source,
            })
            .unwrap();
        self.workflow.make(intent, decision, settle)
    }
}

#[tokio::test]
async fn dropping_the_settlement_receiver_does_not_cancel_owner_work() {
    let f = mutations::fixture().await;
    let (observations, mut observed) = mpsc::unbounded_channel();
    let coordinator = MutationCoordinator::pending();
    coordinator.connect(
        Arc::new(CapturingSettlementFactory {
            workflow: f.client.clone(),
            observations,
        }),
        Arc::new(NoopCommitNotifications),
    );
    let application = ApplicationClient::from_manager(
        coordinator,
        f.application,
        ReleaseChannel::Stable,
        f.shutdown.child_token(),
        &f.tasks,
    )
    .await
    .unwrap();
    let before = application.snapshot();
    f.builder.park.store(true, Ordering::SeqCst);
    let writer = application.clone();
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
    let work = tokio::spawn(async move { writer.patch(patch).await });
    let observation = observed.recv().await.unwrap();
    f.builder.entered.notified().await;
    assert_eq!(application.snapshot().version, before.version);
    drop(observation.receiver);
    f.builder.release.notify_one();
    let receipt = mutations::settled(&f.client, observation.operation_id).await;
    assert_eq!(receipt.domain, ConfigDomain::Application);
    assert_eq!(receipt.conclusion, MutationConclusion::Confirmed);
    assert!(application.snapshot().version > before.version);
    assert!(!f.endpoint.reconciled_bytes().is_empty());
    // Release the source's independent waiter after observing owner completion.
    observation.source.send(receipt).unwrap();
    assert_eq!(
        work.await.unwrap().unwrap().receipt.unwrap().runtime,
        RuntimeCommitStatus::Applied
    );
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}
