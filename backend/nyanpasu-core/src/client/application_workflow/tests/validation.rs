//! T4: the advisory check and the apply consume one document.
//!
//! The runtime's check is advisory and read-only by design, so a missing or
//! unserviceable one is its own answer. These tests pin that it never collapses
//! into "passed", and that whatever the check saw is byte-for-byte what the
//! reconcile submits.

use std::sync::Arc;

use nyanpasu_core_manager::{CoreError, CoreErrorKind};

use super::super::{
    adapters::CoreCheckValidator,
    ports::{RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeValidatorPort},
    preparation::RuntimePreparation,
};
use crate::{
    actor_v2::{CoreClient, facade::CoreFacade},
    client::{
        SessionPortResolver,
        core_lifecycle::ports::RuntimePreparationPort,
        runtime,
        test_support::{TestCheckAnswer, TestControlEndpoint, runtime_build::TestRuntimeBuildPort},
    },
};
use nyanpasu_runtime_values::{check::RuntimeCheckUnavailable, execution_host::ExecutionHost};

struct Graph {
    endpoint: Arc<TestControlEndpoint>,
    core: CoreClient,
    preparation: RuntimePreparation,
    validator: CoreCheckValidator,
}

async fn graph(dir: &tempfile::TempDir) -> Graph {
    use crate::client::test_support::test_materialization_port;
    use nyanpasu_contracts::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
    let profiles = crate::client::profiles::ProfilesClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap(),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_materialization_port(),
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let paths = runtime::RuntimePaths::from_resolver(&crate::utils::test_support::test_paths(
        dir.path(),
        dir.path().join("data"),
    ));
    let endpoint = TestControlEndpoint::succeeding();
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let preparation = RuntimePreparation::new(
        profiles.snapshot_handle(),
        profiles.content_lock(),
        Arc::new(TestRuntimeBuildPort::from_directory(
            dir.path().join("profiles"),
            paths,
        )),
        Arc::new(SessionPortResolver::default()),
    );
    let validator = CoreCheckValidator::new(&core);
    Graph {
        endpoint,
        core,
        preparation,
        validator,
    }
}

/// The completion condition the whole split exists for: the bytes the core was
/// asked to validate are the bytes it is then asked to apply. Both come from
/// one `RuntimeIntent`, so the equality is structural rather than incidental.
#[tokio::test]
async fn the_check_and_the_apply_consume_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph(&dir).await;
    let prepared = graph
        .preparation
        .prepare(
            Arc::new(nyanpasu_config::profile::Profiles::default()),
            crate::client::test_support::test_clash_config(),
            nyanpasu_config::application::NyanpasuAppConfig::default(),
        )
        .await
        .unwrap();
    let spec = graph
        .preparation
        .core_spec(&prepared.snapshot.target_core)
        .unwrap();

    let outcome = graph
        .validator
        .check(RuntimeCheckRequest {
            core_spec: spec.clone(),
            intent: &prepared.intent,
        })
        .await;
    assert_eq!(outcome, RuntimeCheckOutcome::Passed);

    let service = {
        let process_control = Arc::new(crate::actor_v2::test_support::IdleServiceAdapter);
        crate::actor_v2::service_actor::ServiceClient::spawn(
            process_control.clone(),
            process_control,
            0,
        )
    }
    .await
    .unwrap();
    let mut facade = CoreFacade::new(graph.core.clone(), service);
    let expected = facade.refresh_status().await.unwrap();
    facade
        .reconcile(&prepared.intent, spec, &expected)
        .await
        .unwrap();

    let checked = graph.endpoint.checked();
    let applied = graph.endpoint.reconciled_bytes();
    assert_eq!(checked.len(), 1);
    assert_eq!(applied.len(), 1);
    assert_eq!(
        checked[0].config_bytes, applied[0],
        "the checked document and the applied document must be the same bytes"
    );
    assert_eq!(
        checked[0].digest,
        nyanpasu_core_manager::payload_digest(&applied[0]),
        "the digest the check declared is the digest of what was applied"
    );
}

#[tokio::test]
async fn a_service_hosted_core_is_checked_through_the_local_endpoint() {
    use crate::client::application_workflow::startup::StartupOutcome;

    use super::{
        mutations::{commit_source, overrides, settled},
        startup::{DaemonState, Setup},
    };

    let graph = super::startup::graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        residual: true,
        owner_on_service: true,
        ..Setup::default()
    })
    .await;
    let report = graph.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
    assert_eq!(graph.core.status().host, ExecutionHost::Service);
    assert!(graph.local.delegate.checked().is_empty());
    assert!(graph.service_host.delegate.checked().is_empty());
    assert!(graph.local.delegate.reconciled_bytes().is_empty());
    let applied_before = graph.service_host.delegate.reconciled_bytes().len();
    assert_eq!(applied_before, 1);

    let id = commit_source(
        &graph.clash,
        overrides(serde_json::json!({"mode": "global"})),
    )
    .await
    .unwrap();
    assert_eq!(
        settled(&graph.client, id).await.conclusion.label(),
        "Converged"
    );
    let checked = graph.local.delegate.checked();
    assert_eq!(checked.len(), 1);
    assert!(graph.service_host.delegate.checked().is_empty());
    assert!(checked[0].staged_config.is_none());
    assert!(graph.local.delegate.reconciled_bytes().is_empty());
    let applied = graph.service_host.delegate.reconciled_bytes();
    assert_eq!(applied.len(), applied_before + 1);
    assert_eq!(checked[0].config_bytes, applied[applied_before]);
}

/// A check service that is momentarily unreachable is also not a pass, and it
/// is not the core rejecting the document either: it keeps its own shape so a
/// caller can tell a deterministic rejection from a transient outage.
#[tokio::test]
async fn an_unreachable_check_is_unavailable_not_a_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "check service is restarting",
            true,
        )));
    let prepared = graph
        .preparation
        .prepare(
            Arc::new(nyanpasu_config::profile::Profiles::default()),
            crate::client::test_support::test_clash_config(),
            nyanpasu_config::application::NyanpasuAppConfig::default(),
        )
        .await
        .unwrap();
    let spec = graph
        .preparation
        .core_spec(&prepared.snapshot.target_core)
        .unwrap();

    let outcome = graph
        .validator
        .check(RuntimeCheckRequest {
            core_spec: spec,
            intent: &prepared.intent,
        })
        .await;

    assert_eq!(
        outcome,
        RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
            kind: Some(CoreErrorKind::BackendUnavailable),
            message: "check service is restarting".into(),
            retryable: true,
        })
    );
}

/// The core's own verdict on the document is the one thing that is a
/// rejection, and it stays deterministic: the same bytes fail again.
#[tokio::test]
async fn the_cores_own_verdict_is_a_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "unknown field `nonsense`",
            false,
        )));
    let prepared = graph
        .preparation
        .prepare(
            Arc::new(nyanpasu_config::profile::Profiles::default()),
            crate::client::test_support::test_clash_config(),
            nyanpasu_config::application::NyanpasuAppConfig::default(),
        )
        .await
        .unwrap();
    let spec = graph
        .preparation
        .core_spec(&prepared.snapshot.target_core)
        .unwrap();

    let outcome = graph
        .validator
        .check(RuntimeCheckRequest {
            core_spec: spec,
            intent: &prepared.intent,
        })
        .await;

    assert_eq!(
        outcome,
        RuntimeCheckOutcome::Rejected {
            kind: Some(CoreErrorKind::ConfigCheckFailed),
            message: "unknown field `nonsense`".into(),
        }
    );
}

/// The runtime bounds its own `-t` run at 30 seconds and reports the elapse as
/// `ConfigCheckFailed` with the same wire kind as a real rejection, naming the
/// bound in the message. A slow binary, a loaded machine or a large ruleset
/// must not therefore arrive as a permanent verdict on the user's config:
/// `Rejected` promises the same bytes fail again, and a timeout promises
/// nothing of the sort.
#[tokio::test]
async fn the_runtimes_own_check_timeout_is_unavailable_not_a_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "config check failed: config check timed out after 30s",
            false,
        )));
    let prepared = graph
        .preparation
        .prepare(
            Arc::new(nyanpasu_config::profile::Profiles::default()),
            crate::client::test_support::test_clash_config(),
            nyanpasu_config::application::NyanpasuAppConfig::default(),
        )
        .await
        .unwrap();
    let spec = graph
        .preparation
        .core_spec(&prepared.snapshot.target_core)
        .unwrap();

    let outcome = graph
        .validator
        .check(RuntimeCheckRequest {
            core_spec: spec,
            intent: &prepared.intent,
        })
        .await;

    let RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
        retryable,
        message,
        ..
    }) = outcome
    else {
        panic!("a check that never ran is not a verdict on the config, got {outcome:?}");
    };
    assert!(
        retryable,
        "a wedged check binary is retryable; calling it permanent destroys the deferral"
    );
    assert!(message.contains("timed out"));
}
