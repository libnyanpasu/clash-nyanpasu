//! Factory wiring exercised through source owners and the real workflow.

use std::{
    future::Future,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use camino::Utf8Path;
use nyanpasu_contracts::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
use nyanpasu_state_engine::{DecisionHandle, StateDecision, StateParticipant, Version};
use tokio::sync::{mpsc, oneshot};

use super::mutations::{self, ParkingBuilder};
use crate::{
    client::{
        MutationCoordinators, application_workflow::ApplicationWorkflowClient,
        effects::ports::NoopCommitNotifications, profiles::ProfilesClient, test_support,
    },
    state::mutation::{
        impact::{ActivationIntent, MutationHints, RuntimeImpact},
        intent::{CommandClass, MutationIntent},
        ports::MutationParticipantFactory,
        value::MutationReceipt,
    },
};
use nyanpasu_runtime_values::runtime::{CommitReceipt, RuntimeCommitStatus};

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
    coordinators.profiles.connect(
        factory,
        Arc::new(NoopCommitNotifications),
        Arc::new(crate::state::mutation::IsolatedRuntimeGate),
    );
    let profiles_path = mutations::temp_path(&dir, "profiles.yaml");
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
    .expect("participant domains must prepare and settle through their factory");
    assert!(
        observed.try_recv().is_err(),
        "one factory call per source attempt"
    );
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}
