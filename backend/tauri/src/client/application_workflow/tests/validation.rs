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
    ports::{
        RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeCheckUnavailable, RuntimeValidatorPort,
    },
    preparation::RuntimePreparation,
};
use crate::{
    client::{
        SessionPortResolver,
        core_lifecycle::ports::RuntimePreparationPort,
        runtime,
        tests::{TestCheckAnswer, TestControlEndpoint},
    },
    core::actor_v2::{CoreClient, endpoint::ExecutionHost, facade::CoreFacade},
};

struct Graph {
    endpoint: Arc<TestControlEndpoint>,
    core: CoreClient,
    preparation: RuntimePreparation,
    validator: CoreCheckValidator,
    /// Same core and paths, bounded short enough that a host which never
    /// answers is observable without waiting out the production budget.
    impatient: CoreCheckValidator,
}

async fn graph(dir: &tempfile::TempDir) -> Graph {
    use crate::{
        client::tests::{test_materialization_port, test_typed_config_clients},
        state::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher},
    };
    let (application, _, clash) = test_typed_config_clients(dir).await;
    let (notifier, _dirty) = super::super::DirtyNotifier::channel();
    let profiles = crate::client::profiles::ProfilesClient::new(
        camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap(),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_materialization_port(),
        Arc::new(notifier),
    )
    .await
    .unwrap();
    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let preparation = RuntimePreparation::new(
        application.snapshot_handle(),
        clash.snapshot_handle(),
        profiles.snapshot_handle(),
        Arc::new(super::super::adapters::FsRuntimeBuildAdapter {
            profiles_dir: dir.path().join("profiles"),
            paths: paths.clone(),
        }),
        Arc::new(SessionPortResolver::default()),
    );
    let validator = CoreCheckValidator::new(core.clone(), paths.clone());
    let impatient =
        CoreCheckValidator::with_budget(core.clone(), paths, std::time::Duration::from_millis(50));
    Graph {
        endpoint,
        core,
        preparation,
        validator,
        impatient,
    }
}

/// The completion condition the whole split exists for: the bytes the core was
/// asked to validate are the bytes it is then asked to apply. Both come from
/// one `RuntimeIntent`, so the equality is structural rather than incidental.
#[tokio::test]
async fn the_check_and_the_apply_consume_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph(&dir).await;
    let prepared = graph.preparation.prepare_latest().await.unwrap();
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

    let service = crate::core::actor_v2::service_actor::ServiceClient::spawn(
        Arc::new(crate::client::tests::IdleServiceAdapter),
        0,
    )
    .await
    .unwrap();
    let mut facade = CoreFacade::new(graph.core.clone(), service);
    facade
        .reconcile(&prepared.intent, spec, &facade.core_status())
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

/// V02 family: a host that cannot run a check says so. "We could not check" is
/// never reported as "the core accepted it".
#[tokio::test]
async fn a_host_without_a_check_reports_unavailable_rather_than_passing() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Unsupported);
    let prepared = graph.preparation.prepare_latest().await.unwrap();
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

    let RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::HostUnsupported { host, reason }) =
        outcome
    else {
        panic!("expected an explicitly unavailable check, got {outcome:?}");
    };
    assert_eq!(host, ExecutionHost::Local);
    assert!(reason.contains("no config check"));
}

/// A check service that is momentarily unreachable is also not a pass, and it
/// is not the core rejecting the document either: it keeps its own shape so a
/// caller can tell a deterministic rejection from a transient outage.
#[tokio::test]
async fn an_unreachable_check_is_unavailable_not_a_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "check service is restarting",
            true,
        )));
    let prepared = graph.preparation.prepare_latest().await.unwrap();
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
    let mut graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "unknown field `nonsense`",
            false,
        )));
    let prepared = graph.preparation.prepare_latest().await.unwrap();
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
    let mut graph = graph(&dir).await;
    graph
        .endpoint
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "config check failed: config check timed out after 30s",
            false,
        )));
    let prepared = graph.preparation.prepare_latest().await.unwrap();
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

/// The app's own bound on the call, for a host that accepts the request and
/// then never answers at all. Same answer: unavailable and retryable, never a
/// verdict and never a pass.
#[tokio::test]
async fn a_host_that_never_answers_elapses_into_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph(&dir).await;
    graph.endpoint.set_check_answer(TestCheckAnswer::Hang);
    let prepared = graph.preparation.prepare_latest().await.unwrap();
    let spec = graph
        .preparation
        .core_spec(&prepared.snapshot.target_core)
        .unwrap();

    let outcome = graph
        .impatient
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
        panic!("a host that never answered has not checked anything, got {outcome:?}");
    };
    assert!(retryable);
    assert!(message.contains("did not answer within"));
}
