use std::sync::Arc;

use nyanpasu_config::{
    application::ClashCore, clash::config::tun_stack::TunStack, runtime::executor::StepLogLevel,
};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};

use super::{
    classify,
    mutations::{
        app_with_core, fixture, mutate_settling, no_local_write, overrides, plain, refused,
        settled, simple_mutate,
    },
    refusals,
};
use crate::client::{
    application_workflow::{
        mutation::{MutationConclusion, MutationOutcomeKind},
        policy::CommandClass,
        ports::RuntimeCheckUnavailable,
    },
    runtime_error::RuntimeError,
    test_support::TestCheckAnswer,
};

#[tokio::test]
async fn rejected_check_retains_graph_and_logs_before_error_returns() {
    let mut f = fixture().await;
    let (id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(ClashCore::ClashPremium),
        CommandClass::ExplicitSwitch,
    )
    .await;
    result.unwrap();
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    let store = f.client.snapshot_store();
    let promoted = store.read().promoted.unwrap();
    let confirmed = store.confirmed().unwrap();
    let submissions = f.endpoint.submissions();
    let version = f.clash.snapshot_handle().load().version;
    let message = "  IPC request `/core/check` failed with OtherError: config check failed: bad group at /tmp/check-source-private.yaml\nconfiguration file C:\\Temp\\nyanpasu-core-check-private\\check.yaml test failed\n  ";
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            message,
            false,
        )));
    let mut next = overrides(serde_json::json!({"mode": "global"}));
    next.enable_tun_mode = true;
    next.tun_stack = TunStack::Mixed;
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        next,
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    let receipt = settlement.await.unwrap();
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert_eq!(receipt.conclusion, MutationConclusion::Withdrawn);
    let errors = refusals(&classify(result.unwrap_err(), &receipt));
    let RuntimeError::CoreRejectedConfig {
        message: returned,
        rejected_snapshot_id,
        ..
    } = errors[0].as_ref()
    else {
        panic!("expected a check rejection: {:?}", errors[0]);
    };
    assert_eq!(returned.as_bytes(), message.as_bytes());
    let snapshot = f
        .client
        .rejected_builds()
        .load(rejected_snapshot_id.as_deref().unwrap())
        .unwrap();
    assert!(!snapshot.graph.nodes.is_empty());
    let final_config = snapshot.graph.restore(snapshot.final_node).unwrap();
    assert_eq!(final_config["mode"], "global");
    assert_eq!(final_config["tun"]["stack"], "gvisor");
    assert!(
        snapshot
            .step_logs
            .iter()
            .flat_map(|log| &log.entries)
            .any(|entry| entry.level == StepLogLevel::Warn)
    );
    let checked: serde_json::Value =
        serde_yaml_ng::from_slice(&f.endpoint.checked().last().unwrap().config_bytes).unwrap();
    assert_eq!(checked, final_config);
    assert_eq!(f.endpoint.submissions(), submissions);
    assert_eq!(f.clash.snapshot_handle().load().version, version);
    assert!(Arc::ptr_eq(&store.read().promoted.unwrap(), &promoted));
    assert!(Arc::ptr_eq(
        &store.confirmed().unwrap().receipt,
        &confirmed.receipt
    ));
}

#[tokio::test]
async fn rejected_write_failure_keeps_the_refusal_and_releases_the_candidate() {
    let mut store = crate::client::application_workflow::ports::MockRejectedBuildStore::new();
    let released = Arc::new(std::sync::Mutex::new(None));
    let capture = released.clone();
    store.expect_save().times(1).returning(move |snapshot| {
        *capture.lock().unwrap() = Some(Arc::downgrade(&snapshot));
        None
    });
    store.expect_clear().return_const(());
    let mut f = super::mutations::fixture_with_rejected_store(Arc::new(store)).await;
    let message = "  opaque original refusal\n";
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            message,
            false,
        )));
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    let receipt = settlement.await.unwrap();
    let errors = refusals(&classify(result.unwrap_err(), &receipt));
    let RuntimeError::CoreRejectedConfig {
        message: returned,
        rejected_snapshot_id,
        ..
    } = errors[0].as_ref()
    else {
        panic!("expected original rejection");
    };
    assert_eq!(returned, message);
    assert!(rejected_snapshot_id.is_none());
    assert!(serde_json::to_value(errors[0].as_ref()).unwrap()["rejected_snapshot_id"].is_null());
    assert!(
        released
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .upgrade()
            .is_none()
    );
}

#[tokio::test]
async fn rejected_successful_check_requests_source_cleanup() {
    let mut store = crate::client::application_workflow::ports::MockRejectedBuildStore::new();
    store.expect_clear_source().times(1).return_const(());
    store.expect_clear().return_const(());
    let mut f = super::mutations::fixture_with_rejected_store(Arc::new(store)).await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
    )
    .await;
    result.unwrap();
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
}

#[tokio::test]
async fn rejected_normal_shutdown_clears_the_file() {
    let mut f = fixture().await;
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "opaque refusal",
            false,
        )));
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    let receipt = settlement.await.unwrap();
    let errors = refusals(&classify(result.unwrap_err(), &receipt));
    let RuntimeError::CoreRejectedConfig {
        rejected_snapshot_id: Some(id),
        ..
    } = errors[0].as_ref()
    else {
        panic!("expected a persisted rejection");
    };
    let store = f.client.rejected_builds();
    assert!(store.load(id).is_ok());
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
    assert!(matches!(
        store.load(id),
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
}

#[tokio::test]
async fn rejected_legacy_check_without_kind_stays_unavailable() {
    let mut store = crate::client::application_workflow::ports::MockRejectedBuildStore::new();
    store.expect_save().times(0);
    store.expect_clear().return_const(());
    let mut f = super::mutations::fixture_with_rejected_store(Arc::new(store)).await;
    let message = "IPC request `/core/check` failed with OtherError: config check failed: invalid group in /tmp/candidate-private.yaml\n  ";
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError {
            kind: None,
            message: message.to_owned(),
            retryable: false,
            operation_id: None,
        }));
    let (result, settlement) = mutate_settling(
        &mut f.application,
        &f.client,
        OperationId::generate(),
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
        plain(),
        no_local_write,
    )
    .await;
    assert!(refused(&result), "{result:?}");

    let receipt = settlement.await.unwrap();
    let errors = refusals(&classify(result.unwrap_err(), &receipt));
    assert_eq!(errors.len(), 1);
    let RuntimeError::CheckUnavailable {
        reason:
            RuntimeCheckUnavailable::Backend {
                kind,
                retryable,
                message: returned,
            },
    } = errors[0].as_ref()
    else {
        panic!("expected an unavailable check, got {:?}", errors[0]);
    };
    assert_eq!(*kind, None);
    assert!(!retryable);
    assert_eq!(returned.as_bytes(), message.as_bytes());
    assert!(f.endpoint.reconciled_bytes().is_empty());
}

#[tokio::test]
async fn rejected_file_is_deleted_when_the_same_profile_passes_its_next_check() {
    let mut f = fixture().await;
    std::fs::create_dir_all(&f.profiles_dir).unwrap();
    std::fs::write(
        f.profiles_dir.join("source.yaml"),
        "proxies: []\nrules: []\n",
    )
    .unwrap();
    let item = crate::enhance::test_support::file_config("source", "source.yaml", &[]);
    let mut next = f.profiles.snapshot().as_ref().clone();
    next.current = Some(item.uid.clone());
    next.items.insert(item.uid.clone(), item);
    let (operation, result) =
        simple_mutate(&mut f.profiles, &f.client, next, CommandClass::Save).await;
    result.unwrap();
    assert_eq!(
        settled(&f.client, operation).await.conclusion,
        MutationConclusion::Confirmed
    );
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "original message",
            false,
        )));
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    let receipt = settlement.await.unwrap();
    let errors = refusals(&classify(result.unwrap_err(), &receipt));
    let RuntimeError::CoreRejectedConfig {
        rejected_snapshot_id: Some(id),
        ..
    } = errors[0].as_ref()
    else {
        panic!("expected a persisted rejection");
    };
    let store = f.client.rejected_builds();
    assert!(store.load(id).is_ok());
    f.endpoint.set_check_answer(TestCheckAnswer::Pass);
    let (operation, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
    )
    .await;
    result.unwrap();
    assert_eq!(
        settled(&f.client, operation).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert!(matches!(
        store.load(id),
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
}
