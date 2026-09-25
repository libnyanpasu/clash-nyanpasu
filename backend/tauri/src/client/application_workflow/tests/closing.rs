//! T10 §5.4 steps 1 and 3 on the workflow alone: closing admission refuses
//! what is queued and stops nothing, and the settle wait only watches the
//! running operation reach its own end (X2–X4, V36).

use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::state::ReplaceIfVersionResult;
use nyanpasu_core_manager::OperationId;
use tokio::sync::Notify;

use super::{
    super::{ClosingAck, Settlement, attempt::AttemptStage, mutation::MutationConclusion},
    WaitScript,
    mutations::{
        Rejector, app_with_core, fixture, mutate, no_local_write, overrides, parked_local_write,
        plain, refused, scripted_fixture, settled, simple_mutate, test_budgets,
    },
};
use crate::client::{app_lifecycle::Reply, application_workflow::policy::CommandClass};

/// Long enough that only a stuck operation reaches it.
const SETTLE: Duration = Duration::from_secs(5);
/// The wait a test lets run out on purpose while an operation is held.
const CUT_SHORT: Duration = Duration::from_millis(50);

async fn begin_closing(client: &super::super::ApplicationWorkflowClient) -> ClosingAck {
    match client.begin_closing().await {
        Reply::Answered(ack) => ack,
        _ => panic!("the workflow should acknowledge closing"),
    }
}

#[tokio::test]
async fn closing_admission_refuses_new_work_and_stops_nothing_by_itself() {
    let f = fixture(test_budgets()).await;
    let submitted = f.endpoint.submissions();

    let ack = begin_closing(&f.client).await;

    assert_eq!((ack.rejected, ack.active), (0, None));
    assert!(f.client.status().shutting_down);
    assert!(f.client.reconcile().await.is_err(), "closed to new work");
    assert_eq!(
        f.client.wait_settled(SETTLE).await,
        Settlement::Settled { isolated: false }
    );
    assert_eq!(
        f.endpoint.submissions(),
        submitted,
        "closing admission alone never stops the core"
    );

    // The shutdown request is what stops it.
    assert!(f.client.shutdown().await.unwrap().stop.is_ok());
    assert_eq!(f.endpoint.submissions(), submitted + 1);
}

/// X2: a Try in flight is never cut short. The settle wait names it and its
/// stage while it runs, and settles once it confirmed.
#[tokio::test]
async fn closing_during_a_try_waits_for_it_to_confirm() {
    let f = fixture(test_budgets()).await;
    f.builder.park.store(true, Ordering::SeqCst);
    let operation_id = OperationId::generate();
    let mutation = {
        let client = f.client.clone();
        let mut clash = f.clash;
        tokio::spawn(async move {
            mutate(
                &mut clash,
                &client,
                operation_id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                Duration::from_secs(10),
                plain(),
                no_local_write,
            )
            .await
        })
    };
    f.builder.entered.notified().await;

    let ack = begin_closing(&f.client).await;
    assert_eq!(ack.active, Some(operation_id));
    assert_eq!(
        f.client.wait_settled(CUT_SHORT).await,
        Settlement::Unsettled {
            operation: Some(operation_id),
            attempt: Some((operation_id, AttemptStage::TryingCritical)),
        }
    );

    f.builder.park.store(false, Ordering::SeqCst);
    f.builder.release.notify_one();
    assert!(matches!(
        mutation.await.unwrap(),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&f.client, operation_id).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert_eq!(
        f.client.wait_settled(SETTLE).await,
        Settlement::Settled { isolated: false }
    );
}

/// X3: closing keeps a transaction waiting for its decision and refuses the
/// one queued behind it without touching its source.
#[tokio::test]
async fn closing_during_await_decision_keeps_the_decision_wait() {
    let f = fixture(test_budgets()).await;
    let submitted = f.endpoint.submissions();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let undecided = {
        let client = f.client.clone();
        let mut clash = f.clash;
        let (entered, release) = (entered.clone(), release.clone());
        tokio::spawn(async move {
            mutate(
                &mut clash,
                &client,
                operation_id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                Duration::from_secs(10),
                plain(),
                parked_local_write(entered, release),
            )
            .await
        })
    };
    entered.notified().await;

    let ack = begin_closing(&f.client).await;
    assert_eq!(ack.active, Some(operation_id));
    assert_eq!(
        f.client.wait_settled(CUT_SHORT).await,
        Settlement::Unsettled {
            operation: Some(operation_id),
            attempt: Some((operation_id, AttemptStage::AwaitDecision)),
        }
    );
    let mut application = f.application;
    let before = application.snapshot_handle().load().version;
    let (_, queued) = simple_mutate(
        &mut application,
        &f.client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&queued), "{queued:?}");
    assert_eq!(application.snapshot_handle().load().version, before);

    release.notify_one();
    assert!(matches!(
        undecided.await.unwrap(),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&f.client, operation_id).await.conclusion,
        MutationConclusion::Confirmed,
        "closing does not destroy an undecided transaction"
    );
    assert_eq!(
        f.client.wait_settled(SETTLE).await,
        Settlement::Settled { isolated: false }
    );
    assert_eq!(
        f.endpoint.submissions(),
        submitted + 1,
        "the Try applied once, and closing stopped nothing"
    );
}

/// X4: a Cancel runs to its real completion. Its restore is held at the core
/// while the settle wait runs out, and the baseline is back once it lands.
#[tokio::test]
async fn closing_during_a_cancel_waits_for_the_restore() {
    let mut f = scripted_fixture(test_budgets()).await;
    let (_, primed) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let baseline = f.store.last_confirmed_runtime_receipt().unwrap();
    let scripted = f.scripted.clone().unwrap();
    // The Try applies; the Cancel's restore is held.
    scripted.queue(WaitScript::Deliver);
    scripted.queue(WaitScript::Held);
    f.clash.add_subscriber(Box::new(Rejector));
    let operation_id = OperationId::generate();
    let vetoed = mutate(
        &mut f.clash,
        &f.client,
        operation_id,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        Duration::from_secs(10),
        plain(),
        no_local_write,
    )
    .await;
    assert!(refused(&vetoed), "{vetoed:?}");
    scripted.held().await;

    let ack = begin_closing(&f.client).await;
    assert_eq!(ack.active, Some(operation_id));
    assert_eq!(
        f.client.wait_settled(CUT_SHORT).await,
        Settlement::Unsettled {
            operation: Some(operation_id),
            attempt: Some((operation_id, AttemptStage::Cancelling)),
        }
    );

    scripted.release();
    assert_eq!(
        f.client.wait_settled(SETTLE).await,
        Settlement::Settled { isolated: false }
    );
    assert_eq!(
        settled(&f.client, operation_id).await.conclusion,
        MutationConclusion::Cancelled
    );
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest,
        "the restore finished before the domain counted as settled"
    );
}

/// The abandoned-client path is unchanged: once every client is gone, the
/// workflow stops the core itself and then its own actor.
#[tokio::test]
async fn an_abandoned_workflow_still_stops_the_core() {
    let f = fixture(test_budgets()).await;
    let submitted = f.endpoint.submissions();
    let actor = f.client.0.actor.get_cell();

    drop(f.client);

    actor
        .wait(Some(SETTLE))
        .await
        .expect("the abandoned workflow stops after its shutdown");
    assert_eq!(f.endpoint.submissions(), submitted + 1, "one stop");
}
