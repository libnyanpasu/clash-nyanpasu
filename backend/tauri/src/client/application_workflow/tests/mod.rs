use crate::client::UiEventSink;
mod connection_policy;
mod mutations;
mod recovery;
mod service_recovery;
mod validation;

use super::{
    super::{
        NyanpasuClient,
        tests::{TestControlEndpoint, test_client_args_with_endpoint},
    },
    *,
};
use crate::client::core_lifecycle::ports::{BinaryInstallProgress, PreparedCoreBinary};
use nyanpasu_config::application::ClashCore;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use struct_patch::Patch;
use tokio::sync::Notify;

struct BlockingBuilder {
    delegate: adapters::FsRuntimeBuildAdapter,
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
    /// Scripts a runtime build that fails outright, so a test can drive a
    /// caller through a non-retryable apply failure.
    fail: AtomicBool,
}

#[async_trait::async_trait]
impl ports::RuntimeBuildPort for BlockingBuilder {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> anyhow::Result<super::inputs::FrozenProfileContent> {
        self.delegate.capture_content(profiles).await
    }

    fn core_spec(&self, core: &ClashCore) -> anyhow::Result<nyanpasu_core_manager::CoreSpec> {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> anyhow::Result<Arc<runtime::RuntimeSnapshot>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        anyhow::ensure!(!self.fail.load(Ordering::SeqCst), "scripted build failure");
        self.delegate
            .build(revision, inputs, ports, strict_transforms)
            .await
    }
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()> {
        self.delegate.publish(snapshot).await
    }
}

async fn dirty_graph(
    dir: &tempfile::TempDir,
) -> (
    ApplicationWorkflowClient,
    DirtyNotifier,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
    super::super::clash_config::ClashConfigClient,
) {
    dirty_graph_with_store(dir, runtime::RuntimeSnapshotStore::default()).await
}

#[tokio::test]
async fn injected_snapshot_store_is_shared_by_workflow_and_reader() {
    let dir = tempfile::tempdir().unwrap();
    let store = runtime::RuntimeSnapshotStore::default();
    let (client, notifier, builder, _, _) = dirty_graph_with_store(&dir, store.clone()).await;
    notifier.request_rebuild();
    tick(&client).await;
    builder.entered.notified().await;
    builder.release.notify_one();
    let mut status = client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| !s.completed.is_empty() && s.active.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    let written = store.read().promoted.unwrap();
    let observed = client.runtime().promoted.unwrap();
    assert!(Arc::ptr_eq(&written, &observed));
    client.shutdown().await.unwrap();
}

/// V09: an apply the core confirmed is the recovery checkpoint even when the
/// effective-config inspection never arrives. The baseline must not fall back
/// to an older apply, and it must not wait for an inspection that is only ever
/// diagnostic. The fake core answers `effective_config` with `None` here, so
/// `applied` stays empty throughout.
#[tokio::test]
async fn a_confirmed_apply_is_the_recovery_baseline_without_an_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let store = runtime::RuntimeSnapshotStore::default();
    let (client, _notifier, builder, _, _) = dirty_graph_with_store(&dir, store.clone()).await;
    builder.release.notify_one();

    client.reconcile().await.unwrap();
    let first = store
        .last_confirmed_runtime_receipt()
        .expect("the core confirmed the first apply");
    assert!(
        store.read().applied.is_none(),
        "no effective config arrived, so nothing is inspected"
    );
    assert!(
        store.read().pending.is_some(),
        "the apply is recorded as awaiting its inspection"
    );
    assert_eq!(
        first.config_digest,
        nyanpasu_core_manager::payload_digest(first.config_text.as_bytes()),
        "the receipt carries the bytes the core accepted, and their digest"
    );

    client.reconcile().await.unwrap();
    let second = store
        .last_confirmed_runtime_receipt()
        .expect("the core confirmed the second apply");
    assert!(
        second.revision.get() > first.revision.get(),
        "the baseline follows the latest confirmed apply, not the last inspected one"
    );
    assert!(store.read().applied.is_none());
    client.shutdown().await.unwrap();
}

/// An unobserved reconcile is the third case where the confirmed binding stops
/// being a fact, and the one where it matters most: the core may already be
/// listening on the candidate's ports and the app cannot ask. Publishing the
/// previous binding afterwards is exactly the "the port we used last time"
/// decay the module disclaims.
#[tokio::test]
async fn an_unobserved_reconcile_stops_publishing_the_previous_port_binding() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let service = ServiceClient::spawn(Arc::new(super::super::tests::IdleServiceAdapter), 0)
        .await
        .unwrap();
    let ports = Arc::new(super::super::SessionPortResolver::default());
    let (client, _notifier, builder, _, clash) =
        dirty_graph_with_clients(&dir, core, service, false, ports.clone()).await;
    builder.release.notify_one();

    // Two ports that are distinct by construction. Nothing binds them here, and
    // two ephemeral probes can hand back the same number once the first
    // listener is dropped, which would make the assertion below vacuous.
    let mut config = clash.snapshot().state;
    config.mixed_port = fixed_port(48611);
    clash.replace(config.clone()).await.unwrap();
    client.reconcile().await.unwrap();
    let confirmed = ports
        .confirmed()
        .expect("the apply the core accepted confirms its ports");

    // A new candidate on a different port, and a reconcile whose result is
    // lost: the core may or may not have moved onto it.
    config.mixed_port = fixed_port(48612);
    clash.replace(config).await.unwrap();
    endpoint.set_result_missing(true);
    client
        .reconcile()
        .await
        .expect_err("an unobserved outcome is not an applied one");

    assert_eq!(
        ports.confirmed(),
        None,
        "the previous binding on {} is no longer a fact about anything",
        confirmed.mixed_port
    );
    client.shutdown().await.unwrap();
}

/// V11 (port half): the confirmed binding describes a running instance. When
/// the user stops the core nothing is holding those ports any more, so the
/// self-proxy source and the system proxy must get "unavailable" rather than
/// the endpoint the stopped core used to listen on.
#[tokio::test]
async fn stopping_the_core_ends_the_confirmed_port_binding() {
    let dir = tempfile::tempdir().unwrap();
    let core = CoreClient::spawn(TestControlEndpoint::succeeding())
        .await
        .unwrap();
    let service = ServiceClient::spawn(Arc::new(super::super::tests::IdleServiceAdapter), 0)
        .await
        .unwrap();
    let ports = Arc::new(super::super::SessionPortResolver::default());
    let (client, _notifier, builder, _, _) =
        dirty_graph_with_clients(&dir, core, service, false, ports.clone()).await;
    builder.release.notify_one();

    assert_eq!(
        ports.confirmed(),
        None,
        "nothing has been applied yet, so nothing is listening"
    );
    client.reconcile().await.unwrap();
    let running = ports
        .confirmed()
        .expect("the apply the core accepted confirms its ports");

    client.stop_core().await.unwrap();

    assert_eq!(
        ports.confirmed(),
        None,
        "a stopped core is not still holding {}",
        running.mixed_port
    );
    client.shutdown().await.unwrap();
}

async fn dirty_graph_with_store(
    dir: &tempfile::TempDir,
    snapshots: runtime::RuntimeSnapshotStore,
) -> (
    ApplicationWorkflowClient,
    DirtyNotifier,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
    super::super::clash_config::ClashConfigClient,
) {
    let core = CoreClient::spawn(TestControlEndpoint::succeeding())
        .await
        .unwrap();
    let service = ServiceClient::spawn(Arc::new(super::super::tests::IdleServiceAdapter), 0)
        .await
        .unwrap();
    dirty_graph_with_clients(
        dir,
        core,
        service,
        false,
        Arc::new(super::super::SessionPortResolver::new(snapshots)),
    )
    .await
}

async fn dirty_graph_with_clients(
    dir: &tempfile::TempDir,
    core: CoreClient,
    service: ServiceClient,
    schedule_ticks: bool,
    // Injected so a test can read the binding the workflow confirms; the
    // workflow is the only writer.
    ports: Arc<super::super::SessionPortResolver>,
) -> (
    ApplicationWorkflowClient,
    DirtyNotifier,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
    super::super::clash_config::ClashConfigClient,
) {
    use super::super::tests::{test_materialization_port, test_typed_config_clients};
    use crate::state::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
    let (application, _, clash) = test_typed_config_clients(dir).await;
    let (notifier, dirty) = DirtyNotifier::channel();
    let profiles = super::super::profiles::ProfilesClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap(),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_materialization_port(),
    )
    .await
    .unwrap();
    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let validator_paths = paths.clone();
    let core_for_validator = core.clone();
    let builder = Arc::new(BlockingBuilder {
        delegate: adapters::FsRuntimeBuildAdapter {
            profiles_dir: dir.path().join("profiles"),
            paths,
        },
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        fail: AtomicBool::new(false),
    });
    let client = ApplicationWorkflowClient::spawn_with_ticks(
        ApplicationWorkflowArgs {
            notifications: Arc::new(crate::client::effects::ports::NoopCommitNotifications),
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            core,
            service,
            builder: builder.clone(),
            validator: Arc::new(adapters::CoreCheckValidator::new(
                core_for_validator,
                validator_paths,
            )),
            ports,
            installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),

            dirty,
            budgets: mutation::MutationBudgets::default(),
        },
        schedule_ticks,
    )
    .await
    .unwrap();
    (client, notifier, builder, application, clash)
}

fn fixed_port(port: u16) -> nyanpasu_config::clash::config::clash_strategy::port::PortStrategy {
    nyanpasu_config::clash::config::clash_strategy::port::PortStrategy {
        kind: nyanpasu_config::clash::config::clash_strategy::port::PortStrategyKind::Fixed,
        start_port: port,
    }
}

async fn tick(client: &ApplicationWorkflowClient) {
    client.0.actor.cast(Message::DirtyTick).unwrap();
    barrier(client).await;
}

#[tokio::test]
async fn idle_ticks_do_not_advance_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let (client, ..) = dirty_graph(&dir).await;
    barrier(&client).await;
    let mut journal = client.subscribe_mutations();
    journal.borrow_and_update();
    let before = client.mutation_journal().event_seq;
    for message in [
        Message::DirtyTick,
        Message::ConvergenceTick,
        Message::RecoveryTick,
    ] {
        client.0.actor.cast(message).unwrap();
    }
    barrier(&client).await;
    assert_eq!(client.mutation_journal().event_seq, before);
    assert!(!journal.has_changed().unwrap());
}

#[tokio::test]
async fn dirty_during_build_coalesces_and_eventually_applies_the_new_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (client, notifier, builder, application, _) = dirty_graph(&dir).await;
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
        let (client, notifier, builder, _, _) = dirty_graph(&dir).await;
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
        client.reconcile_core().await.unwrap();
        let switch = {
            let client = client.clone();
            tokio::spawn(async move { client.set_execution_host(true).await })
        };
        tokio::time::timeout(Duration::from_secs(5), endpoint.entered.notified())
            .await
            .unwrap();
        let mut uninstall = Box::pin(client.uninstall_service());
        assert!(uninstall.as_mut().now_or_never().is_none());
        barrier(&client.inner.application_workflow).await;
        assert_eq!(client.core_lifecycle_status().queued.len(), 1);
        assert!(!calls.lock().unwrap().contains(&"uninstall"));
        endpoint.release.notify_one();
        assert!(matches!(
            switch.await.unwrap().unwrap(),
            runtime::MutationOutcome::Committed { .. }
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

/// How many attempts still own a control context inside the actor.
async fn live_mutation_contexts(client: &ApplicationWorkflowClient) -> usize {
    match client
        .0
        .actor
        .call(Message::LiveMutationContexts, Some(Duration::from_secs(5)))
        .await
        .unwrap()
    {
        CallResult::Success(live) => live,
        other => panic!("the workflow should answer: {other:?}"),
    }
}

async fn barrier(client: &ApplicationWorkflowClient) {
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
        let active = f.client.core_lifecycle_status().active.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(staging.exists());
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        barrier(&f.client.inner.application_workflow).await;
        assert_eq!(f.client.core_lifecycle_status().active, Some(active));
        assert_eq!(f.client.core_lifecycle_status().queued.len(), 1);
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
                .core_lifecycle_status()
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
            f.endpoint
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
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
        barrier(&f.client.inner.application_workflow).await;
        assert!(f.client.core_lifecycle_status().shutting_down);
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
        let core_lifecycle = &f.client.inner.application_workflow;
        let mut timed = Box::pin(core_lifecycle.call_with_timeout(
            Command::Core(CoreCommand::ReplaceCoreBinary(artifact)),
            Duration::from_millis(20),
        ));
        assert!(timed.as_mut().now_or_never().is_none());
        f.installer.entered.notified().await;
        let error = match timed.await {
            Err(error) => error,
            Ok(_) => panic!("parked installation must time out"),
        };
        assert_eq!(error.operation_id, f.client.core_lifecycle_status().active);
        let mut pending = Vec::new();
        for _ in 0..MAX_PENDING {
            let mut call = Box::pin(core_lifecycle.reconcile());
            assert!(call.as_mut().now_or_never().is_none());
            pending.push(call);
        }
        barrier(core_lifecycle).await;
        assert_eq!(core_lifecycle.status().queued.len(), MAX_PENDING);
        assert_eq!(
            core_lifecycle.reconcile().await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        let (mut rejected, _) = f.artifact(ClashCore::ClashRs);
        let terminal = Arc::new(TerminalProgress::default());
        rejected.progress = terminal.clone();
        assert!(core_lifecycle.replace_binary(rejected).await.is_err());
        let outcomes = terminal.0.lock().unwrap().clone();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].as_ref().unwrap().contains("queue is full"));
        assert_eq!(f.endpoint.submissions(), 2);
        let mut shutdown = Box::pin(f.client.shutdown_core());
        assert!(shutdown.as_mut().now_or_never().is_none());
        barrier(core_lifecycle).await;
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
            assert_eq!(f.client.core_lifecycle_status().uncertain, panic);
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
        assert!(f.client.core_lifecycle_status().uncertain);
        assert!(f.client.promoted_runtime().await.is_some());
        let status = f.client.core_lifecycle_status();
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
    let (a, notify_a, build_a, _, _) = dirty_graph(&dir_a).await;
    let (b, notify_b, build_b, _, _) = dirty_graph(&dir_b).await;
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

fn override_patch(
    value: serde_json::Value,
) -> nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
    serde_json::from_value(value).unwrap()
}

async fn disable_mode_interruption(client: &NyanpasuClient) {
    let mut config = client.get_clash_config().await.unwrap();
    config.break_connection.on_mode_change = false;
    let mut patch = nyanpasu_config::clash::config::ClashConfig::new_empty_patch();
    patch.break_connection = Some(config.break_connection);
    client.patch_clash_config(patch).await.unwrap();
}

#[test]
fn config_writes_preserve_both_fields_and_reconcile_each_committed_patch() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let client =
        NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(&dir, endpoint.clone()))
            .unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let (left, right) = tokio::join!(
            client.patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"}))),
            client.patch_runtime_overrides(override_patch(serde_json::json!({"ipv6":true})))
        );
        assert!(left.unwrap().degradations().is_empty());
        assert!(right.unwrap().degradations().is_empty());
        let saved =
            serde_json::to_value(client.get_clash_config().await.unwrap().overrides).unwrap();
        assert_eq!(saved["mode"], "global");
        assert_eq!(saved["ipv6"], true);
        let applied = client.runtime_lifecycle_state().await.promoted.unwrap();
        assert_eq!(applied.config["mode"].as_str(), Some("global"));
        assert_eq!(applied.config["ipv6"].as_bool(), Some(true));
        assert_eq!(applied.revision.get(), 3);
        assert_eq!(endpoint.submissions(), 2);
    });
}

#[test]
fn config_reconcile_failure_reports_committed_state_without_replaying() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::failing();
    let args = test_client_args_with_endpoint(&dir, endpoint.clone());
    let config_path = args.paths.clash_config_path();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let outcome = client
            .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"direct"})))
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert_eq!(outcome.degradations()[0].code, "runtime_deferred");
        assert_eq!(
            client.configuration_status().runtime.health,
            crate::client::convergence::ConvergenceHealth::RetryScheduled
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
    });
}

struct RejectConfigMirror(AtomicBool);
impl crate::state::mirror::ClashLegacyBridge for RejectConfigMirror {
    fn prepare(
        &self,
        _: &nyanpasu_config::clash::config::ClashConfig,
    ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
        anyhow::ensure!(
            !self.0.load(Ordering::SeqCst),
            "config preparation rejected"
        );
        Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
    }
    fn snapshot_legacy(&self) -> anyhow::Result<nyanpasu_config::clash::config::ClashConfig> {
        Ok(Default::default())
    }
}

#[test]
fn config_commit_failure_never_reconciles_or_changes_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let bridge = Arc::new(RejectConfigMirror(AtomicBool::new(false)));
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.bridges.clash = bridge.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let before = client.inner.clash_config.snapshot();
        bridge.0.store(true, Ordering::SeqCst);
        assert!(
            client
                .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"})))
                .await
                .is_err()
        );
        let after = client.inner.clash_config.snapshot();
        assert_eq!(after.version, before.version);
        assert_eq!(
            serde_json::to_value(after.state).unwrap(),
            serde_json::to_value(before.state).unwrap()
        );
        assert_eq!(endpoint.submissions(), 0);
        assert_eq!(
            client
                .runtime_lifecycle_state()
                .await
                .promoted
                .unwrap()
                .revision
                .get(),
            1
        );
    });
}

#[test]
fn config_persistence_failure_restores_runtime_and_keeps_source_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let args = test_client_args_with_endpoint(&dir, endpoint.clone());
    let path = args.paths.clash_config_path();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let before = client.inner.clash_config.snapshot();
        let mut settled = client.inner.application_workflow.subscribe_mutations();
        let completed = settled.borrow().completed.len();
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::create_dir_all(&path).unwrap();
        assert!(
            client
                .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"})))
                .await
                .is_err()
        );
        let after = client.inner.clash_config.snapshot();
        assert_eq!(after.version, before.version);
        assert_eq!(
            serde_json::to_value(after.state).unwrap(),
            serde_json::to_value(before.state).unwrap()
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            settled.wait_for(|journal| journal.completed.len() > completed),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            endpoint.submissions(),
            2,
            "Try applied and Cancel restored the baseline"
        );
        assert!(!client.core_lifecycle_status().uncertain);
    });
}

struct FailingConfigTray {
    refreshed: AtomicUsize,
}
impl UiEventSink for FailingConfigTray {
    fn state_changed(&self, _: crate::core::handle::StateChanged) {
        self.refreshed.fetch_add(1, Ordering::SeqCst);
    }
    fn notice_message(&self, _: &crate::core::handle::Message) {}
    fn update_systray(&self) -> crate::client::Result<()> {
        Ok(())
    }
    fn update_systray_part(&self) -> crate::client::Result<()> {
        Err(anyhow::anyhow!("tray refresh rejected").into())
    }
}

#[test]
fn legacy_ui_callbacks_do_not_vote_on_a_successful_reconcile() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let ui = Arc::new(FailingConfigTray {
        refreshed: AtomicUsize::new(0),
    });
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.ui_sink = ui.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let outcome = client
            .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"})))
            .await
            .unwrap();
        assert_eq!(endpoint.submissions(), 1);
        assert!(outcome.degradations().is_empty());
        assert_eq!(
            client
                .runtime_lifecycle_state()
                .await
                .promoted
                .unwrap()
                .config["mode"]
                .as_str(),
            Some("global")
        );
    });
}

#[test]
fn control_channel_reconcile_reads_committed_clash_config() {
    use nyanpasu_config::clash::config::{ClashConfig, ClashControlChannel};
    use nyanpasu_core_manager::LocalIpcPolicy;

    let f = Fixture::new(false, false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.prime(&f.client).await;
        for (channel, disable_http, policy) in [
            (ClashControlChannel::HttpOnly, true, LocalIpcPolicy::Disable),
            (
                ClashControlChannel::HttpOnly,
                false,
                LocalIpcPolicy::Disable,
            ),
            (ClashControlChannel::PreferIpc, true, LocalIpcPolicy::Prefer),
            (
                ClashControlChannel::PreferIpc,
                false,
                LocalIpcPolicy::Prefer,
            ),
        ] {
            let mut patch = ClashConfig::new_empty_patch();
            patch.clash_control_channel = Some(channel);
            patch.clash_ipc_disable_http_controller = Some(disable_http);
            f.client.patch_clash_config(patch).await.unwrap();
            f.client.apply_control_channel().await.unwrap();
            let settings = f.endpoint.local_ipc.lock().unwrap().unwrap();
            assert_eq!(settings.policy, policy);
            assert_eq!(settings.keep_http_controller, !disable_http);
        }
    });
}

#[test]
fn control_channel_application_does_not_start_a_stopped_core() {
    let f = Fixture::new(false, false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        f.client.apply_control_channel().await.unwrap();
        assert_eq!(f.endpoint.submissions(), 0);
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
            Some(nyanpasu_core_manager::CoreKind::Mihomo),
        );
        f.client.apply_control_channel().await.unwrap();
        assert_eq!(f.endpoint.submissions(), 1);
    });
}

#[derive(Default)]
struct TerminalProgress(std::sync::Mutex<Vec<Option<String>>>);
impl BinaryInstallProgress for TerminalProgress {
    fn restarting(&self) {
        panic!("a rejected installation must not restart");
    }
    fn finished(&self, error: Option<&str>) {
        self.0.lock().unwrap().push(error.map(str::to_owned));
    }
}

#[test]
fn queued_installation_timeout_is_settled_when_shutdown_or_uncertainty_rejects_it() {
    for panic in [false, true] {
        let f = Fixture::new(true, false, panic);
        tauri::async_runtime::block_on(async {
            let (first, _) = start_replacement(&f).await;
            let (mut queued, _) = f.artifact(ClashCore::ClashRs);
            let terminal = Arc::new(TerminalProgress::default());
            queued.progress = terminal.clone();
            let client = &f.client.inner.application_workflow;
            let result = client
                .call_with_timeout(
                    Command::Core(CoreCommand::ReplaceCoreBinary(queued)),
                    Duration::from_millis(20),
                )
                .await;
            assert!(
                matches!(result, Err(error) if error.kind == Some(CoreErrorKind::BackendUnavailable))
            );
            assert_eq!(client.status().queued.len(), 1);
            assert!(terminal.0.lock().unwrap().is_empty());
            if panic {
                f.installer.release.notify_one();
                assert!(first.await.unwrap().is_err());
                barrier(client).await;
                assert!(client.status().uncertain);
            } else {
                let mut shutdown = Box::pin(f.client.shutdown_core());
                assert!(shutdown.as_mut().now_or_never().is_none());
                barrier(client).await;
                f.installer.release.notify_one();
                first.await.unwrap().unwrap();
                assert!(shutdown.await.stop.is_ok());
            }
            let outcomes = terminal.0.lock().unwrap().clone();
            assert_eq!(
                outcomes.len(),
                1,
                "rejected request must deliver exactly one terminal notification"
            );
            assert!(outcomes[0].as_ref().unwrap().contains(if panic {
                "uncertain outcome"
            } else {
                "shutting down"
            }));
            assert_eq!(f.installer.calls.load(Ordering::SeqCst), 1);
            assert!(client.status().queued.is_empty());
        });
    }
}
