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
use nyanpasu_updater::PreparedCoreBinary;
use struct_patch::Patch as _;
use tokio::sync::Notify;

use crate::{
    actor_v2::endpoint::{
        CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
    },
    client::{
        NyanpasuClient,
        test_support::{
            TestControlEndpoint, runtime_build::TestRuntimeBuildPort,
            test_client_args_with_endpoint, workflow::Installer,
        },
    },
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;

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
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        self.delegate.check_config(submission).await
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
    async fn new(fail_reconcile: bool) -> Self {
        Self::with_installer(fail_reconcile, Arc::new(Installer::succeeding())).await
    }

    async fn with_installer(
        fail_reconcile: bool,
        installer: Arc<dyn nyanpasu_contracts::core_install::BinaryInstaller>,
    ) -> Self {
        Self::with_fetcher(fail_reconcile, installer, None).await
    }

    async fn with_fetcher(
        fail_reconcile: bool,
        installer: Arc<dyn nyanpasu_contracts::core_install::BinaryInstaller>,
        fetcher: Option<Arc<dyn nyanpasu_contracts::profiles::ports::SubscriptionFetcher>>,
    ) -> Self {
        let calls = Arc::new(Calls::default());
        let (url, server) = (async {
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
            crate::actor_v2::test_support::server(router).await
        })
        .await;
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
        let mut adapters = crate::client::test_support::profile_test_adapters(&dir);
        if let Some(fetcher) = fetcher {
            let factory = adapters.profile_files.clone();
            adapters.profile_files = Arc::new(move |ports| {
                let mut files = factory(ports);
                files.fetcher = fetcher.clone();
                files
            });
        }
        let mut args = test_client_args_with_endpoint(&dir, endpoint.clone(), adapters).await;
        args.binary_installer = installer;
        let client = NyanpasuClient::try_new(args).await.unwrap();
        endpoint.delegate.prime(&client).await;
        calls.events.lock().unwrap().clear();
        endpoint.api_queries.store(0, Ordering::SeqCst);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mode_interruption_follows_same_instance_reconcile_once() {
    let f = Fixture::new(false).await;
    (async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        let version = f.client.inner.clash_config.snapshot().version;
        let checks = f.endpoint.delegate.checked().len();
        let repeated = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(repeated.degradations().is_empty());
        assert_eq!(f.client.inner.clash_config.snapshot().version, version);
        assert_eq!(f.endpoint.delegate.checked().len(), checks);
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "close"]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_policy_and_non_mode_patches_do_not_close_connections() {
    let f = Fixture::new(false).await;
    (async {
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
        f.client
            .patch_clash_config(nyanpasu_config::clash::config::ClashConfigPatch {
                break_connection: config.break_connection.into_patch(),
                ..Default::default()
            })
            .await
            .unwrap();
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        // Setting the mode again closes nothing while the policy is off, and
        // applies nothing either. The API query count is not asserted: every
        // overrides patch also asks for a proxy refresh, which reads the API
        // on its own schedule.
        f.client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "reconcile"]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_reconcile_never_closes_connections() {
    let f = Fixture::new(true).await;
    (async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert!(matches!(
            outcome.degradations()[0].reason,
            nyanpasu_runtime_values::runtime::DegradationReason::RuntimeDeferred { .. }
        ));
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacement_at_the_same_url_never_receives_source_interruption() {
    for reported in [false, true] {
        let f = Fixture::new(false).await;
        f.endpoint.replace.store(true, Ordering::SeqCst);
        f.endpoint.report_restart.store(reported, Ordering::SeqCst);
        (async {
            let outcome = f
                .client
                .patch_runtime_overrides(mode_patch())
                .await
                .unwrap();
            if reported {
                assert!(outcome.degradations().is_empty());
            } else {
                assert!(matches!(
                    outcome.degradations()[0].reason,
                    nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
                ));
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_failure_is_committed_degraded_and_not_replayed() {
    let f = Fixture::new(false).await;
    f.calls.fail_close.store(true, Ordering::SeqCst);
    (async {
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert!(matches!(
            outcome.degradations()[0].reason,
            nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
        ));
        assert!(!outcome.degradations()[0].retryable);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        assert_eq!(
            serde_json::to_value(f.client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
            "global"
        );
    }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_source_is_degraded_but_confirmed_stopped_startup_needs_no_close() {
    for stopped in [false, true] {
        let f = Fixture::new(false).await;
        *f.endpoint.binding.lock().unwrap() = None;
        if stopped {
            f.endpoint
                .delegate
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
        }
        (async {
            let outcome = f
                .client
                .patch_runtime_overrides(mode_patch())
                .await
                .unwrap();
            if stopped {
                assert!(outcome.degradations().is_empty());
            } else {
                assert!(matches!(
                    outcome.degradations()[0].reason,
                    nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
                ));
            }
            assert_eq!(
                *f.calls.events.lock().unwrap(),
                if stopped {
                    Vec::<&str>::new()
                } else {
                    vec!["reconcile"]
                }
            );
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_work_cannot_overtake_pending_interruption() {
    let f = Fixture::new(false).await;
    f.calls.hold_close.store(true, Ordering::SeqCst);
    (async {
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
        let next = {
            let client = f.client.clone();
            tokio::spawn(async move {
                client
                    .patch_runtime_overrides(ClashGuardOverridesPatch {
                        ipv6: Some(true),
                        ..Default::default()
                    })
                    .await
            })
        };
        assert!(!next.is_finished());

        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(next.await.unwrap().unwrap().degradations().is_empty());
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile"]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn controller_rotation_invalidates_source_without_closing_through_new_credentials() {
    let f = Fixture::new(false).await;
    // Simulate an independently retired source lease, not a confirmed process replacement.
    f.endpoint.calls.hold_close.store(true, Ordering::SeqCst);
    (async {
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
        assert!(matches!(
            outcome.degradations()[0].reason,
            nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
        ));
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
    })
    .await;
}

async fn add_profile(f: &Fixture) -> nyanpasu_config::profile::ProfileId {
    f.client
        .add_profile(
            crate::client::test_support::minimal_file_profile_request(),
            Some("proxies: []\n".into()),
        )
        .await
        .unwrap()
        .into_value()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_activation_and_deselection_interrupt_only_actual_current_changes() {
    let f = Fixture::new(false).await;
    (async {
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
        f.calls.events.lock().unwrap().clear();
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
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_autoactivation_uses_policy_and_does_not_interrupt_an_existing_selection() {
    for enabled in [false, true] {
        let f = Fixture::new(false).await;
        (async {
            let mut config = f.client.get_clash_config().await.unwrap();
            config.break_connection.on_profile_change = enabled;
            f.client
                .patch_clash_config(nyanpasu_config::clash::config::ClashConfigPatch {
                    break_connection: config.break_connection.into_patch(),
                    ..Default::default()
                })
                .await
                .unwrap();
            let first = f
                .client
                .create_profile(
                    crate::client::test_support::minimal_file_profile_request(),
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
                    crate::client::test_support::minimal_file_profile_request(),
                    Some("proxies: []\n".into()),
                )
                .await
                .unwrap();
            assert_eq!(
                f.client.get_profiles().await.unwrap().current,
                Some(first.into_value())
            );
            assert!(f.calls.events.lock().unwrap().is_empty());
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_apply_failure_and_interruption_failure_preserve_commit() {
    for fail_reconcile in [false, true] {
        let f = Fixture::new(fail_reconcile).await;
        f.calls.fail_close.store(!fail_reconcile, Ordering::SeqCst);
        (async {
            let uid = add_profile(&f).await;
            let old_revision = f.endpoint.status().await.unwrap().revision;
            let result = f.client.activate_profile(Some(uid.clone())).await;
            if fail_reconcile {
                let outcome = result.unwrap();
                assert!(matches!(outcome, nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded { ref commits, .. } if commits[0].runtime == nyanpasu_runtime_values::runtime::RuntimeCommitStatus::Deferred));
                assert_eq!(f.client.get_profiles().await.unwrap().current, Some(uid));
                assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
                assert_eq!(f.endpoint.status().await.unwrap().revision, old_revision);
            } else {
                assert!(matches!(
                    result.unwrap().degradations()[0].reason,
                    nyanpasu_runtime_values::runtime::DegradationReason::ProfileInterruptionFailed { .. }
                ));
                assert_eq!(f.client.get_profiles().await.unwrap().current, Some(uid));
                assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
            }
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_replacement_never_receives_source_interruption() {
    for reported in [false, true] {
        let f = Fixture::new(false).await;
        f.endpoint.replace.store(true, Ordering::SeqCst);
        f.endpoint.report_restart.store(reported, Ordering::SeqCst);
        (async {
            let uid = add_profile(&f).await;
            let outcome = f.client.activate_profile(Some(uid)).await.unwrap();
            if reported {
                assert!(outcome.degradations().is_empty());
            } else {
                assert!(matches!(
                    outcome.degradations()[0].reason,
                    nyanpasu_runtime_values::runtime::DegradationReason::ProfileInterruptionFailed { .. }
                ));
            }
            assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_mutations_cannot_overtake_pending_interruption() {
    let f = Fixture::new(false).await;
    (async {
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
        let report = f.client.inner.profiles.set_current(None).await.unwrap();
        let next = {
            let client = f.client.clone();
            tokio::spawn(async move { client.after_commit(&report).await })
        };
        assert!(!next.is_finished());

        // The deselection is already committed; what queues is its apply.
        assert!(f.client.get_profiles().await.unwrap().current.is_none());
        f.calls.hold_close.store(false, Ordering::SeqCst);
        f.calls.release.notify_one();
        assert!(first.await.unwrap().unwrap().degradations().is_empty());
        assert!(next.await.unwrap().degradations().is_empty());
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile", "close"]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_profile_activation_does_not_reconcile_or_interrupt() {
    let f = Fixture::new(false).await;
    (async {
        assert!(
            f.client
                .activate_profile(Some(nyanpasu_config::profile::ProfileId("missing".into())))
                .await
                .is_err()
        );
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert!(f.client.get_profiles().await.unwrap().current.is_none());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_missing_source_is_degraded_but_stopped_core_needs_no_interruption() {
    for stopped in [false, true] {
        let f = Fixture::new(false).await;
        *f.endpoint.binding.lock().unwrap() = None;
        if stopped {
            f.endpoint
                .delegate
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
        }
        (async {
            let uid = add_profile(&f).await;
            let outcome = f.client.activate_profile(Some(uid)).await.unwrap();
            if stopped {
                assert!(outcome.degradations().is_empty());
            } else {
                assert!(matches!(
                    outcome.degradations()[0].reason,
                    nyanpasu_runtime_values::runtime::DegradationReason::ProfileInterruptionFailed { .. }
                ));
            }
            assert_eq!(
                *f.calls.events.lock().unwrap(),
                if stopped {
                    Vec::<&str>::new()
                } else {
                    vec!["reconcile"]
                }
            );
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_policy_and_noop_gates_do_not_acquire_a_source() {
    let f = Fixture::new(false).await;
    (async {
        // Only this workflow may read the binding counted below. Stop background
        // consumers, then drain the lease monitor they may already have started.
        f.client.inner.geo_index.stop_for_test().await.unwrap();
        f.client.inner.proxies.stop_for_test().await.unwrap();
        f.client.inner.core_api.api_client().await.unwrap();
        f.client.inner.core_api.release_api_for_test().await;
        f.client.inner.core_api.refresh_status().await.unwrap();
        f.endpoint.api_queries.store(0, Ordering::SeqCst);

        let uid = add_profile(&f).await;
        let mut config = f.client.get_clash_config().await.unwrap();
        config.break_connection.on_profile_change = false;
        f.client
            .patch_clash_config(nyanpasu_config::clash::config::ClashConfigPatch {
                break_connection: config.break_connection.clone().into_patch(),
                ..Default::default()
            })
            .await
            .unwrap();
        f.client.activate_profile(Some(uid.clone())).await.unwrap();
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);

        config.break_connection.on_profile_change = true;
        f.client
            .patch_clash_config(nyanpasu_config::clash::config::ClashConfigPatch {
                break_connection: config.break_connection.into_patch(),
                ..Default::default()
            })
            .await
            .unwrap();
        f.client.activate_profile(Some(uid)).await.unwrap();
        f.client
            .create_profile(
                crate::client::test_support::minimal_file_profile_request(),
                Some("proxies: []\n".into()),
            )
            .await
            .unwrap();
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);

        // The source remains available when a selection actually changes.
        f.client.activate_profile(None).await.unwrap();
        assert!(f.endpoint.api_queries.load(Ordering::SeqCst) > 0);
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "reconcile", "close"]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_profile_waiter_finishes_work_without_replaying_interruption() {
    let f = Fixture::new(false).await;
    let uid = add_profile(&f).await;
    f.calls.hold_close.store(true, Ordering::SeqCst);
    let first = {
        let client = f.client.clone();
        tokio::spawn(async move { client.activate_profile(Some(uid)).await })
    };
    f.calls.entered.notified().await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let report = f.client.inner.profiles.set_current(None).await.unwrap();
    assert!(f.client.get_profiles().await.unwrap().current.is_none());
    f.calls.hold_close.store(false, Ordering::SeqCst);
    f.calls.release.notify_one();
    assert!(
        f.client
            .after_commit(&report)
            .await
            .degradations()
            .is_empty()
    );
    f.client
        .inner
        .application_workflow
        .reconcile()
        .await
        .unwrap();
    assert_eq!(
        *f.calls.events.lock().unwrap(),
        ["reconcile", "close", "reconcile", "close", "reconcile"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_rejects_an_uncommitted_profile_change() {
    let f = Fixture::new(false).await;
    let uid = add_profile(&f).await;
    f.client.request_shutdown();
    assert!(matches!(
        f.client.inner.profiles.set_current(Some(uid)).await,
        Err(
            nyanpasu_runtime_values::profiles::ProfilesError::ShuttingDown
                | nyanpasu_runtime_values::profiles::ProfilesError::ProfilesActorStopped
        )
    ));
    assert!(f.client.get_profiles().await.unwrap().current.is_none());
    f.client.wait_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_reports_closed_for_a_committed_profile_waiting_to_apply() {
    let f = Fixture::new(false).await;
    let uid = add_profile(&f).await;
    f.calls.hold_close.store(true, Ordering::SeqCst);
    let first = {
        let client = f.client.clone();
        tokio::spawn(async move { client.activate_profile(Some(uid)).await })
    };
    f.calls.entered.notified().await;
    let report = f.client.inner.profiles.set_current(None).await.unwrap();
    assert!(f.client.get_profiles().await.unwrap().current.is_none());
    f.client.request_shutdown();
    let mut shutdown = Box::pin(f.client.wait_shutdown());
    assert!(shutdown.as_mut().now_or_never().is_none());
    f.calls.hold_close.store(false, Ordering::SeqCst);
    f.calls.release.notify_one();
    let outcome = f.client.after_commit(&report).await;
    assert!(matches!(&outcome.degradations()[0].reason,
        nyanpasu_runtime_values::runtime::DegradationReason::RuntimeRecoveryRequired { cause: Some(cause), .. }
        if matches!(cause.as_ref(), nyanpasu_runtime_values::runtime_error::RuntimeError::ShuttingDown)));
    assert!(
        matches!(outcome, nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded { ref commits, .. }
        if commits[0].runtime == nyanpasu_runtime_values::runtime::RuntimeCommitStatus::RecoveryRequired)
    );
    first.await.unwrap().unwrap();
    shutdown.await;
    assert_eq!(
        *f.calls.events.lock().unwrap(),
        ["reconcile", "close", "reconcile"]
    );
    assert_eq!(
        f.endpoint.delegate.reconciled_bytes().len(),
        1,
        "shutdown must not apply the queued profile change"
    );
}

#[derive(Default)]
struct CountingInstaller(AtomicUsize);

#[async_trait::async_trait]
impl nyanpasu_contracts::core_install::BinaryInstaller for CountingInstaller {
    async fn install(
        &self,
        _: &nyanpasu_contracts::core_install::CoreBinaryInstallRequest,
    ) -> Result<(), nyanpasu_contracts::core_install::InstallCoreBinaryError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_interruption_serializes_mode_host_and_binary_operations() {
    let installer = Arc::new(CountingInstaller::default());
    let f = Fixture::with_installer(false, installer.clone()).await;
    (async {
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
        let saved = f.client.inner.clash_config.patch_overrides(ClashGuardOverridesPatch {
            mode: Some(next_mode),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(saved.state.overrides.mode(), next_mode);
        let mode = {
            let client = f.client.clone();
            tokio::spawn(async move { client.clash_outcome(saved).await })
        };
        // Polling once sends each request, so both wait behind the held
        // interruption.
        let mut host = Box::pin(
            f.client
                .inner
                .application_workflow
                .change_host(ExecutionHost::Local),
        );
        assert!(host.as_mut().now_or_never().is_none());
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let progress = Arc::new(super::Progress::default());
        let artifact = PreparedCoreBinary {
            target: f.client.get_app_config().await.unwrap().core,
            source: staging.path().join("prepared-core"),
            destination: f._dir.path().join("installed-core"),
            staging,
            progress: progress.clone(),
            companion: None,
        };
        let mut install = Box::pin(f.client.inner.application_workflow.replace_binary(artifact));
        assert!(install.as_mut().now_or_never().is_none());
        // The source is committed while the runtime mailbox is still occupied.
        assert_eq!(f.client.inner.clash_config.snapshot().state.overrides.mode(), next_mode);
        assert!(!mode.is_finished());
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
    }).await;
}

struct RecordingBuilder {
    delegate: TestRuntimeBuildPort,
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
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> super::super::inputs::FrozenProfileContent {
        self.delegate.capture_content(profiles).await
    }

    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, nyanpasu_runtime_values::core_spec::CoreSpecError>
    {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: crate::client::runtime::RuntimeRevision,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<
        Arc<crate::client::runtime::RuntimeSnapshot>,
        nyanpasu_profile_transform::RuntimeBuildError,
    > {
        self.inputs
            .lock()
            .unwrap()
            .push((inputs.profiles.clone(), inputs.clash.clone()));
        if self.fail_build {
            return Err(nyanpasu_profile_transform::RuntimeBuildError::ConfigNotMapping);
        }
        self.delegate
            .build(revision, inputs, ports, strict_transforms)
            .await
    }
    async fn publish(
        &self,
        snapshot: &crate::client::runtime::RuntimeSnapshot,
    ) -> Result<(), nyanpasu_runtime_values::publish::PublishRuntimeError> {
        if self.fail_publish {
            return Err(super::scripted_publish_failure());
        }
        self.delegate.publish(snapshot).await
    }
}

impl RecordingBuilder {
    fn new(f: &Fixture, fail_build: bool, fail_publish: bool) -> Arc<Self> {
        Arc::new(Self {
            delegate: TestRuntimeBuildPort::from_profile_port(
                f.client.inner.fs.clone(),
                crate::client::test_support::test_runtime_paths(&f._dir),
            ),
            inputs: Mutex::new(Vec::new()),
            fail_build,
            fail_publish,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_runtime_inputs_survive_newer_profile_and_channel_state() {
    use nyanpasu_config::clash::config::ClashControlChannel;
    let f = Fixture::new(false).await;
    (async {
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
        let mut newer = nyanpasu_config::clash::config::ClashConfig::new_empty_patch();
        newer.clash_control_channel = Some(ClashControlChannel::PreferIpc);
        newer.clash_ipc_disable_http_controller = Some(true);
        f.client.patch_clash_config(newer).await.unwrap();
        let builder = RecordingBuilder::new(&f, false, false);
        let preparation = super::RuntimePreparation::new(
            f.client.inner.profiles.snapshot_handle(),
            f.client.inner.profiles.content_lock(),
            builder.clone(),
            f.client.inner.ports.clone(),
        );
        let prepared = preparation
            .prepare(
                committed.snapshot.clone(),
                original,
                f.client.inner.application.snapshot().state,
            )
            .await
            .unwrap();
        assert_eq!(
            prepared.intent.local_ipc.policy,
            nyanpasu_core_manager::LocalIpcPolicy::Disable
        );
        assert!(prepared.intent.local_ipc.keep_http_controller);
        assert_eq!(prepared.snapshot.revision.get(), 1);
        let latest = preparation
            .prepare_inputs(
                preparation
                    .capture_committed_inputs(
                        f.client.inner.application.snapshot().state,
                        f.client.get_clash_config().await.unwrap(),
                    )
                    .await
                    .1,
            )
            .await
            .unwrap();
        assert_eq!(
            latest.intent.local_ipc.policy,
            nyanpasu_core_manager::LocalIpcPolicy::Prefer
        );
        assert!(!latest.intent.local_ipc.keep_http_controller);
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
    })
    .await;
}

// Guards excluding unchanged mode from interruption when another override changes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unchanged_mode_with_another_override_applies_without_interruption() {
    let f = Fixture::new(false).await;
    let mode = f.client.get_clash_config().await.unwrap().overrides.mode();
    let outcome = f
        .client
        .patch_runtime_overrides(ClashGuardOverridesPatch {
            mode: Some(mode),
            ipv6: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(outcome.degradations().is_empty());
    assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile"]);
}

// Guards the facade returning a mode-interruption degradation without committing or applying.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_mode_close_failure_is_degraded_without_a_commit() {
    let f = Fixture::new(false).await;
    f.calls.fail_close.store(true, Ordering::SeqCst);
    let before = f.client.inner.clash_config.snapshot();
    let outcome = f
        .client
        .patch_runtime_overrides(ClashGuardOverridesPatch {
            mode: Some(before.state.overrides.mode()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded { .. }
    ));
    assert_eq!(outcome.degradations().len(), 1);
    assert!(matches!(
        outcome.degradations()[0].reason,
        nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
    ));
    assert!(!outcome.degradations()[0].retryable);
    assert_eq!(
        f.client.inner.clash_config.snapshot().version,
        before.version
    );
    assert!(f.endpoint.delegate.checked().is_empty());
    assert_eq!(*f.calls.events.lock().unwrap(), ["close"]);
}

// Guards InterruptConnections running inside the workflow mailbox before later runtime work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_mode_interruption_serializes_runtime_commands() {
    let f = Fixture::new(false).await;
    f.calls.hold_close.store(true, Ordering::SeqCst);
    let mode = f.client.get_clash_config().await.unwrap().overrides.mode();
    let interruption = tokio::spawn({
        let client = f.client.clone();
        async move {
            client
                .patch_runtime_overrides(ClashGuardOverridesPatch {
                    mode: Some(mode),
                    ..Default::default()
                })
                .await
        }
    });
    f.calls.entered.notified().await;
    let mut reconcile = Box::pin(f.client.reconcile_core());
    assert!(reconcile.as_mut().now_or_never().is_none());
    assert_eq!(*f.calls.events.lock().unwrap(), ["close"]);
    f.calls.release.notify_one();
    assert!(
        interruption
            .await
            .unwrap()
            .unwrap()
            .degradations()
            .is_empty()
    );
    reconcile.await.unwrap();
    assert_eq!(*f.calls.events.lock().unwrap(), ["close", "reconcile"]);
}

// The endpoint scripts validation verdicts; it does not validate documents or
// require a prior check before accepting a reconcile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_profile_activation_is_refused_before_committing() {
    let f = Fixture::new(false).await;
    let uid = add_profile(&f).await;
    let before = f.client.get_profiles().await.unwrap();
    f.endpoint
        .delegate
        .set_check_answer(crate::client::test_support::TestCheckAnswer::Reject(
            CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::ConfigCheckFailed,
                "invalid profile",
                false,
            ),
        ));
    assert!(matches!(
        f.client.activate_profile(Some(uid)).await,
        Err(crate::client::ClientError::Profiles(
            nyanpasu_runtime_values::profiles::ProfilesError::Commit {
                source: nyanpasu_runtime_values::mutation::CommitAborted::RuntimeRefused { .. },
                ..
            }
        ))
    ));
    let after = f.client.get_profiles().await.unwrap();
    assert_eq!(after.current, before.current);
    assert_eq!(after.revision(), before.revision());
    assert!(f.calls.events.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_subscription_refresh_preserves_files_and_records_rejection() {
    use nyanpasu_contracts::profiles::ports::{FetchedSubscription, MockSubscriptionFetcher};
    let mut fetcher = MockSubscriptionFetcher::new();
    let fetched = AtomicUsize::new(0);
    fetcher.expect_fetch().times(2).returning(move |_, _| {
        Ok(FetchedSubscription {
            content: if fetched.fetch_add(1, Ordering::SeqCst) == 0 {
                "proxies: []\n".into()
            } else {
                "proxies: []\nmode: unsupported\n".into()
            },
            filename: None,
            subscription: Default::default(),
            suggested_update_interval_minutes: None,
            content_type: None,
            file_name: None,
        })
    });
    let f = Fixture::with_fetcher(
        false,
        Arc::new(Installer::succeeding()),
        Some(Arc::new(fetcher)),
    )
    .await;
    let uid = f
        .client
        .import_profile(
            "https://example.com/profile".parse().unwrap(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .into_value();
    let before = f.client.get_profiles().await.unwrap();
    let document = std::fs::read(f._dir.path().join("profiles.yaml")).unwrap();
    let bytes = f.client.read_profile_file(uid.clone()).await.unwrap();
    f.calls.events.lock().unwrap().clear();
    f.endpoint
        .delegate
        .set_check_answer(crate::client::test_support::TestCheckAnswer::Reject(
            CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::ConfigCheckFailed,
                "subscription rejected",
                false,
            ),
        ));
    assert!(matches!(
        f.client.refresh_profile(uid.clone(), None).await,
        Err(crate::client::ClientError::Profiles(
            nyanpasu_runtime_values::profiles::ProfilesError::Commit {
                source: nyanpasu_runtime_values::mutation::CommitAborted::RuntimeRefused { .. },
                ..
            }
        ))
    ));
    assert_eq!(
        f.client.get_profiles().await.unwrap().revision(),
        before.revision()
    );
    assert_eq!(
        std::fs::read(f._dir.path().join("profiles.yaml")).unwrap(),
        document
    );
    assert_eq!(
        f.client.read_profile_file(uid.clone()).await.unwrap(),
        bytes
    );
    let sources = f.client.inner.profiles.sources();
    let status = sources
        .entries
        .iter()
        .find(|entry| entry.profile == uid)
        .unwrap();
    assert!(
        matches!(&status.outcome, nyanpasu_runtime_values::configuration::SourceOutcome::Rejected { message, .. } if message.contains("subscription rejected"))
    );
    assert!(f.calls.events.lock().unwrap().is_empty());
}

// Guards acquiring the source before a deferred retry and updating selection even when close fails.
#[tokio::test]
async fn deferred_mode_change_interrupts_connections_once_on_retry() {
    for fail_close in [false, true] {
        let f = Fixture::new(false).await;
        let previous = f.endpoint.delegate.status().await.unwrap();
        f.endpoint.delegate.set_status(
            Some(CoreStateDetail::Starting { epoch: 2 }),
            previous.applied_kind,
        );
        let outcome = f
            .client
            .patch_runtime_overrides(mode_patch())
            .await
            .unwrap();
        assert_eq!(
            super::commit_first::receipt(&outcome).runtime,
            nyanpasu_runtime_values::runtime::RuntimeCommitStatus::Deferred
        );
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);
        f.endpoint
            .delegate
            .set_status(previous.state, previous.applied_kind);
        f.calls.fail_close.store(fail_close, Ordering::SeqCst);
        f.client
            .inner
            .application_workflow
            .retry_runtime()
            .await
            .unwrap();
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        let status = f
            .client
            .inner
            .application_workflow
            .settled(
                nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
                f.client.inner.clash_config.snapshot().version,
            )
            .await;
        assert_eq!(status.conclusion.label(), "Converged");
        assert_eq!(status.degradations.len(), usize::from(fail_close));
        if fail_close {
            assert!(matches!(
                status.degradations[0].reason,
                nyanpasu_runtime_values::runtime::DegradationReason::ModeInterruptionFailed { .. }
            ));
        }
        f.client
            .inner
            .application_workflow
            .retry_runtime()
            .await
            .unwrap();
        f.client.reconcile_core().await.unwrap();
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile"]
        );
        f.client.request_shutdown();
        f.client.wait_shutdown().await;
    }
}

// Guards the same selection comparison for Profiles regardless of which handler applies it.
#[tokio::test]
async fn deferred_profile_change_interrupts_connections_once_on_retry() {
    for fail_close in [false, true] {
        let f = Fixture::new(false).await;
        let uid = add_profile(&f).await;
        let previous = f.endpoint.delegate.status().await.unwrap();
        f.endpoint.delegate.set_status(
            Some(CoreStateDetail::Starting { epoch: 2 }),
            previous.applied_kind,
        );
        let outcome = f.client.activate_profile(Some(uid)).await.unwrap();
        assert_eq!(
            super::commit_first::receipt(&outcome).runtime,
            nyanpasu_runtime_values::runtime::RuntimeCommitStatus::Deferred
        );
        assert!(f.calls.events.lock().unwrap().is_empty());
        assert_eq!(f.endpoint.api_queries.load(Ordering::SeqCst), 0);
        f.endpoint
            .delegate
            .set_status(previous.state, previous.applied_kind);
        f.calls.fail_close.store(fail_close, Ordering::SeqCst);
        f.client
            .inner
            .application_workflow
            .retry_runtime()
            .await
            .unwrap();
        assert_eq!(*f.calls.events.lock().unwrap(), ["reconcile", "close"]);
        let status = f
            .client
            .inner
            .application_workflow
            .settled(
                nyanpasu_runtime_values::mutation::ConfigDomain::Profiles,
                f.client.inner.profiles.snapshot_handle().load().version,
            )
            .await;
        assert_eq!(status.conclusion.label(), "Converged");
        assert_eq!(status.degradations.len(), usize::from(fail_close));
        if fail_close {
            assert!(matches!(
                status.degradations[0].reason,
                nyanpasu_runtime_values::runtime::DegradationReason::ProfileInterruptionFailed { .. }
            ));
        }
        f.client
            .inner
            .application_workflow
            .retry_runtime()
            .await
            .unwrap();
        f.client.reconcile_core().await.unwrap();
        assert_eq!(
            *f.calls.events.lock().unwrap(),
            ["reconcile", "close", "reconcile"]
        );
        f.client.request_shutdown();
        f.client.wait_shutdown().await;
    }
}
