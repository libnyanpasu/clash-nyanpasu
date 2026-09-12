use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{Router, extract::State, http::StatusCode, routing::delete};
use futures_util::FutureExt;
use nyanpasu_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo},
    status::{CoreControllerInfo, CoreStateDetail},
};
use tokio::sync::Notify;

use crate::{
    client::{
        NyanpasuClient,
        tests::{TestControlEndpoint, test_client_args_with_endpoint},
    },
    core::actor_v2::endpoint::{
        ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    },
};

#[derive(Default)]
struct Calls {
    events: Mutex<Vec<&'static str>>,
    fail_close: AtomicBool,
    hold_close: AtomicBool,
    entered: Notify,
    release: Notify,
}

struct Endpoint {
    delegate: Arc<TestControlEndpoint>,
    binding: Mutex<Option<CoreApiConnection>>,
    replace: AtomicBool,
    report_restart: AtomicBool,
    calls: Arc<Calls>,
    api_queries: AtomicUsize,
}

impl Endpoint {
    fn outcome(&self, mut operation: OperationInfo) -> OperationInfo {
        if let Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(outcome)) =
            &mut operation.output
        {
            outcome.outcome = if self.report_restart.load(Ordering::SeqCst) {
                nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Restarted
            } else {
                nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Patched
            };
        }
        operation
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for Endpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        self.api_queries.fetch_add(1, Ordering::SeqCst);
        Ok(self.binding.lock().unwrap().clone())
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.calls.events.lock().unwrap().push("reconcile");
        let result = self.delegate.submit(submission).await;
        if self.replace.load(Ordering::SeqCst) {
            self.binding.lock().unwrap().as_mut().unwrap().instance_id = "replacement".into();
        }
        result.map(|operation| self.outcome(operation))
    }
    async fn wait_operation(
        &self,
        id: OperationId,
        timeout: std::time::Duration,
    ) -> Option<OperationInfo> {
        self.delegate
            .wait_operation(id, timeout)
            .await
            .map(|operation| self.outcome(operation))
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

struct Fixture {
    client: NyanpasuClient,
    endpoint: Arc<Endpoint>,
    calls: Arc<Calls>,
    server: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    fn new(fail_reconcile: bool) -> Self {
        Self::with_installer(
            fail_reconcile,
            Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
        )
    }

    fn with_installer(
        fail_reconcile: bool,
        installer: Arc<dyn crate::client::core_lifecycle::ports::BinaryInstaller>,
    ) -> Self {
        let calls = Arc::new(Calls::default());
        let (url, server) = tauri::async_runtime::block_on(async {
            let router = Router::new()
                .route(
                    "/connections",
                    delete(|State(calls): State<Arc<Calls>>| async move {
                        calls.events.lock().unwrap().push("close");
                        if calls.hold_close.load(Ordering::SeqCst) {
                            calls.entered.notify_one();
                            calls.release.notified().await;
                        }
                        if calls.fail_close.load(Ordering::SeqCst) {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::NO_CONTENT
                        }
                    }),
                )
                .with_state(calls.clone());
            crate::core::actor_v2::api::tests::server(router).await
        });
        let endpoint = Arc::new(Endpoint {
            delegate: if fail_reconcile {
                TestControlEndpoint::failing()
            } else {
                TestControlEndpoint::succeeding()
            },
            binding: Mutex::new(Some(CoreApiConnection {
                instance_id: "source".into(),
                controller: CoreControllerInfo::Http(url),
                secret: None,
            })),
            replace: AtomicBool::new(false),
            report_restart: AtomicBool::new(false),
            calls: calls.clone(),
            api_queries: AtomicUsize::new(0),
        });
        let dir = tempfile::tempdir().unwrap();
        let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
        args.binary_installer = installer;
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        Self {
            client,
            endpoint,
            calls,
            server,
            _dir: dir,
        }
    }
}
fn mode_patch() -> ClashGuardOverridesPatch {
    ClashGuardOverridesPatch {
        mode: Some(Mode::Global),
        ..Default::default()
    }
}

#[test]
fn mode_interruption_follows_same_instance_reconcile_once() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        let repeated = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(repeated.degradations().is_empty());
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile", "close"]
        );
    });
}

#[test]
fn disabled_policy_and_non_mode_patches_do_not_close_connections() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let outcome = f
            .client
            .patch_runtime_overrides(ClashGuardOverridesPatch {
                ipv6: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        let mut config = f.client.get_clash_config().await.unwrap();
        config.break_connection.on_mode_change = false;
        f.client.replace_clash_config(config).await.unwrap();
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "reconcile"]);
    });
}

#[test]
fn failed_reconcile_never_closes_connections() {
    let f = Fixture::new(true);
    tauri::async_runtime::block_on(async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(outcome.degradations()[0].code, "config_reconcile_failed");
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
    });
}

#[test]
fn replacement_at_the_same_url_never_receives_source_interruption() {
    for reported in [false, true] {
        let f = Fixture::new(false);
        f.endpoint.replace.store(true, Ordering::SeqCst);
        f.endpoint.report_restart.store(reported, Ordering::SeqCst);
        tauri::async_runtime::block_on(async {
            let outcome = f
                .client
                .patch_runtime_overrides(mode_patch())
                .await
                .unwrap();
            if reported {
                assert!(outcome.degradations().is_empty());
            } else {
                assert_eq!(outcome.degradations()[0].code, "mode_interruption_failed");
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        });
    }
}

#[test]
fn close_failure_is_committed_degraded_and_not_replayed() {
    let f = Fixture::new(false);
    f.calls.fail_close.store(true, Ordering::SeqCst);
    tauri::async_runtime::block_on(async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(outcome.degradations()[0].code, "mode_interruption_failed");
        assert!(!outcome.degradations()[0].retryable);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        assert_eq!(
            serde_json::to_value(f.client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
            "global"
        );
    });
}

#[test]
fn missing_source_is_degraded_but_confirmed_stopped_startup_needs_no_close() {
    for stopped in [false, true] {
        let f = Fixture::new(false);
        *f.endpoint.binding.lock().unwrap() = None;
        if stopped {
            f.endpoint
                .delegate
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
        }
        tauri::async_runtime::block_on(async {
            let outcome = f
                .client
                .patch_runtime_overrides(mode_patch())
                .await
                .unwrap();
            if stopped {
                assert!(outcome.degradations().is_empty());
            } else {
                assert_eq!(outcome.degradations()[0].code, "mode_interruption_failed");
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        });
    }
}

#[test]
fn lifecycle_work_cannot_overtake_pending_interruption() {
    let f = Fixture::new(false);
    f.calls.hold_close.store(true, Ordering::SeqCst);
    tauri::async_runtime::block_on(async {
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.patch_runtime_overrides(mode_patch()).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let mut next = Box::pin(f.client.patch_runtime_overrides(ClashGuardOverridesPatch {
            ipv6: Some(true),
            ..Default::default()
        }));
        assert!(next.as_mut().now_or_never().is_none());
        f.client
            .inner
            .application_workflow
            .0
            .actor
            .call(
                super::Message::Barrier,
                Some(std::time::Duration::from_secs(5)),
            )
            .await
            .unwrap();
        assert_eq!(f.client.inner.application_workflow.status().queued.len(), 1);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(next.await.unwrap().degradations().is_empty());
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile"]
        );
    });
}

#[test]
fn controller_rotation_invalidates_source_without_closing_through_new_credentials() {
    let f = Fixture::new(false);
    // Simulate an independently retired source lease, not a confirmed process replacement.
    f.endpoint.calls.hold_close.store(true, Ordering::SeqCst);
    tauri::async_runtime::block_on(async {
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.patch_runtime_overrides(mode_patch()).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        f.endpoint.binding.lock().unwrap().as_mut().unwrap().secret = Some("rotated".into());
        f.calls.release.notify_one();
        let outcome = first.await.unwrap().unwrap();
        assert_eq!(outcome.degradations()[0].code, "mode_interruption_failed");
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
    });
}

async fn add_profile(f: &Fixture) -> nyanpasu_config::profile::ProfileId {
    f.client
        .add_profile(
            crate::client::tests::minimal_file_profile_request(),
            Some("proxies: []\n".into()),
        )
        .await
        .unwrap()
        .into_value()
}

#[test]
fn profile_activation_and_deselection_interrupt_only_actual_current_changes() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        assert!(
            f.client
                .activate_profile(Some(uid.clone()))
                .await
                .unwrap()
                .degradations()
                .is_empty()
        );
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        f.calls.events.lock().unwrap().clear();
        assert!(
            f.client
                .activate_profile(Some(uid.clone()))
                .await
                .unwrap()
                .degradations()
                .is_empty()
        );
        let other = add_profile(&f).await;
        assert!(
            f.client
                .delete_profile(other)
                .await
                .unwrap()
                .degradations()
                .is_empty()
        );
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert!(f.client.delete_profile(uid.clone()).await.is_err());
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert_eq!(f.client.get_profiles().await.unwrap().current, Some(uid));
        assert!(
            f.client
                .activate_profile(None)
                .await
                .unwrap()
                .degradations()
                .is_empty()
        );
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        assert!(f.client.get_profiles().await.unwrap().current.is_none());
    });
}

#[test]
fn profile_autoactivation_uses_policy_and_does_not_interrupt_an_existing_selection() {
    for enabled in [false, true] {
        let f = Fixture::new(false);
        tauri::async_runtime::block_on(async {
            let mut config = f.client.get_clash_config().await.unwrap();
            config.break_connection.on_profile_change = enabled;
            f.client.replace_clash_config(config).await.unwrap();
            let first = f
                .client
                .create_profile(
                    crate::client::tests::minimal_file_profile_request(),
                    Some("proxies: []\n".into()),
                )
                .await
                .unwrap();
            assert!(first.degradations().is_empty());
            assert_eq!(
                *f.calls.events.lock().unwrap(),
                if enabled {
                    vec!["reconcile", "close"]
                } else {
                    vec!["reconcile"]
                }
            );
            f.calls.events.lock().unwrap().clear();
            f.client
                .create_profile(
                    crate::client::tests::minimal_file_profile_request(),
                    Some("proxies: []\n".into()),
                )
                .await
                .unwrap();
            assert_eq!(
                f.client.get_profiles().await.unwrap().current,
                Some(first.into_value())
            );
            assert!(f.calls.events.lock().unwrap().is_empty());
        });
    }
}

#[test]
fn profile_reconcile_and_interruption_failures_keep_the_committed_selection() {
    for fail_reconcile in [false, true] {
        let f = Fixture::new(fail_reconcile);
        f.calls.fail_close.store(true, Ordering::SeqCst);
        tauri::async_runtime::block_on(async {
            let uid = add_profile(&f).await;
            let outcome = f.client.activate_profile(Some(uid.clone())).await.unwrap();
            assert_eq!(outcome.degradations().len(), 1);
            assert_eq!(
                outcome.degradations()[0].code,
                if fail_reconcile {
                    "runtime_rebuild_failed"
                } else {
                    "profile_interruption_failed"
                }
            );
            assert_eq!(f.client.get_profiles().await.unwrap().current, Some(uid));
            assert_eq!(
                *f.calls.events.lock().unwrap(),
                if fail_reconcile {
                    vec!["reconcile"]
                } else {
                    vec!["reconcile", "close"]
                }
            );
        });
    }
}

#[test]
fn profile_replacement_never_receives_source_interruption() {
    for reported in [false, true] {
        let f = Fixture::new(false);
        f.endpoint.replace.store(true, Ordering::SeqCst);
        f.endpoint.report_restart.store(reported, Ordering::SeqCst);
        tauri::async_runtime::block_on(async {
            let uid = add_profile(&f).await;
            let outcome = f.client.activate_profile(Some(uid)).await.unwrap();
            if reported {
                assert!(outcome.degradations().is_empty());
            } else {
                assert_eq!(
                    outcome.degradations()[0].code,
                    "profile_interruption_failed"
                );
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        });
    }
}

#[test]
fn profile_mutations_cannot_overtake_pending_interruption() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        f.calls.hold_close.store(true, Ordering::SeqCst);
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.activate_profile(Some(uid)).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let mut next = Box::pin(f.client.activate_profile(None));
        assert!(next.as_mut().now_or_never().is_none());
        f.client
            .inner
            .application_workflow
            .0
            .actor
            .call(
                super::Message::Barrier,
                Some(std::time::Duration::from_secs(5)),
            )
            .await
            .unwrap();
        assert_eq!(f.client.inner.application_workflow.status().queued.len(), 1);
        assert!(f.client.get_profiles().await.unwrap().current.is_some());
        f.calls.hold_close.store(false, Ordering::SeqCst);
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(next.await.unwrap().degradations().is_empty());
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile", "close"]
        );
    });
}

#[test]
fn rejected_profile_activation_does_not_reconcile_or_interrupt() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        assert!(
            f.client
                .activate_profile(Some(nyanpasu_config::profile::ProfileId("missing".into())))
                .await
                .is_err()
        );
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert!(f.client.get_profiles().await.unwrap().current.is_none());
    });
}

#[test]
fn profile_missing_source_is_degraded_but_stopped_core_needs_no_interruption() {
    for stopped in [false, true] {
        let f = Fixture::new(false);
        *f.endpoint.binding.lock().unwrap() = None;
        if stopped {
            f.endpoint
                .delegate
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
        }
        tauri::async_runtime::block_on(async {
            let uid = add_profile(&f).await;
            let outcome = f.client.activate_profile(Some(uid)).await.unwrap();
            if stopped {
                assert!(outcome.degradations().is_empty());
            } else {
                assert_eq!(
                    outcome.degradations()[0].code,
                    "profile_interruption_failed"
                );
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        });
    }
}

#[test]
fn profile_policy_and_noop_gates_do_not_acquire_a_source() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        let mut config = f.client.get_clash_config().await.unwrap();
        config.break_connection.on_profile_change = false;
        f.client.replace_clash_config(config.clone()).await.unwrap();
        f.client.activate_profile(Some(uid.clone())).await.unwrap();
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);

        config.break_connection.on_profile_change = true;
        f.client.replace_clash_config(config).await.unwrap();
        f.client.activate_profile(Some(uid)).await.unwrap();
        f.client
            .create_profile(
                crate::client::tests::minimal_file_profile_request(),
                Some("proxies: []\n".into()),
            )
            .await
            .unwrap();
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
    });
}

#[test]
fn cancelled_profile_waiter_keeps_admission_and_dirty_does_not_replay_interruption() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        f.calls.hold_close.store(true, Ordering::SeqCst);
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.activate_profile(Some(uid)).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let workflow = &f.client.inner.application_workflow;
        let active = workflow.status().active.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        let mut dirty =
            Box::pin(workflow.call(super::Command::Core(super::CoreCommand::RuntimeDirty)));
        assert!(dirty.as_mut().now_or_never().is_none());
        let mut next = Box::pin(f.client.activate_profile(None));
        assert!(next.as_mut().now_or_never().is_none());
        super::barrier(workflow).await;
        assert_eq!(workflow.status().active, Some(active));
        assert_eq!(workflow.status().queued.len(), 2);
        assert!(f.client.get_profiles().await.unwrap().current.is_some());
        f.calls.hold_close.store(false, Ordering::SeqCst);
        f.calls.release.notify_one();
        dirty.await.unwrap();
        assert!(next.await.unwrap().degradations().is_empty());
        assert!(
            workflow
                .status()
                .completed
                .iter()
                .any(|result| result.id == active && result.error.is_none())
        );
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile", "reconcile", "close"]
        );
    });
}

#[test]
fn shutdown_rejects_a_queued_profile_before_commit_and_waits_for_close() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        f.calls.hold_close.store(true, Ordering::SeqCst);
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.activate_profile(Some(uid)).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let mut next = Box::pin(f.client.activate_profile(None));
        assert!(next.as_mut().now_or_never().is_none());
        let mut shutdown = Box::pin(f.client.shutdown_core());
        assert!(shutdown.as_mut().now_or_never().is_none());
        super::barrier(&f.client.inner.application_workflow).await;
        assert!(next.await.is_err());
        assert!(f.client.get_profiles().await.unwrap().current.is_some());
        assert!(shutdown.as_mut().now_or_never().is_none());
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(shutdown.await.stop.is_ok());
        assert_eq!(
            f.calls
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "close")
                .count(),
            1
        );
    });
}

#[test]
fn full_queue_rejects_profile_without_changing_selection() {
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        f.calls.hold_close.store(true, Ordering::SeqCst);
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.activate_profile(Some(uid)).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let workflow = &f.client.inner.application_workflow;
        for _ in 0..super::MAX_PENDING {
            workflow
                .0
                .actor
                .cast(super::Message::Request(super::Request {
                    command: super::Command::Core(super::CoreCommand::Reconcile),
                    response: super::Response {
                        id: OperationId::generate(),
                        reply: None,
                    },
                }))
                .unwrap();
        }
        super::barrier(workflow).await;
        assert_eq!(workflow.status().queued.len(), super::MAX_PENDING);
        let error = workflow.activate_profile(None).await.unwrap_err();
        assert_eq!(
            error.kind,
            Some(nyanpasu_core_manager::CoreErrorKind::OperationConflict)
        );
        assert!(f.client.get_profiles().await.unwrap().current.is_some());
        let mut shutdown = Box::pin(f.client.shutdown_core());
        assert!(shutdown.as_mut().now_or_never().is_none());
        super::barrier(workflow).await;
        f.calls.release.notify_one();
        first.await.unwrap().unwrap();
        assert!(shutdown.await.stop.is_ok());
    });
}

#[derive(Default)]
struct CountingInstaller(AtomicUsize);

#[async_trait::async_trait]
impl crate::client::core_lifecycle::ports::BinaryInstaller for CountingInstaller {
    async fn install(
        &self,
        _: &crate::client::core_lifecycle::ports::PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn profile_interruption_serializes_mode_host_and_binary_operations() {
    let installer = Arc::new(CountingInstaller::default());
    let f = Fixture::with_installer(false, installer.clone());
    tauri::async_runtime::block_on(async {
        let uid = add_profile(&f).await;
        f.calls.hold_close.store(true, Ordering::SeqCst);
        let first = {
            let client = f.client.clone();
            tokio::spawn(async move { client.activate_profile(Some(uid)).await })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            f.calls.entered.notified(),
        )
        .await
        .unwrap();
        let before = serde_json::to_value(f.client.get_clash_config().await.unwrap().overrides)
            .unwrap()["mode"]
            .clone();
        let next_mode = if before == "global" {
            Mode::Rule
        } else {
            Mode::Global
        };
        let mut mode = Box::pin(f.client.patch_runtime_overrides(ClashGuardOverridesPatch {
            mode: Some(next_mode),
            ..Default::default()
        }));
        assert!(mode.as_mut().now_or_never().is_none());
        let mut host = Box::pin(f.client.change_execution_host(ExecutionHost::Local));
        assert!(host.as_mut().now_or_never().is_none());
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let progress = Arc::new(super::Progress::default());
        let artifact = crate::client::core_lifecycle::ports::PreparedCoreBinary {
            target: f.client.get_app_config().await.unwrap().core.into(),
            source: staging.path().join("prepared-core"),
            destination: f._dir.path().join("installed-core"),
            staging,
            progress: progress.clone(),
        };
        let mut install = Box::pin(f.client.replace_core_binary(artifact));
        assert!(install.as_mut().now_or_never().is_none());
        super::barrier(&f.client.inner.application_workflow).await;
        assert_eq!(f.client.inner.application_workflow.status().queued.len(), 3);
        assert_eq!(
            serde_json::to_value(f.client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
            before
        );
        assert_eq!(installer.0.load(Ordering::SeqCst), 0);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        f.calls.hold_close.store(false, Ordering::SeqCst);
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(mode.await.unwrap().degradations().is_empty());
        host.await.unwrap();
        install.await.unwrap();
        assert_eq!(
            serde_json::to_value(f.client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
            serde_json::to_value(next_mode).unwrap()
        );
        assert_eq!(installer.0.load(Ordering::SeqCst), 1);
        assert!(progress.0.load(Ordering::SeqCst));
    });
}

struct RecordingBuilder {
    delegate: super::adapters::FsRuntimeBuildAdapter,
    inputs: Mutex<
        Vec<(
            Arc<nyanpasu_config::profile::Profiles>,
            nyanpasu_config::clash::config::ClashConfig,
        )>,
    >,
    fail_build: bool,
    fail_publish: bool,
}

#[async_trait::async_trait]
impl super::ports::RuntimeBuildPort for RecordingBuilder {
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<nyanpasu_core_manager::CoreSpec> {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: crate::client::runtime::RuntimeRevision,
        profiles: Arc<nyanpasu_config::profile::Profiles>,
        clash: nyanpasu_config::clash::config::ClashConfig,
        app: nyanpasu_config::application::NyanpasuAppConfig,
    ) -> anyhow::Result<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inputs
            .lock()
            .unwrap()
            .push((profiles.clone(), clash.clone()));
        anyhow::ensure!(!self.fail_build, "scripted build failure");
        self.delegate.build(revision, profiles, clash, app).await
    }
    async fn publish(
        &self,
        snapshot: &crate::client::runtime::RuntimeSnapshot,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fail_publish, "scripted publish failure");
        self.delegate.publish(snapshot).await
    }
}

impl RecordingBuilder {
    fn new(f: &Fixture, fail_build: bool, fail_publish: bool) -> Arc<Self> {
        Arc::new(Self {
            delegate: super::adapters::FsRuntimeBuildAdapter {
                profiles_dir: f.client.inner.profiles_dir.clone(),
                paths: f.client.inner.runtime_paths.clone(),
                ports: f.client.inner.ports.clone(),
            },
            inputs: Mutex::new(Vec::new()),
            fail_build,
            fail_publish,
        })
    }
}

#[test]
fn committed_runtime_inputs_survive_newer_profile_and_channel_state() {
    use crate::client::core_lifecycle::ports::RuntimePreparationPort;
    use nyanpasu_config::clash::config::ClashControlChannel;
    let f = Fixture::new(false);
    tauri::async_runtime::block_on(async {
        let first = add_profile(&f).await;
        let second = add_profile(&f).await;
        let committed = f
            .client
            .inner
            .profiles
            .set_current(Some(first.clone()))
            .await
            .unwrap();
        f.client
            .inner
            .profiles
            .set_current(Some(second.clone()))
            .await
            .unwrap();
        let mut original = f.client.get_clash_config().await.unwrap();
        original.clash_control_channel = ClashControlChannel::HttpOnly;
        original.clash_ipc_disable_http_controller = false;
        let mut newer = original.clone();
        newer.clash_control_channel = ClashControlChannel::PreferIpc;
        newer.clash_ipc_disable_http_controller = true;
        f.client.replace_clash_config(newer).await.unwrap();
        let builder = RecordingBuilder::new(&f, false, false);
        let mut preparation = super::RuntimePreparation::new(
            f.client.inner.application.clone(),
            f.client.inner.clash_config.clone(),
            f.client.inner.profiles.clone(),
            builder.clone(),
        );
        let prepared = preparation
            .prepare_committed(committed.snapshot.clone(), original)
            .await
            .unwrap();
        assert_eq!(
            prepared.local_ipc.policy,
            nyanpasu_core_manager::LocalIpcPolicy::Disable
        );
        assert!(prepared.local_ipc.keep_http_controller);
        assert_eq!(prepared.snapshot.revision.get(), 1);
        let latest = preparation.prepare_latest().await.unwrap();
        assert_eq!(
            latest.local_ipc.policy,
            nyanpasu_core_manager::LocalIpcPolicy::Prefer
        );
        assert!(!latest.local_ipc.keep_http_controller);
        assert_eq!(latest.snapshot.revision.get(), 2);
        let inputs = builder.inputs.lock().unwrap();
        assert!(Arc::ptr_eq(&inputs[0].0, &committed.snapshot));
        assert_eq!(inputs[0].0.current, Some(first));
        assert_eq!(
            inputs[0].1.clash_control_channel,
            ClashControlChannel::HttpOnly
        );
        assert_eq!(inputs[1].0.current, Some(second));
        assert_eq!(
            inputs[1].1.clash_control_channel,
            ClashControlChannel::PreferIpc
        );
    });
}

#[test]
fn failed_profile_build_or_publish_preserves_commit_without_reconcile_or_close() {
    for publish in [false, true] {
        let f = Fixture::new(false);
        tauri::async_runtime::block_on(async {
            let uid = add_profile(&f).await;
            let builder = RecordingBuilder::new(&f, !publish, publish);
            let workflow = super::ApplicationWorkflowClient::spawn_with_ticks(
                super::ApplicationWorkflowArgs {
                    snapshots: crate::client::runtime::RuntimeSnapshotStore::default(),
                    application: f.client.inner.application.clone(),
                    clash: f.client.inner.clash_config.clone(),
                    profiles: f.client.inner.profiles.clone(),
                    core: super::CoreClient::spawn(f.endpoint.clone()).await.unwrap(),
                    service: super::ServiceClient::spawn(
                        Arc::new(crate::client::tests::IdleServiceAdapter),
                        0,
                    )
                    .await
                    .unwrap(),
                    builder,
                    installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
                    ui: Arc::new(crate::client::NoopUiEventSink),
                    dirty: super::DirtyNotifier::channel().1,
                },
                false,
            )
            .await
            .unwrap();
            let outcome = workflow.activate_profile(Some(uid.clone())).await.unwrap();
            assert_eq!(outcome.degradations().len(), 1);
            assert_eq!(outcome.degradations()[0].code, "runtime_rebuild_failed");
            assert_eq!(f.client.get_profiles().await.unwrap().current, Some(uid));
            assert!(f.calls.events.lock().unwrap().is_empty());
            assert!(workflow.runtime().promoted.is_none());
            assert!(workflow.runtime().applied.is_none());
            assert!(!workflow.status().uncertain);
            workflow.shutdown().await.unwrap();
        });
    }
}
