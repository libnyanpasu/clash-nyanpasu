//! Facade integration with real profile, log, backup and runtime files.

#[cfg(unix)]
use crate::{
    client::test_support::facade::local_config_request,
    state::profiles::ports::{
        CleanupOutcome, MaterializationReconcileReport, MockProfileMaterializationPort,
        PreparedCleanup, PreparedMaterialization,
    },
};
use crate::{
    client::{
        NyanpasuClient,
        test_support::{
            self, TestCheckAnswer, TestControlEndpoint, facade::test_client,
            minimal_file_profile_request, test_runtime_paths,
        },
    },
    service::profile_file::test_support::{
        test_client_args_with_endpoint, test_client_with_fetcher,
    },
    state::profiles::ports::MockSubscriptionFetcher,
};
use nyanpasu_core::utils::test_support::test_paths;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
/// Restores a directory's unix mode on drop so tempdir cleanup stays reliable
/// after permission-poison tests.
#[cfg(unix)]
struct RestoreDirMode {
    path: PathBuf,
    mode: u32,
}
#[cfg(unix)]
impl Drop for RestoreDirMode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_facade_reads_app_files_and_degrades_service_independently() {
    use crate::client::logs::LogSource;
    use nyanpasu_logging::{Direction, Filter, LogError, OpenLogs, QueryLogs};
    let dir = tempdir().unwrap();
    let paths = test_paths(dir.path(), dir.path().join("data"));
    std::fs::create_dir_all(paths.app_logs_dir()).unwrap();
    std::fs::write(
        paths
            .app_logs_dir()
            .join("clash-nyanpasu.2026-09-11.app.log"),
        b"{\"level\":\"INFO\",\"fields\":{\"message\":\"app own log\"}}\n",
    )
    .unwrap();
    let client = test_client(&dir).await;
    (async {
        assert_eq!(
            client.list_log_files(LogSource::App).await.unwrap().len(),
            1
        );
        assert_eq!(
            client.list_log_files(LogSource::Service).await.unwrap_err(),
            LogError::Unsupported
        );
        let session = client
            .open_log_session(
                LogSource::App,
                "window".into(),
                OpenLogs {
                    request_id: "first".into(),
                    file: None,
                },
            )
            .await
            .unwrap();
        let page = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let page = client
                    .query_logs(
                        LogSource::App,
                        "window".into(),
                        QueryLogs {
                            session: session.id.clone(),
                            filter: Filter::default(),
                            direction: Direction::Latest,
                            cursor: None,
                            limit: 200,
                        },
                    )
                    .await
                    .unwrap();
                if !page.building {
                    break page;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(page.rows[0].message, "app own log");
        client
            .close_log_session(LogSource::App, "window".into(), session.id)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_backups_keep_the_newest_three_and_leave_migration_backups() {
    let dir = tempdir().expect("tempdir should be created");
    let backups = dir.path().join("data").join("backups");
    let migration = backups.join("migration-20250101T000000Z-1.0.0-to-2.0.0");
    std::fs::create_dir_all(&migration).unwrap();

    let names = (async {
        let client = test_client(&dir).await;
        let mut names = Vec::new();
        for _ in 0..4 {
            names.push(client.create_config_backup().await.unwrap().name);
        }
        assert_eq!(client.backups_dir().unwrap(), backups);
        names
    })
    .await;

    let mut kept: Vec<_> = std::fs::read_dir(&backups)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("manual-"))
        .collect();
    kept.sort();
    let mut expected = names[1..].to_vec();
    expected.sort();
    assert_eq!(kept, expected);
    assert!(migration.exists());
}

#[tokio::test]
async fn reconcile_publishes_the_runtime_product_read_model() {
    let dir = tempdir().unwrap();
    let client = test_client(&dir).await;
    client.reconcile_core().await.unwrap();
    assert!(client.promoted_runtime().await.is_some());
    assert!(test_runtime_paths(&dir).product().exists());
}

#[tokio::test]
async fn reconcile_product_matches_the_published_snapshot() {
    let dir = tempdir().unwrap();
    let client = test_client(&dir).await;
    client.reconcile_core().await.unwrap();
    let snapshot = client.promoted_runtime().await.unwrap();
    assert_eq!(
        tokio::fs::read(test_runtime_paths(&dir).product())
            .await
            .unwrap(),
        snapshot.product_bytes()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn facade_add_activate_rebuilds_via_control_endpoint() {
    let dir = tempdir().unwrap();
    let client = test_client_with_fetcher(&dir, Arc::new(MockSubscriptionFetcher::new())).await;

    (async {
        let uid = client
            .add_profile(
                minimal_file_profile_request(),
                Some("proxies: []\nmode: rule\n".into()),
            )
            .await
            .expect("add")
            .into_value();
        client
            .activate_profile(Some(uid.clone()))
            .await
            .expect("activate");
        client.reconcile_core().await.expect("reconcile");
        let promoted = client
            .promoted_runtime()
            .await
            .expect("promoted runtime stored after rebuild");
        assert!(promoted.config().get("mixed-port").is_some());
        assert!(
            !promoted.exists_keys.is_empty(),
            "guard overrides must register applied fields"
        );
        let _ = promoted.postprocessing_output.clone();

        let lifecycle = test_support::workflow(&client).runtime();
        assert!(
            lifecycle
                .promoted
                .as_ref()
                .is_some_and(|current| current.identity_eq(promoted.as_ref()))
        );
        let path = client
            .get_profile_materialized_path(uid.clone())
            .await
            .unwrap();
        let expected_file = format!("{}.yaml", uid.0);
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(expected_file.as_str())
        );
        let content = client.read_profile_file(uid.clone()).await.unwrap();
        assert!(content.contains("proxies"));
        client
            .save_profile_file(uid.clone(), "proxies: []\nmode: direct\n".into())
            .await
            .unwrap();
    })
    .await;
}

/// H2 E2E (Unix): Add commits, then post-commit `set_current_if_none` state
/// persistence fails via the production materialization `complete` seam
/// permission-poisoning the profiles parent. create_profile must return
/// `Ok(CommittedDegraded)` with the real ProfileId and keep current empty.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn facade_create_auto_activation_persist_failure_is_committed_degraded() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let parent = dir.path().to_path_buf();
    let restore = RestoreDirMode {
        path: parent.clone(),
        mode: 0o755,
    };

    let mut materialization = MockProfileMaterializationPort::new();
    materialization
        .expect_reconcile()
        .returning(|_| Ok(MaterializationReconcileReport::default()));
    materialization
        .expect_prepare_file_first()
        .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
    materialization.expect_promote().returning(|_| Ok(()));
    let parent_for_complete = parent.clone();
    materialization.expect_complete().returning(move |_| {
        // After durable Add commit, block the subsequent profiles.yaml rewrite
        // that set_current_if_none needs for auto-activation.
        std::fs::set_permissions(&parent_for_complete, std::fs::Permissions::from_mode(0o555))
            .expect("poison profiles parent after Add complete");
        Ok(())
    });
    materialization.expect_compensate().returning(|_| Ok(()));
    materialization
        .expect_prepare_cleanup()
        .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
    materialization
        .expect_activate_cleanup()
        .returning(|_| Ok(()));
    materialization
        .expect_cancel_cleanup()
        .returning(|_| Ok(()));
    materialization
        .expect_retry_cleanup()
        .returning(|_, _| Ok(CleanupOutcome::Removed));

    (async {
        let client =
            test_support::facade::test_client_with_materialization(&dir, Arc::new(materialization))
                .await;

        let outcome = client
            .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
            .await
            .expect("create must keep the committed ProfileId as Ok");
        // Restore before further assertions that may touch the temp dir.
        drop(restore);

        assert!(
            matches!(
                outcome,
                crate::client::runtime::MutationOutcome::CommittedDegraded { .. }
            ),
            "auto-activation hard failure after commit must be CommittedDegraded"
        );
        let uid = outcome.value().clone();
        assert!(
            outcome.degradations().iter().any(|item| matches!(
                item.reason,
                crate::client::runtime::DegradationReason::ProfileAutoActivationFailed { .. }
            )),
            "expected ProfileAutoActivationFailed, got {:?}",
            outcome.degradations()
        );
        assert!(
            outcome.degradations().iter().any(|item| {
                item.phase == crate::client::runtime::DegradationPhase::SystemEffect
                    && item.retryable
            }),
            "H2 degradation must be retryable SystemEffect"
        );

        let snapshot = client.get_profiles().await.unwrap();
        assert!(
            snapshot.items.contains_key(&uid),
            "committed item must remain after auto-activation failure"
        );
        assert!(
            snapshot.current.is_none(),
            "failed set_current_if_none must leave current unset"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_edit_uses_candidate_bytes_and_rejects_invalid_runtime_without_overwriting_source()
{
    let dir = tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    let client =
        NyanpasuClient::try_new(test_client_args_with_endpoint(&dir, endpoint.clone()).await)
            .await
            .unwrap();
    (async {
        client.reconcile_core().await.unwrap();
        let uid = client
            .add_profile(
                minimal_file_profile_request(),
                Some("proxies: []\nmode: rule\n".into()),
            )
            .await
            .unwrap()
            .into_value();
        client.activate_profile(Some(uid.clone())).await.unwrap();
        let path = client
            .get_profile_materialized_path(uid.clone())
            .await
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let revision = client.get_profiles().await.unwrap().revision();
        endpoint.set_check_answer(TestCheckAnswer::Reject(
            nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::InvalidConfig,
                "candidate rejected",
                false,
            ),
        ));
        assert!(
            client
                .save_profile_file(
                    uid.clone(),
                    "proxies: [{name: t6-fresh, type: direct}]\n".into()
                )
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(client.get_profiles().await.unwrap().revision(), revision);
        endpoint.set_check_answer(TestCheckAnswer::Pass);
        client
            .save_profile_file(
                uid.clone(),
                "proxies: [{name: t6-fresh, type: direct}]\n".into(),
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "proxies: [{name: t6-fresh, type: direct}]\n"
        );
        let runtime = client.promoted_runtime().await.unwrap();
        assert_eq!(
            runtime
                .config()
                .get("proxies")
                .and_then(serde_yaml::Value::as_sequence)
                .and_then(|items| items.first())
                .and_then(|item| item.get("name"))
                .and_then(serde_yaml::Value::as_str),
            Some("t6-fresh")
        );
        client.request_shutdown();
        client.wait_shutdown().await;
    })
    .await;
}
