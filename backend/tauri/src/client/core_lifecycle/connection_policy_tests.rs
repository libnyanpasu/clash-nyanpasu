use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
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

use super::super::{
    NyanpasuClient,
    tests::{TestControlEndpoint, test_client_args_with_endpoint},
};
use crate::core::actor_v2::endpoint::{
    ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
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
        });
        let dir = tempfile::tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
        ))
        .unwrap();
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
            .core_lifecycle
            .0
            .actor
            .call(
                super::Message::Barrier,
                Some(std::time::Duration::from_secs(5)),
            )
            .await
            .unwrap();
        assert_eq!(f.client.inner.core_lifecycle.status().queued.len(), 1);
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
