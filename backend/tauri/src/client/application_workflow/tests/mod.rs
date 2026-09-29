use crate::client::UiEventSink;
mod closing;
mod connection_policy;
mod mutations;
mod recovery;
mod service_recovery;
mod startup;
mod validation;

use super::{
    super::{
        NyanpasuClient,
        tests::{TestControlEndpoint, set_service_mode, test_client_args_with_endpoint},
    },
    *,
};
use crate::client::core_lifecycle::ports::{
    BinaryInstallProgress, InstallCoreBinaryError, PreparedCoreBinary,
};
use futures_util::FutureExt;
use nyanpasu_config::application::ClashCore;
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};
use struct_patch::Patch;
use tokio::sync::Notify;

/// What the source that hit `error` would classify it as, given the Runtime's
/// receipt for the operation.
fn classify(
    error: nyanpasu_core::state::ReplaceIfVersionError,
    receipt: &super::mutation::MutationReceipt,
) -> crate::state::mutation::CommitAborted {
    crate::state::mutation::CommitAborted::classify(error, Some(receipt))
}

/// The reasons a required participant gave for refusing a candidate.
fn refusal_reasons(aborted: &crate::state::mutation::CommitAborted) -> String {
    match aborted {
        crate::state::mutation::CommitAborted::RuntimeRefused { reasons, .. } => reasons.join("; "),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// What became of the runtime after the aborted commit.
fn aftermath(
    aborted: &crate::state::mutation::CommitAborted,
) -> &crate::state::mutation::RuntimeAftermath {
    use crate::state::mutation::CommitAborted::*;
    match aborted {
        WriteConfig { runtime, .. }
        | RecoverAfterWriteFailure { runtime, .. }
        | RuntimeRefused { runtime, .. }
        | RuntimeFailed { runtime, .. } => runtime,
        ValidateState { .. } => panic!("{aborted:?} has no runtime aftermath"),
    }
}

/// A publication that fails the way a full disk does.
pub(super) fn scripted_publish_failure() -> crate::client::runtime::PublishRuntimeError {
    crate::client::runtime::PublishRuntimeError::CreateRuntimeDirectory {
        path: std::path::PathBuf::from("runtime").into(),
        source: std::io::Error::other("scripted publish failure"),
    }
}

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
    ) -> super::inputs::FrozenProfileContent {
        self.delegate.capture_content(profiles).await
    }

    fn core_spec(
        &self,
        core: &ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, crate::core::actor_v2::local_host::CoreSpecError>
    {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, crate::enhance::RuntimeBuildError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(crate::enhance::RuntimeBuildError::ConfigNotMapping);
        }
        self.delegate
            .build(revision, inputs, ports, strict_transforms)
            .await
    }
    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), crate::client::runtime::PublishRuntimeError> {
        self.delegate.publish(snapshot).await
    }
}

async fn workflow_graph(
    dir: &tempfile::TempDir,
) -> (
    ApplicationWorkflowClient,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
    super::super::clash_config::ClashConfigClient,
) {
    workflow_graph_with_store(dir, runtime::RuntimeSnapshotStore::default()).await
}

#[tokio::test]
async fn injected_snapshot_store_is_shared_by_workflow_and_reader() {
    let dir = tempfile::tempdir().unwrap();
    let store = runtime::RuntimeSnapshotStore::default();
    let (client, builder, _, _) = workflow_graph_with_store(&dir, store.clone()).await;
    let reconcile = {
        let client = client.clone();
        tokio::spawn(async move { client.reconcile().await })
    };
    builder.entered.notified().await;
    builder.release.notify_one();
    reconcile.await.unwrap().unwrap();
    let written = store.read().promoted.unwrap();
    let observed = client.runtime().promoted.unwrap();
    assert!(Arc::ptr_eq(&written, &observed));
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
    let (client, builder, _, _) = workflow_graph_with_store(&dir, store.clone()).await;
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
    let (client, builder, _, clash) = workflow_graph_with_clients(
        &dir,
        core,
        service,
        false,
        ports.clone(),
        CancellationToken::new(),
    )
    .await;
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
    let (client, builder, _, _) = workflow_graph_with_clients(
        &dir,
        core,
        service,
        false,
        ports.clone(),
        CancellationToken::new(),
    )
    .await;
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
}

async fn workflow_graph_with_store(
    dir: &tempfile::TempDir,
    snapshots: runtime::RuntimeSnapshotStore,
) -> (
    ApplicationWorkflowClient,
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
    workflow_graph_with_clients(
        dir,
        core,
        service,
        false,
        Arc::new(super::super::SessionPortResolver::new(snapshots)),
        CancellationToken::new(),
    )
    .await
}

async fn workflow_graph_with_clients(
    dir: &tempfile::TempDir,
    core: CoreClient,
    service: ServiceClient,
    schedule_ticks: bool,
    // Injected so a test can read the binding the workflow confirms; the
    // workflow is the only writer.
    ports: Arc<super::super::SessionPortResolver>,
    // Cancelling it closes the workflow the way the root shutdown does.
    shutdown: CancellationToken,
) -> (
    ApplicationWorkflowClient,
    Arc<BlockingBuilder>,
    super::super::application::ApplicationClient,
    super::super::clash_config::ClashConfigClient,
) {
    use super::super::tests::{test_materialization_port, test_typed_config_clients};
    use crate::state::profiles::ports::{MockProfileFsPort, MockSubscriptionFetcher};
    let (application, _, clash) = test_typed_config_clients(dir).await;
    let profiles = super::super::profiles::ProfilesClient::new(
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
    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let validator_paths = paths.clone();
    let core_for_validator = core.clone();
    // The graph's router already drives the host it was built on, and these
    // tests are not about proving that.
    let ownership = super::super::core_lifecycle::Ownership::Established {
        host: core.status().host,
    };
    let builder = Arc::new(BlockingBuilder {
        delegate: adapters::FsRuntimeBuildAdapter {
            profiles_dir: dir.path().join("profiles"),
            paths,
            scripts: crate::enhance::ScriptDirs::under(dir.path()),
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
            ownership,
            instance_config_dir: Default::default(),
            shutdown,
            tasks: tokio_util::task::TaskTracker::new(),
        },
        schedule_ticks,
    )
    .await
    .unwrap();
    (client, builder, application, clash)
}

fn fixed_port(port: u16) -> nyanpasu_config::clash::config::clash_strategy::port::PortStrategy {
    nyanpasu_config::clash::config::clash_strategy::port::PortStrategy {
        kind: nyanpasu_config::clash::config::clash_strategy::port::PortStrategyKind::Fixed,
        start_port: port,
    }
}

#[tokio::test]
async fn idle_ticks_do_not_advance_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let (client, ..) = workflow_graph(&dir).await;
    barrier(&client).await;
    let mut journal = client.subscribe_mutations();
    journal.borrow_and_update();
    let before = client.mutation_journal().event_seq;
    for message in [Message::ConvergenceTick, Message::RecoveryTick] {
        client.0.actor.cast(message).unwrap();
    }
    barrier(&client).await;
    assert_eq!(client.mutation_journal().event_seq, before);
    assert!(!journal.has_changed().unwrap());
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

/// How [`ScriptedWaitEndpoint`] answers the wait for one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitScript {
    /// The real terminal result.
    Deliver,
    /// The registry answers nothing: an admitted operation whose result is
    /// lost.
    Missing,
    /// The operation is admitted and still running.
    Running,
    /// The real terminal result, held until the test releases it.
    Held,
}

/// A control endpoint whose operation waits are scripted per operation.
///
/// Each submission takes the next queued script (`Deliver` once the queue is
/// empty) and keeps it until a test rescripts that operation, so the one
/// operation a test cares about can be lost or left running, and later
/// delivered, while everything else is real.
struct ScriptedWaitEndpoint {
    delegate: Arc<TestControlEndpoint>,
    queued: std::sync::Mutex<std::collections::VecDeque<WaitScript>>,
    scripts: std::sync::Mutex<Vec<(OperationId, WaitScript)>>,
    held: Notify,
    release: Notify,
}

impl ScriptedWaitEndpoint {
    fn new(delegate: Arc<TestControlEndpoint>) -> Arc<Self> {
        Arc::new(Self {
            delegate,
            queued: std::sync::Mutex::new(std::collections::VecDeque::new()),
            scripts: std::sync::Mutex::new(Vec::new()),
            held: Notify::new(),
            release: Notify::new(),
        })
    }

    /// Resolves once a `Held` wait is holding its result.
    async fn held(&self) {
        self.held.notified().await;
    }

    /// Lets the held wait deliver.
    fn release(&self) {
        self.release.notify_one();
    }

    /// The script the next submission takes.
    fn queue(&self, script: WaitScript) {
        self.queued.lock().unwrap().push_back(script);
    }

    fn rescript(&self, operation: OperationId, script: WaitScript) {
        let mut scripts = self.scripts.lock().unwrap();
        let entry = scripts
            .iter_mut()
            .find(|(id, _)| *id == operation)
            .expect("only a submitted operation is rescripted");
        entry.1 = script;
    }

    fn submitted(&self) -> usize {
        self.scripts.lock().unwrap().len()
    }

    /// Every submitted operation, in order.
    fn operations(&self) -> Vec<OperationId> {
        self.scripts
            .lock()
            .unwrap()
            .iter()
            .map(|(operation, _)| *operation)
            .collect()
    }
}

#[async_trait::async_trait]
impl crate::core::actor_v2::endpoint::ControlEndpoint for ScriptedWaitEndpoint {
    async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.delegate.effective_config().await
    }
    async fn api_connection(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        self.delegate.api_connection().await
    }
    async fn api_changes(
        &self,
    ) -> Result<Option<crate::core::actor_v2::endpoint::ApiChanges>, CoreError> {
        self.delegate.api_changes().await
    }
    fn host(&self) -> ExecutionHost {
        self.delegate.host()
    }
    async fn check_config(
        &self,
        submission: crate::core::actor_v2::endpoint::CheckSubmission,
    ) -> crate::core::actor_v2::endpoint::CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn submit(
        &self,
        submission: crate::core::actor_v2::endpoint::CoreSubmission,
    ) -> Result<nyanpasu_ipc::api::core::v2::OperationInfo, CoreError> {
        let operation = submission.envelope.operation_id;
        let script = self
            .queued
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(WaitScript::Deliver);
        self.scripts.lock().unwrap().push((operation, script));
        self.delegate.submit(submission).await
    }
    async fn wait_operation(
        &self,
        id: OperationId,
        timeout: Duration,
    ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
        let script = self
            .scripts
            .lock()
            .unwrap()
            .iter()
            .find(|(operation, _)| *operation == id)
            .map_or(WaitScript::Deliver, |(_, script)| *script);
        match script {
            WaitScript::Deliver => self.delegate.wait_operation(id, timeout).await,
            WaitScript::Missing => None,
            WaitScript::Running => Some(nyanpasu_ipc::api::core::v2::OperationInfo {
                id: id.to_string(),
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Running,
                output: None,
                error: None,
            }),
            WaitScript::Held => {
                self.held.notify_one();
                self.release.notified().await;
                self.delegate.wait_operation(id, timeout).await
            }
        }
    }
    async fn status(
        &self,
    ) -> Result<crate::core::actor_v2::endpoint::CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

/// Commit notifications that count what the Runtime told them. The Runtime
/// sends only its own slice; a source's slice here is a bug.
#[derive(Default)]
struct RecordingNotifications {
    bound: AtomicUsize,
    full: AtomicUsize,
}

impl RecordingNotifications {
    fn bound(&self) -> usize {
        self.bound.load(Ordering::SeqCst)
    }

    fn full(&self) -> usize {
        self.full.load(Ordering::SeqCst)
    }
}

impl crate::client::effects::ports::CommitNotifications for RecordingNotifications {
    fn application_committed(
        &self,
        _: crate::client::effects::plan::ApplicationEffectFields,
        _: Vec<crate::client::effects::plan::EffectKind>,
    ) {
        unreachable!("only the application owner sends its slice")
    }

    fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {
        unreachable!("only the clash config owner sends its slice")
    }

    fn profiles_committed(&self) {
        unreachable!("only the profiles owner sends its slice")
    }

    fn runtime_bound(
        &self,
        _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>,
        _: bool,
    ) {
        self.bound.fetch_add(1, Ordering::SeqCst);
    }

    fn publish_full(&self, _: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>) {
        self.full.fetch_add(1, Ordering::SeqCst);
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
        let core = CoreClient::spawn(HostTransitionEndpoint::stopped(
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
                installed_for: dir.path().into(),
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
        assert_eq!(
            client.startup_reconcile().await.outcome,
            super::startup::StartupOutcome::Ready
        );
        let switch = {
            let client = client.clone();
            tokio::spawn(async move { set_service_mode(&client, true).await })
        };
        tokio::time::timeout(Duration::from_secs(5), endpoint.entered.notified())
            .await
            .unwrap();
        // Polling once sends the request, so it is in the mailbox behind the
        // switch before the switch is released.
        let mut uninstall = Box::pin(client.uninstall_service());
        assert!(uninstall.as_mut().now_or_never().is_none());
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
        set_service_mode(&client, false).await.unwrap();
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
    calls: AtomicUsize,
    submissions_at_copy: AtomicUsize,
}

#[async_trait::async_trait]
impl BinaryInstaller for Installer {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.submissions_at_copy
            .store(self.endpoint.submissions(), Ordering::SeqCst);
        self.entered.notify_one();
        if self.park {
            self.release.notified().await;
        }
        if self.fail {
            return Err(InstallCoreBinaryError::ElevatedCopyFailed {
                core: artifact.target,
                destination: (&artifact.destination).into(),
                exit_code: Some(1),
            });
        }
        tokio::fs::copy(&artifact.source, &artifact.destination)
            .await
            .unwrap();
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
    fn new(park: bool, fail: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let installer = Arc::new(Installer {
            endpoint: endpoint.clone(),
            entered: Notify::new(),
            release: Notify::new(),
            park,
            fail,
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
                target,
                source,
                destination: self.dir.path().join("installed-core"),
                staging,
                progress: progress.clone(),
            },
            progress,
        )
    }
}

/// Which owner the idle workflow holds proven.
async fn ownership(client: &ApplicationWorkflowClient) -> Ownership {
    match client
        .0
        .actor
        .call(Message::Ownership, Some(Duration::from_secs(5)))
        .await
        .unwrap()
    {
        CallResult::Success(ownership) => ownership,
        other => panic!("the idle workflow should answer: {other:?}"),
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
    tokio::task::JoinHandle<Result<(), CoreError>>,
    std::path::PathBuf,
) {
    let target = f.client.get_app_config().await.unwrap().core;
    let (artifact, _) = f.artifact(target);
    let staging_path = artifact.staging.path().to_owned();
    let client = f.client.clone();
    let task = tokio::spawn(async move {
        client
            .inner
            .application_workflow
            .replace_binary(artifact)
            .await
    });
    f.installer.entered.notified().await;
    (task, staging_path)
}

#[test]
fn replacement_serializes_reconcile_and_retains_files_after_caller_cancellation() {
    let f = Fixture::new(true, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.prime(&f.client).await;
        let (task, staging) = start_replacement(&f).await;
        assert_eq!(
            f.endpoint.submissions(),
            2,
            "stop and death proof precede installation"
        );
        let active = f.client.inner.application_workflow.status().active.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(staging.exists());
        // Polling once sends the reconcile, so it waits behind the parked
        // installation.
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        assert_eq!(
            f.client.inner.application_workflow.status().active,
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
        assert!(!f.client.inner.application_workflow.status().uncertain);
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
        let f = Fixture::new(false, false);
        tauri::async_runtime::block_on(async {
            f.endpoint.prime(&f.client).await;
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
            f.client
                .inner
                .application_workflow
                .replace_binary(artifact)
                .await
                .unwrap();
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
    let f = Fixture::new(false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        f.endpoint.set_recover_should_fail(true);
        let (artifact, progress) = f.artifact(ClashCore::ClashRs);
        assert!(
            f.client
                .inner
                .application_workflow
                .replace_binary(artifact)
                .await
                .is_err()
        );
        assert_eq!(f.installer.calls.load(Ordering::SeqCst), 0);
        assert!(!progress.0.load(Ordering::SeqCst));
    });
}

#[test]
fn shutdown_rejects_pending_work_and_waits_for_the_active_installation() {
    let f = Fixture::new(true, false);
    tauri::async_runtime::block_on(async {
        let (replace, _) = start_replacement(&f).await;
        let mut reconcile = Box::pin(f.client.reconcile_core());
        assert!(reconcile.as_mut().now_or_never().is_none());
        f.client.request_shutdown();
        let mut shutdown = Box::pin(f.client.wait_shutdown());
        assert!(shutdown.as_mut().now_or_never().is_none());
        assert_eq!(f.endpoint.submissions(), 2);
        f.installer.release.notify_one();
        replace.await.unwrap().unwrap();
        assert_eq!(
            reconcile.await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        shutdown.await;
        let before = f.endpoint.submissions();
        assert!(f.client.reconcile_core().await.is_err());
        assert_eq!(f.endpoint.submissions(), before);
    });
}

#[test]
fn failed_installation_does_not_restart() {
    let f = Fixture::new(false, true);
    tauri::async_runtime::block_on(async {
        let (artifact, progress) = f.artifact(ClashCore::Mihomo);
        assert!(
            f.client
                .inner
                .application_workflow
                .replace_binary(artifact)
                .await
                .is_err()
        );
        assert!(!progress.0.load(Ordering::SeqCst));
        assert_eq!(f.endpoint.submissions(), 2);
        assert!(!f.client.inner.application_workflow.status().uncertain);
        f.client.reconcile_core().await.unwrap();
    });
}

#[test]
fn lost_backend_result_blocks_new_mutations_without_hiding_the_promoted_product() {
    let f = Fixture::new(false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.prime(&f.client).await;
        f.endpoint.set_result_missing(true);
        let error = f.client.reconcile_core().await.unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
        assert!(f.client.inner.application_workflow.status().uncertain);
        assert!(f.client.promoted_runtime().await.is_some());
        assert_eq!(
            f.client.stop_core().await.unwrap_err().kind,
            Some(CoreErrorKind::OperationConflict)
        );
        assert_eq!(f.endpoint.submissions(), 1);
    });
}

/// Two graphs share no workflow state: a reconcile in one builds nothing in
/// the other, and stopping one leaves the other able to reconcile.
#[tokio::test]
async fn a_reconcile_and_a_shutdown_stay_within_their_graph() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let (a, build_a, _, _) = workflow_graph(&dir_a).await;
    let (b, build_b, _, _) = workflow_graph(&dir_b).await;
    let reconcile_a = {
        let a = a.clone();
        tokio::spawn(async move { a.reconcile().await })
    };
    build_a.entered.notified().await;
    barrier(&b).await;
    assert_eq!(build_b.calls.load(Ordering::SeqCst), 0);
    build_a.release.notify_one();
    reconcile_a.await.unwrap().unwrap();
    let stopped = a.0.actor.get_cell();
    drop(a);
    stopped
        .wait(Some(Duration::from_secs(5)))
        .await
        .expect("the abandoned workflow stops");
    let reconcile_b = {
        let b = b.clone();
        tokio::spawn(async move { b.reconcile().await })
    };
    build_b.entered.notified().await;
    build_b.release.notify_one();
    reconcile_b.await.unwrap().unwrap();
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
    patch.break_connection = config.break_connection.into_patch();
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
        let applied = client
            .inner
            .application_workflow
            .runtime()
            .promoted
            .unwrap();
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

#[test]
fn config_commit_failure_never_reconciles_or_changes_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let args = test_client_args_with_endpoint(&dir, endpoint.clone());
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        let before = client.inner.clash_config.snapshot();
        endpoint.set_check_answer(crate::client::tests::TestCheckAnswer::Reject(
            nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::ConfigCheckFailed,
                "candidate rejected",
                false,
            ),
        ));
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
                .inner
                .application_workflow
                .runtime()
                .promoted
                .unwrap()
                .revision
                .get(),
            1
        );
    });
}

/// V08/V09 (U7): a save whose write fails after its Try applied is returned
/// as an error that names the persistence cause and what became of the
/// runtime: rolled back, or not, in which case the domain is isolated. The
/// source keeps its version either way.
#[test]
fn config_persistence_failure_restores_runtime_and_keeps_source_unchanged() {
    for restore_lost in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let scripted = ScriptedWaitEndpoint::new(endpoint.clone());
        let args = test_client_args_with_endpoint(&dir, scripted.clone());
        let path = args.paths.clash_config_path();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            endpoint.prime(&client).await;
            disable_mode_interruption(&client).await;
            let before = client.inner.clash_config.snapshot();
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
                .expect_err("the save failed")
                .to_string();
            let after = client.inner.clash_config.snapshot();
            assert_eq!(after.version, before.version);
            assert_eq!(
                serde_json::to_value(after.state).unwrap(),
                serde_json::to_value(before.state).unwrap()
            );
            assert!(error.contains("failed to persist clash config"), "{error}");
            assert!(error.contains("failed to write config"), "{error}");
            assert_eq!(
                endpoint.submissions(),
                2,
                "Try applied and Cancel resubmitted the baseline"
            );
            if restore_lost {
                assert!(error.contains("rolling the runtime back failed"), "{error}");
                assert!(error.contains("recovery required"), "{error}");
                assert!(client.inner.application_workflow.status().uncertain);
            } else {
                assert!(
                    error.contains("the runtime was rolled back to the previous configuration"),
                    "{error}"
                );
                assert!(!client.inner.application_workflow.status().uncertain);
            }
        });
    }
}

/// An installation sent after the workflow has stopped never reaches it, so it
/// certainly did not run: the caller gets a definite refusal, and the progress
/// observer its one terminal answer.
#[test]
fn an_installation_the_workflow_never_received_is_refused_as_not_run() {
    let f = Fixture::new(false, false);
    tauri::async_runtime::block_on(async {
        f.client.request_shutdown();
        f.client.wait_shutdown().await;
        let (mut artifact, _) = f.artifact(ClashCore::Mihomo);
        let terminal = Arc::new(TerminalProgress::default());
        artifact.progress = terminal.clone();
        let error = f
            .client
            .inner
            .application_workflow
            .replace_binary(artifact)
            .await
            .unwrap_err();
        assert!(error.message.contains("was not run"), "{error}");
        let outcomes = terminal.0.lock().unwrap().clone();
        assert_eq!(outcomes.len(), 1, "exactly one terminal notification");
        assert!(outcomes[0].as_ref().unwrap().contains("was not run"));
        assert_eq!(f.installer.calls.load(Ordering::SeqCst), 0);

        // Through the updater, the same refusal ends its task as a failure
        // that reserves nothing.
        use crate::core::updater::{UpdaterClient, UpdaterState};
        let updater = UpdaterClient::spawn(
            Arc::new(crate::core::updater::tests::ReadyBackend),
            Arc::new(f.client.inner.application_workflow.clone()),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        updater.fetch_latest().await.unwrap();
        let id = updater.update(ClashCore::Mihomo).await.unwrap();
        let state = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = updater.inspect(id).await.unwrap().state;
                if matches!(state, UpdaterState::Done | UpdaterState::Failed(_)) {
                    return state;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the updater task ends");
        assert!(
            matches!(&state, UpdaterState::Failed(reason) if reason.contains("was not run")),
            "{state:?}"
        );
        assert_ne!(
            updater.update(ClashCore::Mihomo).await.unwrap(),
            id,
            "a failed task reserves nothing"
        );
    });
}

/// Counts the ClashConfig notifications the client sends the UI.
struct CountingUi {
    refreshed: tokio::sync::watch::Sender<usize>,
}
impl UiEventSink for CountingUi {
    fn state_changed(&self, state: crate::client::StateChanged) {
        if matches!(state, crate::client::StateChanged::ClashConfig) {
            self.refreshed.send_modify(|count| *count += 1);
        }
    }
}

/// Waits until every notification sent so far has reached the UI, so a
/// later refresh can only come from what the test does next. An idle
/// workflow has sent its last operation's notification, the effects barrier
/// has queued what it carried, and no pending effect is left to refresh.
async fn until_notified(client: &NyanpasuClient) {
    let mut status = client.inner.application_workflow.subscribe_status();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        status.wait_for(|status| status.active.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    client.inner.effects.barrier().await;
    let mut effects = client.inner.effects.subscribe();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        effects.wait_for(|snapshot| {
            snapshot.effects.iter().all(|progress| {
                progress.health != crate::client::convergence::ConvergenceHealth::Pending
            })
        }),
    )
    .await
    .unwrap()
    .unwrap();
}

#[test]
fn an_override_patch_submits_once_and_notifies_the_ui() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let ui = Arc::new(CountingUi {
        refreshed: tokio::sync::watch::Sender::new(0),
    });
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.ui_sink = ui.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        disable_mode_interruption(&client).await;
        until_notified(&client).await;
        let mut refreshed = ui.refreshed.subscribe();
        let before = *refreshed.borrow_and_update();
        let outcome = client
            .patch_runtime_overrides(override_patch(serde_json::json!({"mode":"global"})))
            .await
            .unwrap();
        assert_eq!(endpoint.submissions(), 1);
        assert!(outcome.degradations().is_empty());
        assert_eq!(
            client
                .inner
                .application_workflow
                .runtime()
                .promoted
                .unwrap()
                .config["mode"]
                .as_str(),
            Some("global")
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            refreshed.wait_for(|count| *count > before),
        )
        .await
        .expect("the UI hears about the ClashConfig change")
        .unwrap();
    });
}

/// A core reconcile refreshes the UI's clash view, as every lifecycle command
/// does; the removed rebuild route only repeated this refresh.
#[test]
fn a_core_reconcile_notifies_the_ui() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let ui = Arc::new(CountingUi {
        refreshed: tokio::sync::watch::Sender::new(0),
    });
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.ui_sink = ui.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        endpoint.prime(&client).await;
        until_notified(&client).await;
        let mut refreshed = ui.refreshed.subscribe();
        let before = *refreshed.borrow_and_update();
        client.reconcile_core().await.unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            refreshed.wait_for(|count| *count > before),
        )
        .await
        .expect("the UI hears about the reconcile")
        .unwrap();
    });
}

/// The explicit start a reconcile becomes while no owner is proven refreshes
/// the clash view too.
#[test]
fn an_explicit_start_notifies_the_ui() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    // No StartupReconcile runs, so nothing proves who owns the stopped core.
    endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
        None,
    );
    let ui = Arc::new(CountingUi {
        refreshed: tokio::sync::watch::Sender::new(0),
    });
    let mut args = test_client_args_with_endpoint(&dir, endpoint.clone());
    args.ui_sink = ui.clone();
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let workflow = &client.inner.application_workflow;
        assert_eq!(ownership(workflow).await, Ownership::Unproven);
        until_notified(&client).await;
        let mut refreshed = ui.refreshed.subscribe();
        let before = *refreshed.borrow_and_update();
        client.reconcile_core().await.unwrap();
        assert_eq!(
            ownership(workflow).await,
            Ownership::Established {
                host: ExecutionHost::Local
            }
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            refreshed.wait_for(|count| *count > before),
        )
        .await
        .expect("the UI hears about the explicit start")
        .unwrap();
    });
}

#[test]
fn a_control_channel_patch_applies_the_committed_channel() {
    use nyanpasu_config::clash::config::{ClashConfig, ClashControlChannel};
    use nyanpasu_core_manager::LocalIpcPolicy;

    let f = Fixture::new(false, false);
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
            let settings = f.endpoint.local_ipc.lock().unwrap().unwrap();
            assert_eq!(settings.policy, policy);
            assert_eq!(settings.keep_http_controller, !disable_http);
        }
    });
}

#[test]
fn a_control_channel_patch_does_not_start_a_stopped_core() {
    use nyanpasu_config::clash::config::{ClashConfig, ClashControlChannel};

    let f = Fixture::new(false, false);
    tauri::async_runtime::block_on(async {
        f.endpoint.prime(&f.client).await;
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        let mut patch = ClashConfig::new_empty_patch();
        patch.clash_control_channel = Some(ClashControlChannel::HttpOnly);
        f.client.patch_clash_config(patch).await.unwrap();
        assert_eq!(f.endpoint.submissions(), 0);
        f.endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
            Some(nyanpasu_core_manager::CoreKind::Mihomo),
        );
        let mut patch = ClashConfig::new_empty_patch();
        patch.clash_control_channel = Some(ClashControlChannel::PreferIpc);
        f.client.patch_clash_config(patch).await.unwrap();
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

/// V27: an installation refused before it runs settles its progress observer
/// exactly once, whether the shutdown or an isolated execution domain refused
/// it. It waits behind a parked installation, and its caller has already given
/// up on it.
#[test]
fn queued_installation_timeout_is_settled_when_shutdown_or_uncertainty_rejects_it() {
    for isolate in [false, true] {
        let f = Fixture::new(true, false);
        tauri::async_runtime::block_on(async {
            // A running core, so the installation ends in a restart.
            f.endpoint.prime(&f.client).await;
            let (first, _) = start_replacement(&f).await;
            let (mut queued, _) = f.artifact(ClashCore::ClashRs);
            let terminal = Arc::new(TerminalProgress::default());
            queued.progress = terminal.clone();
            let client = &f.client.inner.application_workflow;
            // Polling once sends the request; dropping the call then is the
            // caller giving up on it.
            let mut call =
                Box::pin(client.call(Command::Core(CoreCommand::ReplaceCoreBinary(queued))));
            assert!(call.as_mut().now_or_never().is_none());
            drop(call);
            assert!(terminal.0.lock().unwrap().is_empty());
            if isolate {
                // The parked installation's restart loses its result.
                f.endpoint.set_result_missing(true);
                f.installer.release.notify_one();
                assert!(first.await.unwrap().is_err());
                barrier(client).await;
                assert!(client.status().uncertain);
            } else {
                f.client.request_shutdown();
                f.installer.release.notify_one();
                first.await.unwrap().unwrap();
                f.client.wait_shutdown().await;
            }
            let outcomes = terminal.0.lock().unwrap().clone();
            assert_eq!(
                outcomes.len(),
                1,
                "rejected request must deliver exactly one terminal notification"
            );
            assert!(outcomes[0].as_ref().unwrap().contains(if isolate {
                "uncertain outcome"
            } else {
                "shutting down"
            }));
            assert_eq!(f.installer.calls.load(Ordering::SeqCst), 1);
        });
    }
}
