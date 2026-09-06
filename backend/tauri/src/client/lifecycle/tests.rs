use super::{
    super::{
        NyanpasuClient,
        tests::{TestControlEndpoint, test_client_args_with_endpoint},
    },
    *,
};
use ports::{BinaryInstallProgress, PreparedCoreBinary};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use struct_patch::Patch;
use tokio::sync::Notify;

struct BlockingBuilder {
    delegate: ports::FsRuntimeBuildAdapter,
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}

#[async_trait::async_trait]
impl ports::RuntimeBuildPort for BlockingBuilder {
    fn core_spec(&self, core: &ClashCore) -> anyhow::Result<nyanpasu_core_manager::CoreSpec> {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        profiles: Arc<nyanpasu_config::profile::Profiles>,
        clash: nyanpasu_config::clash::config::ClashConfig,
        app: nyanpasu_config::application::NyanpasuAppConfig,
    ) -> anyhow::Result<Arc<runtime::RuntimeSnapshot>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.delegate.build(revision, profiles, clash, app).await
    }
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()> {
        self.delegate.publish(snapshot).await
    }
}

async fn dirty_graph(
    dir: &tempfile::TempDir,
) -> (
    CoreLifecycleClient,
    DirtyNotifier,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
) {
    use super::super::tests::{
        IdleServiceAdapter, test_materialization_port, test_typed_config_clients,
    };
    use crate::state::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
    let (application, _, clash) = test_typed_config_clients(dir).await;
    let (notifier, dirty) = DirtyNotifier::channel();
    let profiles = super::super::profiles::ProfilesClient::new(
        camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap(),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_materialization_port(),
        Arc::new(notifier.clone()),
    )
    .await
    .unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let core = CoreClient::spawn(endpoint).await.unwrap();
    let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
        .await
        .unwrap();
    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let builder = Arc::new(BlockingBuilder {
        delegate: ports::FsRuntimeBuildAdapter {
            profiles_dir: dir.path().join("profiles"),
            paths,
            ports: Arc::new(super::super::SessionPortResolver::default()),
        },
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let client = CoreLifecycleClient::spawn_with_ticks(
        LifecycleArgs {
            application: application.clone(),
            clash,
            profiles,
            core,
            service,
            builder: builder.clone(),
            installer: Arc::new(ports::FsBinaryInstaller),
            ui: Arc::new(super::super::NoopUiEventSink),
            dirty,
        },
        false,
    )
    .await
    .unwrap();
    (client, notifier, builder, application)
}

async fn tick(client: &CoreLifecycleClient) {
    client.0.actor.cast(Message::DirtyTick).unwrap();
    barrier(client).await;
}

#[tokio::test]
async fn dirty_during_build_coalesces_and_eventually_applies_the_new_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (client, notifier, builder, application) = dirty_graph(&dir).await;
    for _ in 0..8 {
        notifier.request_rebuild();
    }
    tick(&client).await;
    builder.entered.notified().await;
    let mut patch = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
    patch.core = Some(ClashCore::ClashRs);
    application.patch(patch).await.unwrap();
    for _ in 0..8 {
        notifier.request_rebuild();
    }
    tick(&client).await;
    assert_eq!(builder.calls.load(Ordering::SeqCst), 1);
    builder.release.notify_one();
    let mut status = client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| s.completed.len() >= 2 && s.active.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(builder.calls.load(Ordering::SeqCst), 2);
    let snapshot = client.runtime().promoted.unwrap();
    assert_eq!(snapshot.revision.get(), 2);
    assert_eq!(snapshot.target_core, ClashCore::ClashRs);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_discards_dirty_before_start_and_after_an_active_build() {
    for active in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (client, notifier, builder, _) = dirty_graph(&dir).await;
        if active {
            notifier.request_rebuild();
            tick(&client).await;
            builder.entered.notified().await;
        }
        notifier.request_rebuild();
        let mut shutdown = Box::pin(client.shutdown());
        assert!(shutdown.as_mut().now_or_never().is_none());
        barrier(&client).await;
        notifier.request_rebuild();
        tick(&client).await;
        builder.release.notify_one();
        assert!(shutdown.await.unwrap().stop.is_ok());
        notifier.request_rebuild();
        tick(&client).await;
        assert_eq!(builder.calls.load(Ordering::SeqCst), usize::from(active));
    }
}

struct ParkedEndpoint {
    delegate: crate::core::actor_v2::endpoint::EndpointHandle,
    entered: Notify,
    release: Notify,
}

#[async_trait::async_trait]
impl crate::core::actor_v2::endpoint::ControlEndpoint for ParkedEndpoint {
    fn host(&self) -> ExecutionHost {
        self.delegate.host()
    }
    async fn submit(
        &self,
        submission: crate::core::actor_v2::endpoint::CoreSubmission,
    ) -> Result<nyanpasu_ipc::api::core::v2::OperationInfo, CoreError> {
        self.delegate.submit(submission).await
    }
    async fn wait_operation(
        &self,
        id: OperationId,
        timeout: Duration,
    ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
        let result = self.delegate.wait_operation(id, timeout).await;
        if matches!(
            result.as_ref().and_then(|r| r.output.as_ref()),
            Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(_))
        ) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        result
    }
    async fn status(
        &self,
    ) -> Result<crate::core::actor_v2::endpoint::CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

#[test]
fn uninstall_waits_for_the_complete_host_switch_then_checks_ownership() {
    use super::super::tests::{HostTransitionEndpoint, HostTransitionServiceAdapter};
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let endpoint = Arc::new(ParkedEndpoint {
        delegate: HostTransitionEndpoint::new(ExecutionHost::Service, calls.clone()),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let (core, service) = tauri::async_runtime::block_on(async {
        let core = CoreClient::spawn(HostTransitionEndpoint::new(
            ExecutionHost::Local,
            calls.clone(),
        ))
        .await
        .unwrap();
        let service = ServiceClient::spawn(
            Arc::new(HostTransitionServiceAdapter {
                endpoint: endpoint.clone(),
                calls: calls.clone(),
                stopped: AtomicBool::new(false),
            }),
            0,
        )
        .await
        .unwrap();
        (core, service)
    });
    let mut args = test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding());
    args.core_v2 = core;
    args.service = service;
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let mut switch = Box::pin(client.set_execution_host(true));
        assert!(switch.as_mut().now_or_never().is_none());
        endpoint.entered.notified().await;
        let mut uninstall = Box::pin(client.uninstall_service());
        assert!(uninstall.as_mut().now_or_never().is_none());
        barrier(&client.inner.lifecycle).await;
        assert_eq!(client.lifecycle_status().queued.len(), 1);
        assert!(!calls.lock().unwrap().contains(&"uninstall"));
        endpoint.release.notify_one();
        assert!(matches!(
            switch.await.unwrap(),
            runtime::MutationOutcome::Applied { .. }
        ));
        assert_eq!(
            uninstall.await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        client.set_execution_host(false).await.unwrap();
        client.uninstall_service().await.unwrap();
        let calls = calls.lock().unwrap();
        let reconcile = calls.iter().rposition(|c| *c == "reconcile_local").unwrap();
        let uninstall = calls.iter().position(|c| *c == "uninstall").unwrap();
        assert!(reconcile < uninstall);
    });
}

#[derive(Default)]
struct Progress(AtomicBool, AtomicBool);
impl BinaryInstallProgress for Progress {
    fn restarting(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn finished(&self, _error: Option<&str>) {
        self.1.store(true, Ordering::SeqCst);
    }
}

struct Installer {
    endpoint: Arc<TestControlEndpoint>,
    entered: Notify,
    release: Notify,
    park: bool,
    fail: bool,
    panic: bool,
    calls: AtomicUsize,
    submissions_at_copy: AtomicUsize,
}

#[async_trait::async_trait]
impl BinaryInstaller for Installer {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.submissions_at_copy
            .store(self.endpoint.submissions(), Ordering::SeqCst);
        self.entered.notify_one();
        if self.park {
            self.release.notified().await;
        }
        assert!(!self.panic, "scripted installer panic");
        anyhow::ensure!(!self.fail, "scripted installation failure");
        tokio::fs::copy(&artifact.source, &artifact.destination).await?;
        Ok(())
    }
}

struct Fixture {
    client: NyanpasuClient,
    endpoint: Arc<TestControlEndpoint>,
    installer: Arc<Installer>,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(park: bool, fail: bool, panic: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let installer = Arc::new(Installer {
            endpoint: endpoint.clone(),
            entered: Notify::new(),
            release: Notify::new(),
            park,
            fail,
            panic,
            calls: AtomicUsize::new(0),
            submissions_at_copy: AtomicUsize::new(0),
        });
        let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
        args.binary_installer = installer.clone();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        Self {
            client,
            endpoint,
            installer,
            dir,
        }
    }

    fn artifact(&self, target: ClashCore) -> (PreparedCoreBinary, Arc<Progress>) {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("new-core");
        std::fs::write(&source, b"new binary").unwrap();
        let progress = Arc::new(Progress::default());
        (
            PreparedCoreBinary {
                target: target.into(),
                source,
                destination: self.dir.path().join("installed-core"),
                staging,
                progress: progress.clone(),
            },
            progress,
        )
    }
}

async fn barrier(client: &CoreLifecycleClient) {
    assert!(matches!(
        client
            .0
            .actor
            .call(Message::Barrier, Some(Duration::from_secs(5)))
            .await
            .unwrap(),
        CallResult::Success(())
    ));
}

async fn start_replacement(
    f: &Fixture,
) -> (
    tokio::task::JoinHandle<super::super::Result<()>>,
    std::path::PathBuf,
) {
    let target = f.client.get_app_config().await.unwrap().core;
    let (artifact, _) = f.artifact(target);
    let staging_path = artifact.staging.path().to_owned();
    let client = f.client.clone();
    let task = tokio::spawn(async move { client.replace_core_binary(artifact).await });
    f.installer.entered.notified().await;
    (task, staging_path)
}

#[test]
fn replacement_serializes_reconcile_and_retains_files_after_caller_cancellation() {
    let f = Fixture::new(true, false, false);
    tauri::async_runtime::block_on(async {
        let (task, staging) = start_replacement(&f).await;
        assert_eq!(
            f.endpoint.submissions(),
            2,
            "stop and death proof precede installation"
        );
        let active = f.client.lifecycle_status().active.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(staging.exists());
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        barrier(&f.client.inner.lifecycle).await;
        assert_eq!(f.client.lifecycle_status().active, Some(active));
        assert_eq!(f.client.lifecycle_status().queued.len(), 1);
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
        assert!(
            f.client
                .lifecycle_status()
                .completed
                .iter()
                .any(|r| r.id == active && r.error.is_none())
        );
    });
}

#[test]
fn replacement_decisions_use_applied_identity_and_always_recover() {
    use nyanpasu_core_manager::CoreKind;
    use nyanpasu_ipc::api::status::CoreStateDetail;
    // desired, applied kind, stopped, target, calls before copy, restart
    let cases = [
        (
            ClashCore::Mihomo,
            Some(CoreKind::ClashRust),
            false,
            ClashCore::ClashRs,
            2,
            true,
        ),
        (
            ClashCore::Mihomo,
            Some(CoreKind::Mihomo),
            false,
            ClashCore::ClashRs,
            1,
            false,
        ),
        (ClashCore::Mihomo, None, false, ClashCore::ClashRs, 2, true),
        (ClashCore::ClashRs, None, true, ClashCore::ClashRs, 1, true),
        (ClashCore::Mihomo, None, true, ClashCore::ClashRs, 1, false),
    ];
    for (desired, kind, stopped, target, before_copy, restart) in cases {
        let f = Fixture::new(false, false, false);
        tauri::async_runtime::block_on(async {
            let mut patch = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
            patch.core = Some(desired);
            f.client.patch_app_config(patch).await.unwrap();
            f.endpoint.set_status(
                Some(if stopped {
                    CoreStateDetail::Stopped { reason: None }
                } else {
                    CoreStateDetail::Running { epoch: 1, pid: 7 }
                }),
                kind,
            );
            let (artifact, progress) = f.artifact(target);
            f.client.replace_core_binary(artifact).await.unwrap();
            assert_eq!(
                f.installer.submissions_at_copy.load(Ordering::SeqCst),
                before_copy
            );
            assert_eq!(f.endpoint.submissions(), before_copy + usize::from(restart));
            assert_eq!(progress.0.load(Ordering::SeqCst), restart);
            if restart {
                assert_eq!(
                    f.client.promoted_runtime().await.unwrap().target_core,
                    desired
                );
            }
        });
    }
}

#[test]
fn failed_death_proof_never_installs_even_when_status_says_stopped() {
    let f = Fixture::new(false, false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        f.endpoint.set_recover_should_fail(true);
        let (artifact, progress) = f.artifact(ClashCore::ClashRs);
        assert!(f.client.replace_core_binary(artifact).await.is_err());
        assert_eq!(f.installer.calls.load(Ordering::SeqCst), 0);
        assert!(!progress.0.load(Ordering::SeqCst));
    });
}

#[test]
fn shutdown_rejects_pending_work_and_waits_for_the_active_installation() {
    let f = Fixture::new(true, false, false);
    tauri::async_runtime::block_on(async {
        let (replace, _) = start_replacement(&f).await;
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        let mut shutdown = Box::pin(f.client.shutdown_core());
        assert!(shutdown.as_mut().now_or_never().is_none());
        barrier(&f.client.inner.lifecycle).await;
        assert!(f.client.lifecycle_status().shutting_down);
        assert_eq!(
            reconcile.await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        assert_eq!(f.endpoint.submissions(), 2);
        f.installer.release.notify_one();
        replace.await.unwrap().unwrap();
        assert!(shutdown.await.stop.is_ok());
        let before = f.endpoint.submissions();
        assert!(f.client.shutdown_core().await.stop.is_ok());
        assert!(f.client.reconcile_core().await.is_err());
        assert_eq!(f.endpoint.submissions(), before);
    });
}

#[test]
fn queue_is_bounded_and_caller_timeout_does_not_release_admission() {
    let f = Fixture::new(true, false, false);
    tauri::async_runtime::block_on(async {
        let (artifact, progress) = f.artifact(ClashCore::Mihomo);
        let lifecycle = &f.client.inner.lifecycle;
        let mut timed = Box::pin(lifecycle.call_with_timeout(
            Command::ReplaceCoreBinary(artifact),
            Duration::from_millis(20),
        ));
        assert!(timed.as_mut().now_or_never().is_none());
        f.installer.entered.notified().await;
        let error = match timed.await {
            Err(error) => error,
            Ok(_) => panic!("parked installation must time out"),
        };
        assert_eq!(error.operation_id, f.client.lifecycle_status().active);
        let mut pending = Vec::new();
        for _ in 0..MAX_PENDING {
            let mut call = Box::pin(lifecycle.reconcile());
            assert!(call.as_mut().now_or_never().is_none());
            pending.push(call);
        }
        barrier(lifecycle).await;
        assert_eq!(lifecycle.status().queued.len(), MAX_PENDING);
        assert_eq!(
            lifecycle.reconcile().await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        assert_eq!(f.endpoint.submissions(), 2);
        let mut shutdown = Box::pin(f.client.shutdown_core());
        assert!(shutdown.as_mut().now_or_never().is_none());
        barrier(lifecycle).await;
        for call in pending {
            assert!(call.await.is_err());
        }
        f.installer.release.notify_one();
        assert!(shutdown.await.stop.is_ok());
        assert!(
            progress.1.load(Ordering::SeqCst),
            "terminal progress survives caller timeout"
        );
    });
}

#[test]
fn failed_installation_does_not_restart_and_a_panic_fails_admission_closed() {
    for panic in [false, true] {
        let f = Fixture::new(false, !panic, panic);
        tauri::async_runtime::block_on(async {
            let (artifact, progress) = f.artifact(ClashCore::Mihomo);
            assert!(f.client.replace_core_binary(artifact).await.is_err());
            assert!(!progress.0.load(Ordering::SeqCst));
            assert_eq!(f.endpoint.submissions(), 2);
            assert_eq!(f.client.lifecycle_status().uncertain, panic);
            if panic {
                assert_eq!(
                    f.client.reconcile_core().await.unwrap_err().kind,
                    Some(CoreErrorKind::OperationConflict)
                );
                assert!(f.client.shutdown_core().await.stop.is_ok());
            } else {
                f.client.reconcile_core().await.unwrap();
            }
        });
    }
}

#[test]
fn lost_backend_result_blocks_new_mutations_without_hiding_the_promoted_product() {
    let f = Fixture::new(false, false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.set_result_missing(true);
        let error = f.client.reconcile_core().await.unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
        assert!(f.client.lifecycle_status().uncertain);
        assert!(f.client.promoted_runtime().await.is_some());
        let status = f.client.lifecycle_status();
        let result = status
            .completed
            .iter()
            .find(|r| Some(r.id) == error.operation_id)
            .unwrap();
        assert!(result.backend_operation_id.is_some());
        assert_ne!(result.backend_operation_id, error.operation_id);
        assert_eq!(
            f.client.stop_core().await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        assert_eq!(f.endpoint.submissions(), 1);
        f.endpoint.set_result_missing(false);
        assert!(f.client.shutdown_core().await.stop.is_ok());
    });
}

#[tokio::test]
async fn dirty_notifications_and_shutdown_are_isolated_between_graphs() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let (a, notify_a, build_a, _) = dirty_graph(&dir_a).await;
    let (b, notify_b, build_b, _) = dirty_graph(&dir_b).await;
    notify_a.request_rebuild();
    tick(&a).await;
    tick(&b).await;
    build_a.entered.notified().await;
    assert_eq!(build_b.calls.load(Ordering::SeqCst), 0);
    build_a.release.notify_one();
    a.shutdown().await.unwrap();
    notify_b.request_rebuild();
    tick(&b).await;
    build_b.entered.notified().await;
    build_b.release.notify_one();
    b.shutdown().await.unwrap();
    assert_eq!(build_a.calls.load(Ordering::SeqCst), 1);
    assert_eq!(build_b.calls.load(Ordering::SeqCst), 1);
}
