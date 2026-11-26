//! A check that refuses a built candidate keeps it and names it in the error.

use std::sync::Arc;

use nyanpasu_config::{
    application::ClashCore, clash::config::dns::ControlSource, runtime::executor::StepLogLevel,
};
use nyanpasu_core_manager::{CoreError, CoreErrorKind};

use super::mutations::{SourceOwner, app_with_core, commit_source, fixture, overrides, settled};
use crate::{client::test_support::TestCheckAnswer, state::config_error::ConfigError};
use nyanpasu_runtime_values::{mutation::CommitAborted, runtime_error::RuntimeError};

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
        .inspect(id, |archive| {
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
    assert!(f.candidates.inspect(id, |_| Ok(())).await.is_ok());
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
