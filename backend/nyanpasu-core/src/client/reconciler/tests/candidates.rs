//! A failure after a change built its candidate keeps it and names it: a
//! check that refuses it, an application the core refuses, a wait for the
//! core and a publication that fails.

use std::sync::{Arc, atomic::Ordering};

use nyanpasu_config::{
    application::ClashCore, clash::config::dns::ControlSource, runtime::executor::StepLogLevel,
};
use nyanpasu_core_manager::{CoreError, CoreErrorKind};

use super::mutations::{
    Fixture, SourceOwner, app_with_core, commit_source, fixture, overrides, settled,
};
use crate::{
    client::{
        configuration_status::runtime_convergence,
        reconciler::{Conclusion, Message, RuntimeReconcilerClient},
        runtime_inspection::CandidateStage,
        test_support::TestCheckAnswer,
    },
    state::config_error::ConfigError,
};
use nyanpasu_runtime_values::{
    candidate::CandidateSnapshotId, mutation::CommitAborted, runtime::DegradationReason,
    runtime_error::RuntimeError,
};

async fn tick(client: &RuntimeReconcilerClient) {
    client.0.actor.cast(Message::Tick).unwrap();
    super::barrier(client).await;
}

/// The candidate the current conclusion names.
fn named(client: &RuntimeReconcilerClient) -> Option<CandidateSnapshotId> {
    match client.reconciler_status().conclusion {
        Conclusion::NotInEffect { cause }
        | Conclusion::Waiting { cause }
        | Conclusion::Unknown { cause } => cause.candidate_snapshot_id().cloned(),
        _ => None,
    }
}

/// Asserts the store holds exactly `ids`, in any order.
fn assert_held(f: &Fixture, ids: &[&CandidateSnapshotId]) {
    let held = f.candidates.pinned();
    assert_eq!(held.len(), ids.len(), "{held:?}");
    for id in ids {
        assert!(held.contains(id), "{id:?} is not held: {held:?}");
    }
}

/// Refuses every check from now on.
fn reject_checks(f: &Fixture) {
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "config check failed\n",
            false,
        )));
}

async fn mode_and_stage(
    f: &Fixture,
    id: &CandidateSnapshotId,
) -> (serde_json::Value, CandidateStage) {
    f.candidates
        .inspect(id, |archive, stage| {
            Ok((
                archive.graph.restore(archive.final_node).unwrap()["mode"].clone(),
                stage,
            ))
        })
        .await
        .unwrap()
}

/// The single runtime error a check refused or failed the commit with.
async fn refusal<S: SourceOwner>(owner: &S, next: S::Value) -> Arc<RuntimeError> {
    let Err(error) = commit_source(owner, next).await else {
        panic!("expected the check to refuse the commit");
    };
    let ConfigError::Commit {
        source:
            CommitAborted::RuntimeRefused { errors, .. } | CommitAborted::RuntimeFailed { errors, .. },
        ..
    } = error
    else {
        panic!("expected a refused commit, got {error:?}");
    };
    assert_eq!(errors.len(), 1);
    errors[0].clone()
}

#[tokio::test]
async fn a_rejected_check_keeps_the_exact_candidate_it_checked() {
    let f = fixture().await;
    let source = commit_source(&f.application, app_with_core(ClashCore::ClashRs))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Converged
    ));
    let store = f.client.snapshot_store();
    let promoted = store.read().promoted.unwrap();
    let submissions = f.endpoint.submissions();
    let version = f.clash.snapshot_handle().load().version;
    let message = "config check failed: bad group\n";
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            message,
            false,
        )));
    // clash-rs does not read `dns.prefer-h3`, so the build leaves it out with a warning.
    let mut next = overrides(serde_json::json!({"mode": "global"}));
    next.dns_control = ControlSource::Nyanpasu;
    next.dns.prefer_h3 = true;
    let error = refusal(&f.clash, next).await;

    let RuntimeError::CoreRejectedConfig {
        message: returned, ..
    } = error.as_ref()
    else {
        panic!("expected a check rejection: {error:?}");
    };
    assert_eq!(returned, message);
    let id = error
        .candidate_snapshot_id()
        .expect("the candidate is kept");
    let (final_config, warned) = f
        .candidates
        .inspect(id, |archive, _| {
            assert!(archive.halted.is_none());
            Ok((
                archive.graph.restore(archive.final_node).unwrap(),
                archive
                    .step_logs
                    .iter()
                    .flat_map(|log| &log.entries)
                    .any(|entry| entry.level == StepLogLevel::Warn),
            ))
        })
        .await
        .unwrap();
    assert_eq!(final_config["mode"], "global");
    assert!(final_config["dns"].get("prefer-h3").is_none());
    assert!(warned);
    let checked: serde_json::Value =
        yaml_serde::from_slice(&f.endpoint.checked().last().unwrap().config_bytes).unwrap();
    assert_eq!(checked, final_config);
    // Keeping it changed nothing the refusal protects.
    assert_eq!(f.endpoint.submissions(), submissions);
    assert_eq!(f.clash.snapshot_handle().load().version, version);
    assert!(Arc::ptr_eq(&store.read().promoted.unwrap(), &promoted));
}

#[tokio::test]
async fn an_unavailable_check_keeps_its_candidate_as_unchecked() {
    let f = fixture().await;
    f.endpoint.set_check_answer(TestCheckAnswer::Unsupported);
    let error = refusal(&f.clash, overrides(serde_json::json!({"mode": "global"}))).await;
    assert!(
        matches!(error.as_ref(), RuntimeError::CheckUnavailable { .. }),
        "{error:?}"
    );
    let id = error
        .candidate_snapshot_id()
        .expect("the candidate is kept");
    assert!(f.candidates.inspect(id, |_, _| Ok(())).await.is_ok());
}

#[tokio::test]
async fn a_refusal_whose_candidate_cannot_be_kept_is_reported_unchanged() {
    let f = fixture().await;
    f.candidates.close();
    let message = "opaque original refusal\n";
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            message,
            false,
        )));
    let error = refusal(&f.clash, overrides(serde_json::json!({"mode": "global"}))).await;
    let RuntimeError::CoreRejectedConfig {
        message: returned,
        candidate_snapshot_id: None,
        ..
    } = error.as_ref()
    else {
        panic!("expected the original rejection without an id: {error:?}");
    };
    assert_eq!(returned, message);
    assert!(serde_json::to_value(error.as_ref()).unwrap()["candidate_snapshot_id"].is_null());
}

#[tokio::test]
async fn a_passing_check_keeps_nothing() {
    let f = fixture().await;
    let source = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Converged
    ));
    assert!(f.candidates.kept().is_empty());
}

#[tokio::test]
async fn a_refused_application_keeps_its_candidate_pinned_while_it_is_reported() {
    let f = fixture().await;
    f.endpoint.set_failure(Some("apply_failed"));
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "NotInEffect"
    );
    let id = named(&f.client).expect("the refused attempt names its candidate");
    assert_held(&f, &[&id]);
    assert_eq!(
        mode_and_stage(&f, &id).await,
        (serde_json::json!("global"), CandidateStage::Built)
    );
    // The check passed and kept nothing.
    assert!(f.candidates.kept().is_empty());

    // Later rounds report the same attempt without keeping it again.
    tick(&f.client).await;
    assert_eq!(
        f.client.reconciler_status().conclusion.label(),
        "NotInEffect"
    );
    assert_eq!(named(&f.client), Some(id.clone()));
    assert_held(&f, &[&id]);
    assert!(f.candidates.kept().is_empty());
}

#[tokio::test]
async fn a_round_in_progress_does_not_report_the_failure_before_it() {
    let f = fixture().await;
    f.endpoint.set_failure(Some("apply_failed"));
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    settled(&f.client, version).await;
    assert!(named(&f.client).is_some());

    f.builder.park.store(true, Ordering::SeqCst);
    let client = f.client.clone();
    let retry = tokio::spawn(async move { client.retry_runtime().await });
    f.builder.entered.notified().await;
    let runtime = runtime_convergence(&f.client.reconciler_status());
    assert_eq!(
        runtime.health,
        nyanpasu_runtime_values::configuration::ConvergenceHealth::Pending
    );
    assert_eq!(runtime.message, None);
    assert_eq!(runtime.candidate_snapshot_id, None);
    f.builder.park.store(false, Ordering::SeqCst);
    f.builder.release.notify_one();
    let _ = retry.await.unwrap();
}

/// Lets the next build through only after the core stops answering status
/// reads, so the round sees a stable core and its submission does not.
async fn retry_into_an_unanswering_core(f: &Fixture) {
    f.builder.park.store(true, Ordering::SeqCst);
    let client = f.client.clone();
    let retry = tokio::spawn(async move { client.retry_runtime().await });
    f.builder.entered.notified().await;
    f.builder.park.store(false, Ordering::SeqCst);
    f.endpoint.set_status_fails(true);
    f.builder.release.notify_one();
    let _ = retry.await.unwrap();
    f.endpoint.set_status_fails(false);
}

#[tokio::test]
async fn a_target_waiting_for_the_core_keeps_its_candidate_the_first_time_only() {
    let f = fixture().await;
    f.endpoint.set_failure(Some("apply_failed"));
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    settled(&f.client, version).await;
    let refused = named(&f.client).unwrap();
    f.endpoint.set_failure(None);

    retry_into_an_unanswering_core(&f).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Waiting");
    let waiting = named(&f.client).expect("a wait with a built candidate names it");
    assert_ne!(waiting, refused);
    // The refused attempt, which a later round can replay, stays held
    // beside the wait.
    assert_held(&f, &[&refused, &waiting]);
    assert!(f.candidates.kept().is_empty());
    assert_eq!(mode_and_stage(&f, &waiting).await.0, "global");

    // The same target waiting again names the first candidate.
    retry_into_an_unanswering_core(&f).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Waiting");
    assert_eq!(named(&f.client), Some(waiting.clone()));
    assert_held(&f, &[&refused, &waiting]);
    assert!(f.candidates.kept().is_empty());
}

#[tokio::test]
async fn a_wait_for_the_core_names_the_candidate_its_change_passed_the_check_with() {
    let f = fixture().await;
    f.endpoint.set_status_fails(true);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Waiting"
    );
    let id = named(&f.client).expect("the wait names the checked candidate");
    assert_held(&f, &[&id]);
    assert_eq!(
        mode_and_stage(&f, &id).await,
        (serde_json::json!("global"), CandidateStage::Built)
    );
    let runtime = runtime_convergence(&f.client.reconciler_status());
    assert_eq!(runtime.candidate_snapshot_id, Some(id.clone()));

    // Later rounds of the same wait name it without keeping another.
    tick(&f.client).await;
    assert_eq!(named(&f.client), Some(id));
    assert!(f.candidates.kept().is_empty());
}

#[tokio::test]
async fn a_wait_with_nothing_checked_names_none() {
    let f = fixture().await;
    f.endpoint.set_status_fails(true);
    tick(&f.client).await;
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Waiting");
    assert_eq!(named(&f.client), None);
    assert!(f.candidates.pinned().is_empty());
    assert!(f.candidates.kept().is_empty());
}

#[tokio::test]
async fn a_replayed_refusal_stays_open_after_a_wait_and_many_other_failures() {
    let f = fixture().await;
    f.endpoint.set_failure(Some("apply_failed"));
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    settled(&f.client, version).await;
    let refused = named(&f.client).unwrap();
    retry_into_an_unanswering_core(&f).await;
    let waiting = named(&f.client).unwrap();
    assert_ne!(waiting, refused);

    // Other refusals fill the recent ones before any round reports the
    // refused attempt again; it stays open, and so does the wait.
    reject_checks(&f);
    for _ in 0..10 {
        refusal(&f.clash, overrides(serde_json::json!({"mode": "direct"}))).await;
    }
    f.endpoint.set_check_answer(TestCheckAnswer::Pass);

    // An ordinary round replays the refused attempt.
    tick(&f.client).await;
    assert_eq!(
        f.client.reconciler_status().conclusion.label(),
        "NotInEffect"
    );
    let reported = runtime_convergence(&f.client.reconciler_status())
        .candidate_snapshot_id
        .unwrap();
    assert_eq!(reported, refused);
    assert_eq!(mode_and_stage(&f, &reported).await.0, "global");

    // Another forced retry that waits names the wait's candidate, which is
    // still open.
    retry_into_an_unanswering_core(&f).await;
    let named_again = runtime_convergence(&f.client.reconciler_status())
        .candidate_snapshot_id
        .unwrap();
    assert_eq!(named_again, waiting);
    assert_eq!(mode_and_stage(&f, &named_again).await.0, "global");
}

#[tokio::test]
async fn a_wait_whose_build_cannot_be_kept_does_not_try_again_on_every_round() {
    let f = fixture().await;
    f.files.refuse.store(true, Ordering::SeqCst);
    f.endpoint.set_status_fails(true);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Waiting"
    );
    assert_eq!(named(&f.client), None);
    let attempts = f.files.created.load(Ordering::SeqCst);
    assert_eq!(attempts, 1);
    for _ in 0..3 {
        tick(&f.client).await;
        assert_eq!(f.client.reconciler_status().conclusion.label(), "Waiting");
    }
    assert_eq!(f.files.created.load(Ordering::SeqCst), attempts);
}

#[tokio::test]
async fn a_healthy_round_leaves_nothing_pinned() {
    let f = fixture().await;
    f.endpoint.set_failure(Some("apply_failed"));
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    settled(&f.client, version).await;
    let refused = named(&f.client).unwrap();
    f.endpoint.set_failure(None);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "direct"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Converged"
    );
    assert!(f.candidates.pinned().is_empty());
    // The failure it replaced ages out among the recent ones.
    assert_eq!(f.candidates.kept(), [refused]);
}

#[tokio::test]
async fn an_unpublished_application_keeps_its_candidate_and_its_retries_name_it() {
    let f = fixture().await;
    f.builder.fail_publish.store(true, Ordering::SeqCst);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    let status = settled(&f.client, version).await;
    assert_eq!(status.conclusion.label(), "Converged");
    let id = status
        .degradations
        .iter()
        .find_map(|degradation| match &degradation.reason {
            DegradationReason::RuntimeProductPublishFailed { cause } => {
                cause.candidate_snapshot_id().cloned()
            }
            _ => None,
        })
        .expect("the failed publication names the applied candidate");
    assert_eq!(
        mode_and_stage(&f, &id).await,
        (
            serde_json::json!("global"),
            CandidateStage::AppliedUnpublished
        )
    );
    assert_held(&f, &[&id]);

    // A retry applies again and its new product fails to publish; the
    // republication that follows in the same round names that product's
    // candidate instead of keeping a third one.
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.reconciler_status().maintenance.is_some());
    let [retried] = f.candidates.pinned().try_into().unwrap();
    assert_ne!(retried, id);
    assert_eq!(f.candidates.kept(), [id]);
}

#[tokio::test]
async fn a_converged_runtime_names_its_unpublished_product_until_it_is_published() {
    let f = fixture().await;
    f.builder.fail_publish.store(true, Ordering::SeqCst);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    let status = settled(&f.client, version).await;
    assert_eq!(status.conclusion.label(), "Converged");
    assert!(status.maintenance.is_some());
    let runtime = runtime_convergence(&status);
    let id = runtime
        .candidate_snapshot_id
        .expect("the converged runtime names the unpublished product");
    assert_eq!(
        mode_and_stage(&f, &id).await.1,
        CandidateStage::AppliedUnpublished
    );
    assert_held(&f, &[&id]);

    f.builder.fail_publish.store(false, Ordering::SeqCst);
    f.client.retry_runtime().await.unwrap();
    let status = f.client.reconciler_status();
    assert!(status.maintenance.is_none());
    assert_eq!(runtime_convergence(&status).candidate_snapshot_id, None);
    assert!(f.candidates.pinned().is_empty());
}
