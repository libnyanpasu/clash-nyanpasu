//! Core observations wake reconciliation without waiting for the recovery tick.

use std::sync::Arc;

use nyanpasu_ipc::api::status::CoreStateDetail;
use nyanpasu_runtime_values::mutation::ConfigDomain;
use tokio_util::sync::CancellationToken;

use super::{barrier, stopped_endpoint, workflow_graph_with_clients};
use crate::{
    actor_v2::{CoreClient, endpoint::ControlEndpoint as _, service_actor::ServiceClient},
    client::{application_workflow::ApplicationWorkflowClient, test_support::TestControlEndpoint},
};

struct Fixture {
    client: ApplicationWorkflowClient,
    builder: Arc<super::BlockingBuilder>,
    core: CoreClient,
    endpoint: Arc<TestControlEndpoint>,
    clash: crate::client::clash_config::ClashConfigClient,
    shutdown: CancellationToken,
    _dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = stopped_endpoint();
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        // Establish the watch baseline before subscribing the workflow.
        core.refresh_status().await.unwrap();
        let adapter = Arc::new(crate::actor_v2::test_support::IdleServiceAdapter);
        let service = ServiceClient::spawn(adapter.clone(), adapter, 0)
            .await
            .unwrap();
        let shutdown = CancellationToken::new();
        let (client, builder, _, clash) = workflow_graph_with_clients(
            &dir,
            core.clone(),
            service,
            false,
            Arc::default(),
            shutdown.clone(),
        )
        .await;
        builder.release.notify_one();
        client.reconcile().await.unwrap();
        let fixture = Self {
            client,
            builder,
            core,
            endpoint,
            clash,
            shutdown,
            _dir: dir,
        };
        fixture.publish().await;
        fixture
    }

    async fn publish(&self) {
        let mut observations = self.client.0.core_observations.clone();
        let before = observations.borrow_and_update().0;
        self.core.refresh_status().await.unwrap();
        observations
            .wait_for(|(count, _)| *count > before)
            .await
            .unwrap();
        // The forwarder records consumption after casting, so this follows its round.
        barrier(&self.client).await;
        self.client.settle_core_changes().await;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

// Guards forwarding a changed state into a round that applies the pending committed target.
#[tokio::test(start_paused = true)]
async fn running_publication_applies_a_target_waiting_for_a_transition() {
    let f = Fixture::new().await;
    let running = f.endpoint.status().await.unwrap();
    f.endpoint.set_status(
        Some(CoreStateDetail::Starting { epoch: 2 }),
        running.applied_kind,
    );
    f.publish().await;
    let before = f.endpoint.reconciled_bytes().len();
    let mut config = f.clash.snapshot().state;
    config.mixed_port = super::fixed_port(48621);
    f.clash.replace(config).await.unwrap();
    let version = f.clash.snapshot().version;
    f.client.source_committed(ConfigDomain::Clash, version);
    let status = f.client.settled(ConfigDomain::Clash, version).await;
    assert_eq!(status.conclusion.label(), "Waiting");
    assert_eq!(f.endpoint.reconciled_bytes().len(), before);

    f.endpoint.set_status(running.state, running.applied_kind);
    f.publish().await;

    assert_eq!(f.client.reconciler_status().conclusion.label(), "Converged");
    let submitted = f.endpoint.reconciled_bytes();
    assert_eq!(submitted.len(), before + 1);
    let applied: yaml_serde::Value = yaml_serde::from_slice(submitted.last().unwrap()).unwrap();
    assert_eq!(applied["mixed-port"].as_u64(), Some(48621));
    // The application's own revision change needs no special handling: it settles idle.
    f.publish().await;
    assert_eq!(f.endpoint.reconciled_bytes().len(), before + 1);
    assert_eq!(
        f.client.reconciler_status().recent.back().unwrap().trigger,
        "CoreChanged"
    );
}

// Guards comparing the trigger key before casting, including refreshes caused by a round itself.
#[tokio::test(start_paused = true)]
async fn ten_identical_publications_do_not_start_a_round() {
    let f = Fixture::new().await;
    let before = f.client.reconciler_status().seq;
    for _ in 0..10 {
        f.publish().await;
        assert_eq!(f.client.reconciler_status().seq, before);
    }
}

// Guards the CoreChanged trigger remaining non-forced when the same target has failed.
#[tokio::test(start_paused = true)]
async fn a_failure_stop_publication_blocks_without_restarting() {
    let f = Fixture::new().await;
    let before = f.client.reconciler_status().seq;
    f.endpoint.reset_tracking();
    f.endpoint.set_status(
        Some(CoreStateDetail::Stopped {
            reason: Some("core crashed".to_owned()),
        }),
        None,
    );
    f.publish().await;

    let status = f.client.reconciler_status();
    assert_eq!(status.seq, before + 2, "one round starts and finishes");
    assert_eq!(status.conclusion.label(), "NotInEffect");
    assert_eq!(status.recent.back().unwrap().trigger, "CoreChanged");
    assert_eq!(f.endpoint.submissions(), 0);
}

// Guards bounding queued changes while retaining one observation made during a round.
#[tokio::test(start_paused = true)]
async fn alternating_publications_during_a_gated_round_queue_one_further_round() {
    use std::sync::atomic::Ordering;

    let f = Fixture::new().await;
    f.builder.entered.notified().await;
    f.builder.calls.store(0, Ordering::SeqCst);
    f.builder.fail.store(true, Ordering::SeqCst);
    let mut config = f.clash.snapshot().state;
    config.mixed_port = super::fixed_port(48622);
    f.clash.replace(config).await.unwrap();
    let before = f.client.reconciler_status().seq;
    let running = Some(CoreStateDetail::Running { epoch: 1, pid: 42 });
    f.endpoint.set_status(running.clone(), None);
    f.core.refresh_status().await.unwrap();
    f.builder.entered.notified().await;

    let mut observations = f.client.0.core_observations.clone();
    for state in [
        Some(CoreStateDetail::Starting { epoch: 2 }),
        running.clone(),
        Some(CoreStateDetail::Starting { epoch: 2 }),
        running,
    ] {
        f.endpoint.set_status(state.clone(), None);
        f.core.refresh_status().await.unwrap();
        observations
            .wait_for(|(_, status)| status.snapshot.as_ref().and_then(|s| s.state.clone()) == state)
            .await
            .unwrap();
    }
    assert_eq!(
        f.client.reconciler_status().seq,
        before + 1,
        "the first round is still held"
    );
    f.builder.release.notify_one();
    barrier(&f.client).await;
    f.client.settle_core_changes().await;
    let status = f.client.reconciler_status();
    assert_eq!(
        status.seq,
        before + 4,
        "the held round and exactly one further round finish"
    );
    assert_eq!(status.conclusion.label(), "NotInEffect");
    assert_eq!(status.recent.back().unwrap().trigger, "CoreChanged");
}
