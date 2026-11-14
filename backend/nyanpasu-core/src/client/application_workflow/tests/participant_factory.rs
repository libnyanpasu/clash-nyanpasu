//! Factory wiring exercised through source owners and the real workflow.

use std::sync::Arc;

use nyanpasu_contracts::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
use nyanpasu_state_engine::{DecisionHandle, StateParticipant};
use tokio::sync::{mpsc, oneshot};

use super::mutations;
use crate::{
    client::{
        MutationCoordinators, application_workflow::ApplicationWorkflowClient,
        effects::ports::NoopCommitNotifications, profiles::ProfilesClient, test_support,
    },
    state::mutation::{
        intent::MutationIntent, ports::MutationParticipantFactory, value::MutationReceipt,
    },
};

struct FactoryObservation {
    _intent: MutationIntent,
    _decision: DecisionHandle,
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
                _intent: intent.clone(),
                _decision: decision.clone(),
            })
            .unwrap();
        self.workflow.make(intent, decision, settle)
    }
}

#[tokio::test]
async fn profiles_commits_do_not_enlist_participants() {
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

    let report = profiles.set_valid_fields(vec!["dns".into()]).await.unwrap();
    assert!(report.reaches_runtime);
    assert!(report.receipt.operation_id.is_some());
    assert!(
        observed.try_recv().is_err(),
        "source commits do not enlist participants"
    );
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}
