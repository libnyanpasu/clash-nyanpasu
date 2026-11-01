use std::{sync::atomic::Ordering, time::Duration};

use nyanpasu_core_manager::OperationId;
use tokio::time::Instant;

use super::{
    barrier,
    mutations::{fixture, mutate, no_local_write, overrides, plain, settled},
    startup::{DaemonState, Setup, graph},
};
use crate::{
    actor_v2::endpoint::ExecutionHost,
    client::application_workflow::{Message, mutation::MutationConclusion, policy::CommandClass},
    state::ReplaceIfVersionResult,
};

#[tokio::test(start_paused = true)]
async fn revocation_rearms_deferred_convergence_once() {
    let _clock = crate::client::jobs::explicit_test_time();
    let g = graph(Setup {
        schedule_ticks: true,
        ..Setup::default()
    })
    .await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    let before = g.client.mutation_journal().deferred.unwrap();
    let due = before.next_attempt.unwrap();
    g.session_end.set_pending(true);
    barrier(&g.client).await;
    tokio::time::advance(due.saturating_duration_since(Instant::now()) + Duration::from_secs(1))
        .await;
    g.client.0.actor.cast(Message::ConvergenceTick).unwrap();
    barrier(&g.client).await;
    let paused = g.client.mutation_journal().deferred.unwrap();
    assert_eq!(paused.attempts, before.attempts);
    assert_eq!(paused.attempts_remaining, before.attempts_remaining);
    assert_eq!(paused.next_attempt, before.next_attempt);
    g.local.delegate.set_failure(None);
    g.session_end.set_pending(false);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            barrier(&g.client).await;
            if g.client.mutation_journal().deferred.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        g.log()
            .iter()
            .filter(|entry| entry.ends_with(":reconcile"))
            .count(),
        2
    );
    g.client.0.actor.stop_and_wait(None, None).await.unwrap();
}

#[tokio::test]
async fn session_end_pauses_follow_service_until_revoked() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Stopped,
        ..Setup::default()
    })
    .await;
    g.start().await;
    assert_eq!(g.client.core_status().host, ExecutionHost::Local);
    g.session_end.set_pending(true);
    g.daemon
        .state
        .lock()
        .unwrap()
        .clone_from(&DaemonState::Running);
    g.publish_readiness().await;
    barrier(&g.client).await;
    assert_eq!(g.client.core_status().host, ExecutionHost::Local);
    g.session_end.set_pending(false);
    g.client.0.actor.cast(Message::ServiceReadiness).unwrap();
    barrier(&g.client).await;
    assert_eq!(g.client.core_status().host, ExecutionHost::Service);
}

#[tokio::test]
async fn session_end_finishes_admitted_try_and_keeps_explicit_admission_open() {
    let f = fixture().await;
    f.builder.park.store(true, Ordering::SeqCst);
    let id = OperationId::generate();
    let mutation = {
        let client = f.client.clone();
        let mut clash = f.clash;
        tokio::spawn(async move {
            mutate(
                &mut clash,
                &client,
                id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                plain(),
                no_local_write,
            )
            .await
        })
    };
    f.builder.entered.notified().await;
    f.session_end.set_pending(true);
    f.builder.park.store(false, Ordering::SeqCst);
    f.builder.release.notify_one();
    assert!(matches!(
        mutation.await.unwrap(),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    f.client.reconcile().await.unwrap();
    assert!(!f.shutdown.is_cancelled());
}
