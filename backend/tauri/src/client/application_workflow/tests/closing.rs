//! The workflow's side of the shutdown: once its token is cancelled it admits
//! nothing new, lets the running operation reach its own end, and only then
//! stops the core and itself (X2–X4, V16).

use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::state::ReplaceIfVersionResult;
use nyanpasu_core_manager::OperationId;
use tokio::sync::Notify;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    super::mutation::MutationConclusion,
    WaitScript,
    mutations::{
        Rejector, app_with_core, fixture, mutate, no_local_write, overrides, parked_local_write,
        plain, refused, scripted_fixture, settled, simple_mutate, test_budgets,
    },
};
use crate::client::application_workflow::policy::CommandClass;

/// Long enough that only a stuck operation reaches it.
const SETTLE: Duration = Duration::from_secs(5);
/// How long a test lets the shutdown wait run while an operation is held.
const CUT_SHORT: Duration = Duration::from_millis(50);

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
    let f = fixture(test_budgets()).await;
    let submitted = f.endpoint.submissions();

    request_shutdown(&f.shutdown, &f.tasks);

    assert!(f.client.reconcile().await.is_err(), "closed to new work");
    assert!(stopped_within(&f.tasks, SETTLE).await);
    assert!(f.client.status().shutting_down);
    assert_eq!(f.endpoint.submissions(), submitted + 1, "one stop");
}

/// X2 (V16): a Try in flight is never cut short. The core stops only once it
/// confirmed.
#[tokio::test]
async fn the_shutdown_during_a_try_waits_for_it_to_confirm() {
    let f = fixture(test_budgets()).await;
    let submitted = f.endpoint.submissions();
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
                plain(),
                no_local_write,
            )
            .await
        })
    };
    f.builder.entered.notified().await;

    request_shutdown(&f.shutdown, &f.tasks);
    assert!(
        !stopped_within(&f.tasks, CUT_SHORT).await,
        "the Try still runs"
    );
    assert_eq!(
        f.endpoint.submissions(),
        submitted,
        "nothing stopped the core"
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
    assert!(stopped_within(&f.tasks, SETTLE).await);
    assert_eq!(
        f.endpoint.submissions(),
        submitted + 2,
        "the Try applied, then the core stopped"
    );
}

/// X3 (V16): the shutdown keeps a transaction waiting for its decision and
/// refuses the one that arrives behind it without touching its source.
#[tokio::test]
async fn the_shutdown_during_await_decision_keeps_the_decision_wait() {
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
                plain(),
                parked_local_write(entered, release),
            )
            .await
        })
    };
    entered.notified().await;

    request_shutdown(&f.shutdown, &f.tasks);
    assert!(
        !stopped_within(&f.tasks, CUT_SHORT).await,
        "the decision is awaited"
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
    assert_eq!(
        f.endpoint.submissions(),
        submitted + 1,
        "the Try applied once, and nothing stopped the core yet"
    );

    release.notify_one();
    assert!(matches!(
        undecided.await.unwrap(),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&f.client, operation_id).await.conclusion,
        MutationConclusion::Confirmed,
        "the shutdown does not destroy an undecided transaction"
    );
    assert!(stopped_within(&f.tasks, SETTLE).await);
    assert_eq!(f.endpoint.submissions(), submitted + 2, "then the stop");
}

/// X4 (V16): a Cancel runs to its real completion. Its restore is held at the
/// core while the shutdown waits, and the baseline is back before the core
/// stops.
#[tokio::test]
async fn the_shutdown_during_a_cancel_waits_for_the_restore() {
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
        plain(),
        no_local_write,
    )
    .await;
    assert!(refused(&vetoed), "{vetoed:?}");
    scripted.held().await;

    request_shutdown(&f.shutdown, &f.tasks);
    assert!(
        !stopped_within(&f.tasks, CUT_SHORT).await,
        "the restore is awaited"
    );

    scripted.release();
    assert!(stopped_within(&f.tasks, SETTLE).await);
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
        "the restore finished before the core stopped"
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
