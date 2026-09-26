//! T4: a restore is verified against the receipt, not against the name the
//! lower control plane gave its own transaction result (v2 C4, V04).

use std::sync::Arc;

use super::workflow_graph_with_clients;
use crate::{
    client::{
        runtime,
        runtime_recovery::{
            ObservedRuntime, RecoveryMismatch, RecoveryVerification, verify_recovery_target,
        },
        tests::{IdleServiceAdapter, TestCheckAnswer, TestControlEndpoint},
    },
    core::actor_v2::{CoreClient, service_actor::ServiceClient},
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
    let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
        .await
        .unwrap();
    let (client, builder, _, _) = workflow_graph_with_clients(
        &dir,
        core.clone(),
        service,
        false,
        Arc::new(crate::client::SessionPortResolver::new(store.clone())),
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
        error.kind,
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
    client.shutdown().await.unwrap();
}

// -- T10 §1.11/§4: a live attempt and a write-ahead action -------------------
//
// Every test below isolates the execution domain the way it happens for real
// — a panic, a lost reply, an elapsed wait — and recovers it only through the
// explicit retry, reading what the two slots kept. The interruptions are
// scripted fakes and barriers; nothing here sleeps for an ordering.

use nyanpasu_core::state::{ReplaceIfVersionError, ReplaceIfVersionResult};
use nyanpasu_core_manager::{CoreErrorKind, OperationId};

use std::sync::atomic::Ordering;

use super::{
    WaitScript, barrier,
    mutations::{
        Fixture, app_with_core, fixture, mutate, mutate_with_hints, names_overrides, overrides,
        parked_local_write, plain, refused, scripted_fixture, settled, simple_mutate, test_budgets,
        unserviceable_check,
    },
    panic_at_confirm,
};
use crate::client::application_workflow::{
    ApplicationWorkflowClient, Command,
    attempt::{ActionView, AttemptOriginKind, AttemptStage, LifecycleCommand, RecoveryView},
    mutation::{DEFERRED_RETRY_BUDGET, MutationConclusion, MutationOutcomeKind},
    policy::CommandClass,
};

fn recovery(client: &ApplicationWorkflowClient) -> RecoveryView {
    client
        .mutation_journal()
        .recovery
        .expect("an isolated execution domain names its attempt")
}

/// Waits until the running attempt is back in the actor. A transaction can
/// finish before its attempt does — a panic right after Confirm is one — so
/// its own result says nothing about that.
async fn until_idle(client: &ApplicationWorkflowClient) {
    let mut status = client.0.status.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        status.wait_for(|status| status.active.is_none()),
    )
    .await
    .expect("the attempt should come back")
    .expect("the workflow should stay alive");
    barrier(client).await;
}

fn isolated(client: &ApplicationWorkflowClient) -> bool {
    client.status().uncertain
}

/// The one operation the pending action names.
fn pending_submission(client: &ApplicationWorkflowClient) -> (OperationId, bool) {
    match recovery(client).action {
        Some(ActionView::Submission {
            operation,
            accepted,
        }) => (operation, accepted),
        other => panic!("the pending action should be a submission, got {other:?}"),
    }
}

/// A baseline the next mutation's Cancel can put back, applied for real.
async fn prime(f: &mut Fixture) -> Arc<runtime::RuntimeApplyReceipt> {
    let (id, primed) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&f.client, id).await;
    f.store
        .last_confirmed_runtime_receipt()
        .expect("the primed apply is the baseline")
}

/// Makes the next source write fail, so the transaction aborts after its Try.
fn fail_the_next_save(path: &camino::Utf8Path) {
    let _ = std::fs::remove_file(path);
    std::fs::create_dir_all(path).unwrap();
}

/// L1: the Try panics. The attempt still names the mutation, with the
/// baseline it read, and the aborted decision sends recovery back to that
/// baseline — here visibly, because the runtime drifted meanwhile.
#[tokio::test]
async fn a_panic_during_the_try_keeps_the_mutation_and_restores_its_baseline() {
    let mut f = fixture(test_budgets()).await;
    let baseline = prime(&mut f).await;

    f.builder.panic.store(true, Ordering::SeqCst);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::State(_))),
        "{result:?}"
    );
    until_idle(&f.client).await;
    assert!(isolated(&f.client));
    let view = recovery(&f.client);
    assert_eq!(view.operation_id, id);
    assert_eq!(view.origin, AttemptOriginKind::SourceDecision);
    assert_eq!(view.stage, AttemptStage::TryingCritical);
    assert_eq!(view.action, None, "nothing was submitted before the panic");

    f.builder.panic.store(false, Ordering::SeqCst);
    f.endpoint.set_source_hash("drifted");
    let submitted = f.endpoint.reconciled_bytes();
    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    assert!(f.client.mutation_journal().recovery.is_none());
    let restored = f.endpoint.reconciled_bytes();
    assert_eq!(restored.len(), submitted.len() + 1);
    assert_eq!(
        restored.last(),
        Some(&baseline.config_text.as_bytes().to_vec()),
        "the aborted mutation's recorded baseline is what goes back"
    );
}

/// L4: the capture panics while the mutation is still preparing. It never
/// read the runtime, so an aborted decision settles it without touching the
/// runtime, however that looks now.
#[tokio::test]
async fn a_panic_while_preparing_settles_an_aborted_mutation_without_a_restore() {
    let mut f = fixture(test_budgets()).await;
    f.builder.panic_capture.store(true, Ordering::SeqCst);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::State(_))),
        "{result:?}"
    );
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(view.operation_id, id);
    assert_eq!(view.stage, AttemptStage::Preparing);

    f.builder.panic_capture.store(false, Ordering::SeqCst);
    f.endpoint.set_source_hash("drifted");
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "an attempt that never read the runtime has nothing to put back"
    );
}

/// L2: Confirm is interrupted, once for each verdict the Try can have
/// committed. Recovery reads the committed decision and finishes what Confirm
/// owed: verify the applied receipt, install the deferred target, or only
/// settle and notify.
#[tokio::test]
async fn a_panic_during_confirm_is_finished_by_the_verdict_it_committed() {
    // Applied: the receipt is running, so it is verified in place.
    let mut f = fixture(test_budgets()).await;
    f.notifications.panic_next.store(true, Ordering::SeqCst);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::Confirming)
    );
    let applied = f.store.last_confirmed_runtime_receipt().unwrap();
    let submitted = f.endpoint.reconciled_bytes().len();
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted);
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        applied.config_digest
    );

    // Deferred, interrupted before Confirm installed anything: recovery
    // installs the target, with a fresh budget of its own.
    let mut f = fixture(test_budgets()).await;
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    panic_at_confirm(&f.client).await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    assert_eq!(recovery(&f.client).stage, AttemptStage::Confirming);
    assert!(
        f.client.mutation_journal().deferred.is_none(),
        "Confirm was interrupted before it installed the target"
    );
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    let target = f.client.mutation_journal().deferred.expect("installed");
    assert_eq!(target.operation_id, id);
    assert_eq!(target.attempts_remaining, DEFERRED_RETRY_BUDGET);
    assert!(f.endpoint.reconciled_bytes().is_empty());

    // Saved: nothing was owed but the notification recovery now sends.
    let mut f = fixture(test_budgets()).await;
    let mut app = f.application.snapshot().as_ref().clone();
    app.language = nyanpasu_config::application::I18nLanguage::English;
    f.notifications.panic_next.store(true, Ordering::SeqCst);
    let (_, result) = simple_mutate(&mut f.application, &f.client, app, CommandClass::Save).await;
    assert!(result.is_ok(), "{result:?}");
    until_idle(&f.client).await;
    assert_eq!(recovery(&f.client).stage, AttemptStage::Confirming);
    let notified = f.notifications.committed();
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert_eq!(f.notifications.committed(), notified + 1);
}

/// L2, the budget half (review 3 #6): recovering a deferral whose identity
/// matches the outstanding target keeps that target's spent budget; only a
/// different identity opens a full one. Confirm is interrupted before it
/// installs anything, so the target recovery leaves is recovery's own.
#[tokio::test]
async fn a_recovered_deferral_keeps_the_budget_of_the_same_target() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_failure(Some("queue_full"));
    let target = overrides(serde_json::json!({"mode": "direct"}));
    let (_, result) = mutate_with_hints(
        &mut f.clash,
        &f.client,
        target.clone(),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    for _ in 0..DEFERRED_RETRY_BUDGET {
        f.client
            .call(Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
    }
    let spent = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(spent.attempts_remaining, 0);

    // The same target again, interrupted before its Confirm installed it.
    panic_at_confirm(&f.client).await;
    let (same, result) = mutate_with_hints(
        &mut f.clash,
        &f.client,
        target,
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    assert!(isolated(&f.client));
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().operation_id,
        spent.operation_id,
        "only the spent target is installed so far"
    );
    f.client.retry_runtime().await.unwrap();
    let recovered = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(recovered.operation_id, same, "recovery installed it");
    assert_eq!(
        recovered.attempts_remaining, 0,
        "an unchanged identity never refills a spent budget"
    );
    assert_eq!(recovered.attempts, spent.attempts);
    assert_eq!(recovered.waits, spent.waits);

    // A different target is a different gap.
    panic_at_confirm(&f.client).await;
    let (other, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().operation_id,
        same
    );
    f.client.retry_runtime().await.unwrap();
    let fresh = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(fresh.operation_id, other);
    assert_eq!(fresh.attempts_remaining, DEFERRED_RETRY_BUDGET);
    assert_eq!(fresh.attempts, 0);
}

/// Minor 4: a committed `SavedInactive` save supersedes the outstanding target
/// the way Confirm drops it. Interrupted before Confirm, recovery has to drop
/// it too, or a stale target outlives the save that replaced it.
#[tokio::test]
async fn a_recovered_saved_inactive_commit_drops_the_outstanding_target() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_failure(Some("queue_full"));
    let (_, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert!(f.client.mutation_journal().deferred.is_some());
    f.endpoint.set_failure(None);
    f.client.stop_core().await.unwrap();

    panic_at_confirm(&f.client).await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    assert_eq!(recovery(&f.client).operation_id, id);
    assert!(
        f.client.mutation_journal().deferred.is_some(),
        "Confirm was interrupted before it dropped the target"
    );
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert!(f.client.mutation_journal().deferred.is_none());
}

/// S10b: five saves of one committed target after its automatic budget is
/// spent. Each is re-evaluated, none refills the budget, and only a new
/// identity does.
#[tokio::test]
async fn saving_an_unchanged_target_never_refills_its_spent_budget() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_failure(Some("queue_full"));
    let target = overrides(serde_json::json!({"mode": "direct"}));
    let (_, result) = mutate_with_hints(
        &mut f.clash,
        &f.client,
        target.clone(),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    for _ in 0..DEFERRED_RETRY_BUDGET {
        f.client
            .call(Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
    }
    for _ in 0..5 {
        let (id, result) = mutate_with_hints(
            &mut f.clash,
            &f.client,
            target.clone(),
            CommandClass::Save,
            names_overrides(),
        )
        .await;
        assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
        assert_eq!(
            settled(&f.client, id).await.outcome,
            MutationOutcomeKind::Deferred
        );
        assert_eq!(
            f.client
                .mutation_journal()
                .deferred
                .unwrap()
                .attempts_remaining,
            0
        );
    }
    let (_, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        f.client
            .mutation_journal()
            .deferred
            .unwrap()
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET
    );
}

/// L3: the Cancel's restore panics while waiting for its own submission. The
/// slot holds that restore, not the Try before it; once it is seen to finish,
/// recovery puts the baseline back and proves it.
#[tokio::test]
async fn a_panic_during_cancel_keeps_the_restore_and_recovers_the_baseline() {
    let mut f = scripted_fixture(test_budgets()).await;
    let baseline = prime(&mut f).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let id = OperationId::generate();
    let mutation = {
        let client = f.client.clone();
        let (entered, release) = (entered.clone(), release.clone());
        let mut clash = f.clash;
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                id,
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
                std::time::Duration::from_secs(10),
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };
    entered.notified().await;
    let scripted = f.scripted.clone().expect("a scripted fixture");
    let tried = *scripted.operations().last().expect("the Try was submitted");
    fail_the_next_save(&f.clash_path);
    scripted.queue(WaitScript::Panic);
    release.notify_one();
    let (_, result) = mutation.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );
    until_idle(&f.client).await;

    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::Cancelling)
    );
    let (restore, accepted) = pending_submission(&f.client);
    assert!(accepted);
    assert_ne!(restore, tried, "the slot holds the restore, not the Try");
    scripted.rescript(restore, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest
    );
    assert_eq!(
        f.endpoint.reconciled_bytes().last(),
        Some(&baseline.config_text.as_bytes().to_vec())
    );
}

/// L5 (N1): the submission was accepted and its waiter panicked while the
/// operation runs on. The slot holds that accepted submission; a retry while
/// it runs changes nothing, and only its terminal answer lets the mutation's
/// recovery continue.
#[tokio::test]
async fn an_accepted_submission_whose_waiter_panicked_is_waited_out_before_recovery() {
    let mut f = scripted_fixture(test_budgets()).await;
    let baseline = prime(&mut f).await;
    let scripted = f.scripted.clone().expect("a scripted fixture");

    scripted.queue(WaitScript::Panic);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::State(_))),
        "{result:?}"
    );
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::TryingCritical)
    );
    let (tried, accepted) = pending_submission(&f.client);
    assert!(accepted, "the ticket arrived before the waiter panicked");

    scripted.rescript(tried, WaitScript::Running);
    let submitted = scripted.submitted();
    let error = f.client.retry_runtime().await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
    assert!(isolated(&f.client));
    assert_eq!(pending_submission(&f.client).0, tried);
    assert_eq!(scripted.submitted(), submitted, "nothing is resent blind");

    scripted.rescript(tried, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest,
        "the aborted mutation's candidate was applied, and recovery took it back"
    );
}

/// L6 (N2): the first action is resolved and the restore recovery submits in
/// its place loses its receipt. The slot now names the restore; the next retry
/// asks only about it and resends nothing.
#[tokio::test]
async fn a_lost_second_action_replaces_the_resolved_one_and_is_never_resent() {
    let mut f = scripted_fixture(test_budgets()).await;
    let baseline = prime(&mut f).await;
    let scripted = f.scripted.clone().expect("a scripted fixture");

    scripted.queue(WaitScript::Missing);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(refused(&result), "{result:?}");
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::RecoveryRequired
    );
    let (tried, _) = pending_submission(&f.client);

    scripted.rescript(tried, WaitScript::Deliver);
    scripted.queue(WaitScript::Missing);
    assert!(f.client.retry_runtime().await.is_err());
    let (restore, accepted) = pending_submission(&f.client);
    assert_ne!(restore, tried, "the resolved action was consumed");
    assert!(accepted);
    assert_eq!(recovery(&f.client).stage, AttemptStage::Recovering);

    let submitted = scripted.submitted();
    assert!(f.client.retry_runtime().await.is_err());
    assert_eq!(pending_submission(&f.client).0, restore);
    assert_eq!(
        scripted.submitted(),
        submitted,
        "the lost restore is not resent"
    );

    scripted.rescript(restore, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest
    );
}

/// L7 (N2): recovery panics while its own restore is in flight. The slot holds
/// that restore, not the action recovery already resolved, and the attempt
/// says it was recovering.
#[tokio::test]
async fn a_panic_during_the_recovery_action_leaves_that_action_in_the_slot() {
    let mut f = scripted_fixture(test_budgets()).await;
    prime(&mut f).await;
    let scripted = f.scripted.clone().expect("a scripted fixture");

    scripted.queue(WaitScript::Missing);
    let (_, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(refused(&result), "{result:?}");
    until_idle(&f.client).await;
    let (tried, _) = pending_submission(&f.client);

    scripted.rescript(tried, WaitScript::Deliver);
    scripted.queue(WaitScript::Panic);
    assert!(f.client.retry_runtime().await.is_err());
    let (restore, accepted) = pending_submission(&f.client);
    assert_ne!(restore, tried);
    assert!(accepted);
    assert_eq!(recovery(&f.client).stage, AttemptStage::Recovering);

    scripted.rescript(restore, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
}

/// L9 (N4): an automatic retry of a committed target loses its receipt. It is
/// not an unconfirmed source transaction: once the lost operation is seen to
/// finish and the runtime is still on the baseline the retry found, the
/// target goes back as it was — charged once, with its health and schedule
/// untouched — and a later explicit retry applies it.
#[tokio::test]
async fn a_committed_target_retry_that_lost_its_receipt_goes_back_charged_once() {
    let mut f = scripted_fixture(test_budgets()).await;
    let scripted = f.scripted.clone().expect("a scripted fixture");
    f.endpoint.set_failure(Some("queue_full"));
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&f.client, id).await.outcome,
        MutationOutcomeKind::Deferred
    );
    let scheduled = f.client.mutation_journal().deferred.unwrap();

    scripted.queue(WaitScript::Missing);
    f.client
        .call(Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    assert!(isolated(&f.client));
    let view = recovery(&f.client);
    assert_eq!(view.origin, AttemptOriginKind::CommittedTarget);
    let (retried, _) = pending_submission(&f.client);

    scripted.rescript(retried, WaitScript::Deliver);
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    let target = f.client.mutation_journal().deferred.expect("put back");
    assert_eq!(target.operation_id, id);
    assert_eq!(target.attempts_remaining, DEFERRED_RETRY_BUDGET - 1);
    assert_eq!(target.attempts, 1);
    assert_eq!(target.health, scheduled.health);
    assert_eq!(target.next_attempt, scheduled.next_attempt);

    f.endpoint.set_failure(None);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.mutation_journal().deferred.is_none());
    assert!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_text
            .contains("mode: direct")
    );
}

/// L12: an automatic retry never recovers an isolated domain, even once the
/// evidence to do so exists; only the explicit one consumes the action.
#[tokio::test]
async fn an_automatic_retry_never_recovers() {
    let f = fixture(test_budgets()).await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.reconcile().await.is_err());
    f.endpoint.set_result_missing(false);

    let refused = f
        .client
        .call(Command::RetryRuntime { explicit: false })
        .await
        .err()
        .map(|error| error.kind);
    assert_eq!(refused, Some(Some(CoreErrorKind::OperationConflict)));
    assert!(isolated(&f.client));
    assert!(recovery(&f.client).action.is_some());

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
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.reconcile().await.is_err());
    let view = recovery(&f.client);
    assert_eq!(
        view.origin,
        AttemptOriginKind::Lifecycle {
            command: LifecycleCommand::Reconcile
        }
    );
    assert!(matches!(
        view.action,
        Some(ActionView::Submission { accepted: true, .. })
    ));
    assert!(f.client.retry_runtime().await.is_err());
    assert!(recovery(&f.client).action.is_some(), "still unobserved");

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
            .config_text
            .as_bytes(),
        f.endpoint.reconciled_bytes().last().unwrap().as_slice()
    );

    let (_, admitted) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(nyanpasu_config::application::ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(admitted.is_ok(), "{admitted:?}");
}

/// L13 (review 3 #1): the Try succeeded and nothing is pending, but the
/// decision did not arrive within its budget. The attempt stays and keeps the
/// domain isolated; once the decision exists, recovery settles it by that
/// decision, and finishes what Confirm would have owed the commit: the older
/// blocked target it superseded goes, and its product is published.
///
/// Every mutation gets the ordinary decision budget; only the one under test
/// outlives it, on a clock advanced past that budget once its write is
/// parked, so no real window has to be met or missed.
#[tokio::test]
async fn an_elapsed_decision_wait_keeps_the_attempt_until_the_decision_is_read() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_failure(Some("queue_full"));
    let (_, older) = mutate_with_hints(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(older, Ok(ReplaceIfVersionResult::Replaced)));
    for _ in 0..DEFERRED_RETRY_BUDGET {
        f.client
            .call(Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
    }
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().health,
        crate::client::convergence::ConvergenceHealth::Blocked
    );
    f.endpoint.set_failure(None);
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let id = OperationId::generate();
    let mutation = {
        let client = f.client.clone();
        let (entered, release) = (entered.clone(), release.clone());
        let mut clash = f.clash;
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                std::time::Duration::from_secs(10),
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };
    entered.notified().await;
    // The decision wait started before the write was parked, so a paused
    // clock moved past its budget elapses it and nothing else that matters.
    tokio::time::pause();
    tokio::time::advance(test_budgets().decision_wait + std::time::Duration::from_secs(1)).await;
    tokio::time::resume();
    let receipt = settled(&f.client, id).await;
    assert_eq!(receipt.conclusion, MutationConclusion::RecoveryRequired);
    assert!(isolated(&f.client));
    let view = recovery(&f.client);
    assert_eq!(
        (view.stage, view.action),
        (AttemptStage::AwaitDecision, None)
    );
    assert!(f.client.retry_runtime().await.is_err(), "still undecided");

    let (_, refused_result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(nyanpasu_config::application::ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&refused_result), "{refused_result:?}");

    release.notify_one();
    let (clash, result) = mutation.await.unwrap();
    f.clash = clash;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_ne!(promoted_mode(&f), Some("global".into()));
    assert!(f.client.mutation_journal().deferred.is_some());
    let submitted = f.endpoint.reconciled_bytes().len();
    f.client.retry_runtime().await.unwrap();
    assert!(!isolated(&f.client));
    assert_eq!(
        f.endpoint.reconciled_bytes().len(),
        submitted,
        "the committed receipt is running, so it is verified, not resubmitted"
    );
    assert!(
        f.client.mutation_journal().deferred.is_none(),
        "the applied commit superseded the older target"
    );
    assert_eq!(promoted_mode(&f), Some("global".into()));
    assert!(f.client.mutation_journal().maintenance.is_none());
}

/// The mode of the runtime product last published.
fn promoted_mode(f: &Fixture) -> Option<String> {
    f.store
        .read()
        .promoted
        .and_then(|product| product.config["mode"].as_str().map(str::to_owned))
}

/// A publication that fails while recovery finishes a committed apply is not
/// dropped: it stays the retryable maintenance item a failed Confirm leaves,
/// and a later explicit retry publishes it without resubmitting anything.
#[tokio::test]
async fn a_publication_that_fails_during_recovery_is_retried_later() {
    let mut f = fixture(test_budgets()).await;
    panic_at_confirm(&f.client).await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::Confirming)
    );
    let submitted = f.endpoint.reconciled_bytes().len();
    f.builder.fail_publish.store(true, Ordering::SeqCst);

    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    let maintenance = f.client.mutation_journal().maintenance;
    assert!(
        maintenance
            .as_deref()
            .is_some_and(|item| item.contains("runtime_product_publish_failed")),
        "{maintenance:?}"
    );
    assert_ne!(promoted_mode(&f), Some("global".into()));

    f.builder.fail_publish.store(false, Ordering::SeqCst);
    f.client.retry_runtime().await.unwrap();

    assert!(f.client.mutation_journal().maintenance.is_none());
    assert_eq!(promoted_mode(&f), Some("global".into()));
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted);
}

/// A committed apply whose runtime drifted while the domain was isolated is
/// restored before recovery finishes what Confirm owed. The restore lands on
/// a new instance, and the confirmed receipt, the published product and its
/// inspection all name that instance; the older blocked target the commit
/// superseded goes.
#[tokio::test]
async fn a_drifted_apply_is_restored_before_its_confirm_is_finished() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_effective_enabled(true);
    f.endpoint.set_failure(Some("queue_full"));
    let (_, older) = mutate_with_hints(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(older, Ok(ReplaceIfVersionResult::Replaced)));
    for _ in 0..DEFERRED_RETRY_BUDGET {
        f.client
            .call(Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
    }
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().health,
        crate::client::convergence::ConvergenceHealth::Blocked
    );
    f.endpoint.set_failure(None);
    panic_at_confirm(&f.client).await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::Confirming)
    );
    let applied = f.store.last_confirmed_runtime_receipt().unwrap();
    assert!(f.client.mutation_journal().deferred.is_some());
    assert_ne!(promoted_mode(&f), Some("global".into()));

    f.endpoint.set_source_hash("drifted");
    let submitted = f.endpoint.reconciled_bytes().len();
    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    let reconciled = f.endpoint.reconciled_bytes();
    assert_eq!(
        (reconciled.len(), reconciled.last()),
        (
            submitted + 1,
            Some(&applied.config_text.as_bytes().to_vec())
        ),
        "the committed receipt is resubmitted, since the core left it"
    );
    let restored = f.store.last_confirmed_runtime_receipt().unwrap();
    assert_eq!(restored.config_digest, applied.config_digest);
    assert_ne!(
        restored.binding, applied.binding,
        "the restore lands on a new instance"
    );
    let runtime = f.store.read();
    assert!(runtime.pending.is_none());
    let inspected = runtime.applied.expect("the restored instance is inspected");
    assert_eq!(inspected.applied_binding.as_ref(), Some(&restored.binding));
    let promoted = runtime
        .promoted
        .expect("the committed product is published");
    assert_eq!(promoted_mode(&f), Some("global".into()));
    assert_eq!(
        promoted.applied_binding.as_ref(),
        Some(&restored.binding),
        "the product names the instance that is running it"
    );
    assert!(promoted.effective.is_some(), "and carries its inspection");
    assert!(
        f.client.mutation_journal().deferred.is_none(),
        "the applied commit superseded the older target"
    );
    assert!(f.client.mutation_journal().maintenance.is_none());
}

/// L14 (review 3 #1): an aborted decision that still owes a local resource
/// recovery settles nothing, and neither does an empty action slot.
#[tokio::test]
async fn an_abort_that_needs_recovery_stays_isolated_with_nothing_pending() {
    let f = fixture(test_budgets()).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let abandoned = OperationId::generate();
    let mut clash = f.clash;
    let version = clash.snapshot_handle().load().version;
    let workflow = f.client.clone();
    {
        let writing = entered.clone();
        let released = release.clone();
        let mut pending = Box::pin(clash.replace_if_version_with_participant(
            version,
            overrides(serde_json::json!({"mode": "global"})),
            move |decision| {
                super::super::participant::ApplicationMutationParticipant::new(
                    abandoned,
                    super::super::impact::MutationHints::default(),
                    CommandClass::Save,
                    decision,
                    workflow,
                )
            },
            move || async move {
                writing.notify_one();
                released.notified().await;
                Ok(())
            },
            || async { Err(anyhow::anyhow!("resource recovery failed")) },
        ));
        tokio::select! {
            result = &mut pending => panic!("write should stay pending: {result:?}"),
            _ = entered.notified() => {}
        }
        drop(pending);
    }
    release.notify_one();
    settled(&f.client, abandoned).await;
    assert!(isolated(&f.client));
    assert_eq!(recovery(&f.client).action, None);

    let error = f.client.retry_runtime().await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
    assert!(isolated(&f.client));
    assert_eq!(recovery(&f.client).operation_id, abandoned);
}

/// The ordinary daemon, except that it can be made to refuse its stop.
struct RefusingStop {
    delegate: crate::client::tests::HostTransitionServiceAdapter,
    refuse: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl crate::core::actor_v2::service_actor::ServiceHostAdapter for RefusingStop {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, String> {
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), String> {
        self.delegate.install().await
    }
    async fn uninstall(&self) -> Result<(), String> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), String> {
        self.delegate.start_daemon().await
    }
    async fn stop_daemon(&self) -> Result<(), String> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err("scripted: the daemon refused to stop".into());
        }
        self.delegate.stop_daemon().await
    }
    async fn update(&self) -> Result<(), String> {
        self.delegate.update().await
    }
    fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
        self.delegate.endpoint()
    }
}

/// Releasing the daemon is part of what Confirm owes a commit that leaves
/// service mode. A release that fails while recovery finishes such a commit
/// is kept the way a failed publication is: maintenance names it, and a
/// later explicit retry releases the daemon.
#[tokio::test]
async fn a_daemon_release_that_fails_during_recovery_is_retried_later() {
    use crate::core::actor_v2::endpoint::ExecutionHost;
    let daemon = Arc::new(RefusingStop {
        delegate: crate::client::tests::HostTransitionServiceAdapter {
            endpoint: TestControlEndpoint::succeeding_on(ExecutionHost::Service),
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            stopped: std::sync::atomic::AtomicBool::new(false),
        },
        refuse: std::sync::atomic::AtomicBool::new(true),
    });
    let mut f =
        super::mutations::fixture_with_daemon(test_budgets(), true, Some(daemon.clone())).await;
    prime(&mut f).await;
    let mut app = f.application.snapshot().as_ref().clone();
    app.enable_service_mode = true;
    let (id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app.clone(),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert_eq!(f.client.core_status().host, ExecutionHost::Service);

    panic_at_confirm(&f.client).await;
    app.enable_service_mode = false;
    let (id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app,
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    until_idle(&f.client).await;
    let view = recovery(&f.client);
    assert_eq!(
        (view.operation_id, view.stage),
        (id, AttemptStage::Confirming)
    );
    assert_eq!(f.client.core_status().host, ExecutionHost::Local);

    f.client.retry_runtime().await.unwrap();

    assert!(!isolated(&f.client));
    let maintenance = f.client.mutation_journal().maintenance;
    assert!(
        maintenance
            .as_deref()
            .is_some_and(|item| item.contains("service_stop_failed")),
        "{maintenance:?}"
    );
    assert!(!daemon.delegate.stopped.load(Ordering::SeqCst));

    daemon.refuse.store(false, Ordering::SeqCst);
    f.client.retry_runtime().await.unwrap();

    assert!(f.client.mutation_journal().maintenance.is_none());
    assert!(
        daemon.delegate.stopped.load(Ordering::SeqCst),
        "the retry released the daemon"
    );
}

/// A service daemon whose install is held at a barrier past the actor's
/// bound, while its probe keeps answering `Ready`.
struct ParkedInstall {
    delegate: crate::client::tests::HostTransitionServiceAdapter,
    parked: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::core::actor_v2::service_actor::ServiceHostAdapter for ParkedInstall {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, String> {
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), String> {
        self.parked.notify_one();
        self.release.notified().await;
        self.delegate.install().await
    }
    async fn uninstall(&self) -> Result<(), String> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), String> {
        self.delegate.start_daemon().await
    }
    async fn stop_daemon(&self) -> Result<(), String> {
        self.delegate.stop_daemon().await
    }
    async fn update(&self) -> Result<(), String> {
        self.delegate.update().await
    }
    fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
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
        delegate: crate::client::tests::HostTransitionServiceAdapter {
            endpoint: TestControlEndpoint::succeeding_on(
                crate::core::actor_v2::endpoint::ExecutionHost::Service,
            ),
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            stopped: std::sync::atomic::AtomicBool::new(false),
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
    let service =
        ServiceClient::spawn_bounded(daemon.clone(), 0, std::time::Duration::from_millis(50))
            .await
            .unwrap();
    let (client, builder, _, _) = workflow_graph_with_clients(
        &dir,
        core,
        service.clone(),
        false,
        Arc::new(crate::client::SessionPortResolver::default()),
    )
    .await;
    builder.release.notify_one();

    let error = client.install_service().await.unwrap_err();
    assert!(error.message.contains("still running"), "{}", error.message);
    daemon.parked.notified().await;
    assert_eq!(
        service.probe().await.unwrap().phase,
        crate::core::actor_v2::service_actor::ServicePhase::Ready,
        "the daemon probes a stable Ready while the helper runs"
    );
    assert!(client.status().uncertain);
    let view = recovery(&client);
    assert_eq!(
        view.origin,
        AttemptOriginKind::Lifecycle {
            command: LifecycleCommand::InstallService
        }
    );
    assert_eq!(
        view.action,
        Some(ActionView::ServiceCommand {
            command: crate::core::actor_v2::service_actor::ServiceCommandKind::Install
        })
    );
    assert_eq!(
        client.start_service().await.unwrap_err().kind,
        Some(CoreErrorKind::OperationConflict)
    );
    assert!(client.retry_runtime().await.is_err());
    assert!(recovery(&client).action.is_some(), "the helper still runs");

    assert!(local.reconciled_bytes().is_empty());

    daemon.release.notify_one();
    let resolved = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let _ = client.retry_runtime().await;
            if client
                .mutation_journal()
                .recovery
                .is_none_or(|view| view.action.is_none())
            {
                break;
            }
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
    assert_eq!(
        recovery(&g.client).action,
        Some(ActionView::Handoff {
            target: crate::core::actor_v2::endpoint::ExecutionHost::Local
        })
    );
    assert_eq!(
        g.client.retry_runtime().await.unwrap_err().kind,
        Some(CoreErrorKind::OperationConflict),
        "the router is still handing off"
    );
    assert!(isolated(&g.client));
    assert!(recovery(&g.client).action.is_some());
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
    use crate::core::actor_v2::{EndpointConnectivity, endpoint::ExecutionHost};
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
        crate::client::convergence::ConvergenceHealth::RecoveryRequired
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
    let f = fixture(test_budgets()).await;
    f.endpoint.set_result_missing(true);
    assert!(f.client.stop_core().await.is_err());
    assert_eq!(
        recovery(&f.client).origin,
        AttemptOriginKind::Lifecycle {
            command: LifecycleCommand::StopCore
        }
    );
    f.endpoint.set_result_missing(false);
    f.endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    f.endpoint.set_failure(Some("apply_failed"));
    let submitted = f.endpoint.reconciled_bytes().len();

    let error = f.client.retry_runtime().await.unwrap_err();

    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
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
        crate::core::actor_v2::endpoint::ControlEndpoint::status(f.endpoint.as_ref())
            .await
            .unwrap()
            .state,
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
    ));
}
