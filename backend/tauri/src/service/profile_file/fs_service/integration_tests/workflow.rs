//! Runtime transactions retaining persisted bytes and staging ownership.

use crate::service::profile_file::test_support::test_client_args_with_endpoint;
use futures_util::FutureExt;
use nyanpasu_config::clash::config::{
    ClashConfig,
    clash_strategy::port::{PortStrategy, PortStrategyKind},
};
use nyanpasu_core::client::{
    NyanpasuClient,
    application_workflow::{mutation, startup::StartupOutcome},
    test_support,
    test_support::{
        TestControlEndpoint,
        workflow::{
            Fixture, ScriptedWaitEndpoint, WaitScript, aftermath, disable_mode_interruption,
            override_patch, start_replacement,
        },
    },
};
use nyanpasu_ipc::api::status::CoreStateDetail;
const STOPPED: CoreStateDetail = CoreStateDetail::Stopped { reason: None };
/// S4 (random port): startup is not a source transaction. A randomly picked
/// mixed port lands in the runtime binding, and none of the four source
/// domains changes, neither its version nor its bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_random_port_start_binds_its_pick_and_rewrites_no_source() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_status(Some(STOPPED), None);
    let args = test_client_args_with_endpoint(&dir, endpoint.clone()).await;
    let clash = ClashConfig {
        mixed_port: PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 7890,
        },
        ..ClashConfig::default()
    };
    nyanpasu_core::client::test_support::write_clash_config(args.paths.clash_config_path(), &clash);
    let files = [
        args.paths.application_config_path(),
        args.paths.clash_config_path(),
        args.paths.profiles_path(),
        args.paths.session_state_path(),
    ];
    let bytes = || {
        files
            .iter()
            .map(|path| std::fs::read(path).ok())
            .collect::<Vec<_>>()
    };
    let client = NyanpasuClient::try_new(args).await.unwrap();
    (async {
        let versions = |client: &NyanpasuClient| {
            let versions = client.configuration_status().source_versions;
            (
                versions.application,
                versions.clash,
                versions.session,
                versions.profiles,
            )
        };
        let (before_versions, before_bytes) = (versions(&client), bytes());

        let report = client.startup_reconcile().await;

        assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
        assert_eq!(versions(&client), before_versions);
        assert_eq!(bytes(), before_bytes);
        assert_eq!(endpoint.submissions(), 1);
        let submitted: serde_yaml::Value =
            serde_yaml::from_slice(&endpoint.reconciled_bytes()[0]).unwrap();
        let picked = submitted["mixed-port"]
            .as_u64()
            .expect("the submitted document carries its mixed port");
        assert_eq!(
            client
                .session_ports()
                .map(|ports| u64::from(ports.mixed_port)),
            Some(picked),
            "the binding is the port the build picked"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacement_serializes_reconcile_and_retains_files_after_caller_cancellation() {
    let f = Fixture::with_adapters(true, false, |dir| {
        crate::service::profile_file::test_support::test_host_adapters(
            &nyanpasu_core::utils::test_support::test_paths(dir.path(), dir.path().join("data")),
        )
    })
    .await;
    (async {
        f.endpoint.prime(&f.client).await;
        let (task, staging) = start_replacement(&f).await;
        assert_eq!(
            f.endpoint.submissions(),
            2,
            "stop and death proof precede installation"
        );
        let active = test_support::workflow(&f.client).status().active.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(staging.exists());
        // Polling once sends the reconcile, so it waits behind the parked
        // installation.
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        assert_eq!(
            test_support::workflow(&f.client).status().active,
            Some(active)
        );
        assert_eq!(f.endpoint.submissions(), 2);
        // Status reads stay responsive while the installer is parked.
        let _ = f.client.core_status();
        let _ = f.client.promoted_runtime().await;
        f.installer.release.notify_one();
        reconcile.await.unwrap();
        assert_eq!(
            f.endpoint.submissions(),
            4,
            "replacement restart followed by queued reconcile"
        );
        assert!(!staging.exists());
        assert!(!test_support::workflow(&f.client).status().uncertain);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reconcile_failure_reports_committed_state_without_replaying() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::failing();
    let args = test_client_args_with_endpoint(&dir, endpoint.clone()).await;
    let config_path = args.paths.clash_config_path();
    let client = NyanpasuClient::try_new(args).await.unwrap();
    (async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let outcome = client
            .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"direct"})))
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert!(matches!(
            outcome.degradations()[0].reason,
            nyanpasu_core::client::runtime::DegradationReason::RuntimeDeferred { .. }
        ));
        assert_eq!(
            client.configuration_status().runtime.health,
            nyanpasu_core::client::convergence::ConvergenceHealth::RetryScheduled
        );
        assert_eq!(endpoint.submissions(), 1);
        let persisted: nyanpasu_config::clash::config::ClashConfig =
            serde_yaml::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(persisted.overrides).unwrap()["mode"],
            "direct"
        );
        assert_eq!(
            serde_json::to_value(client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
            "direct"
        );
    }).await;
}

/// V08/V09 (U7): a save whose write fails after its Try applied is returned
/// as an error that names the persistence cause and what became of the
/// runtime: rolled back, or not, in which case the domain is isolated. The
/// source keeps its version either way.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_persistence_failure_restores_runtime_and_keeps_source_unchanged() {
    for restore_lost in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let scripted = ScriptedWaitEndpoint::new(endpoint.clone());
        let args = test_client_args_with_endpoint(&dir, scripted.clone()).await;
        let path = args.paths.clash_config_path();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        (async {
            endpoint.prime(&client).await;
            disable_mode_interruption(&client).await;
            let before = test_support::clash_snapshot(&client);
            if path.exists() {
                std::fs::remove_file(&path).unwrap();
            }
            std::fs::create_dir_all(&path).unwrap();
            // The Try applies; the Cancel's restore may lose its answer.
            scripted.queue(WaitScript::Deliver);
            if restore_lost {
                scripted.queue(WaitScript::Missing);
            }
            let error = client
                .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"})))
                .await
                .expect_err("the save failed");
            let after = test_support::clash_snapshot(&client);
            assert_eq!(after.version, before.version);
            assert_eq!(
                serde_json::to_value(after.state).unwrap(),
                serde_json::to_value(before.state).unwrap()
            );
            let nyanpasu_core::client::ClientError::Config(
                nyanpasu_core::state::config_error::ConfigError::Commit {
                    domain: mutation::ConfigDomain::Clash,
                    source: aborted,
                },
            ) = &error
            else {
                panic!("{error:?}");
            };
            let nyanpasu_core::state::mutation::CommitAborted::WriteConfig { source, .. } = aborted
            else {
                panic!("{aborted:?}");
            };
            assert!(
                source.to_string().contains("failed to write config"),
                "{source}"
            );
            assert_eq!(
                endpoint.submissions(),
                2,
                "Try applied and Cancel resubmitted the baseline"
            );
            if restore_lost {
                assert!(
                    matches!(
                        aftermath(aborted),
                        nyanpasu_core::state::mutation::RuntimeAftermath::RollbackFailed { .. }
                    ),
                    "{aborted:?}"
                );
                assert!(test_support::workflow(&client).status().uncertain);
            } else {
                assert!(
                    matches!(
                        aftermath(aborted),
                        nyanpasu_core::state::mutation::RuntimeAftermath::RolledBack
                    ),
                    "{aborted:?}"
                );
                assert!(!test_support::workflow(&client).status().uncertain);
            }
        })
        .await;
    }
}
