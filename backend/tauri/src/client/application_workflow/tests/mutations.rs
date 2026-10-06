//! T5: a source-config mutation as a Required participant of its own state
//! transaction.
//!
//! Every test here drives a real `PersistentStateManager` through
//! `replace_if_version_with_participant`, so the prepare / commit / rollback
//! ordering under test is the coordinator's own and not a re-implementation of
//! it. The runtime underneath is the fake control endpoint, and the points the
//! tests need to hold still are barriers and channels rather than sleeps: the
//! builder parks its first build, and the transaction's `local_write` step
//! parks between prepare and the compare-and-swap.

use std::{
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig},
    clash::config::{
        ClashConfig,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    },
    profile::{ManagedProfilePath, ProfileId, Profiles},
};
use nyanpasu_core::state::{
    Ack, AckPolicy, AckStatus, PersistentStateManager, PersistentStateManagerSetup,
    ReplaceIfVersionError, ReplaceIfVersionResult, RollbackReason, StateAckSubscriber, StateChange,
    StateParticipant, SubscriberName, error::StateChangedError,
};
use nyanpasu_core_manager::{CoreErrorKind, CoreKind, OperationId};
use nyanpasu_ipc::api::status::CoreStateDetail;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::Notify;

use super::{
    super::{
        ApplicationWorkflowArgs, ApplicationWorkflowClient, adapters,
        impact::{
            ActivationIntent, ContentDigest, MutationHints, RequestedRuntimeFields, RuntimeImpact,
            TouchedContent, runtime_impact,
        },
        mutation::{
            CheckRecord, DEFERRED_RETRY_BUDGET, EvidenceGap, MutationConclusion,
            MutationOutcomeKind, MutationReceipt,
        },
        participant::ApplicationMutationParticipant,
        policy::CommandClass,
    },
    RecordingNotifications, ScriptedWaitEndpoint, classify, refusals,
};
use crate::{
    client::{
        SessionPortResolver,
        core_lifecycle::Ownership,
        runtime,
        runtime_error::{RuntimeError, refusal_of},
        tests::{TestCheckAnswer, TestControlEndpoint},
    },
    core::actor_v2::{
        CoreClient,
        endpoint::ExecutionHost,
        service_actor::{ServiceClient, ServiceHostAdapter},
    },
    state::mutation::{CommitAborted, RuntimeAftermath},
};

// -- fixture ---------------------------------------------------------------

/// The real build, with a switch that holds a candidate inside it.
///
/// Parking the build is how these tests keep a Try in flight for as long as they
/// need without a sleep: the tracked task is genuinely mid-operation, which is
/// the state every ordering claim here is about.
pub(super) struct ParkingBuilder {
    delegate: adapters::FsRuntimeBuildAdapter,
    pub(super) entered: Notify,
    pub(super) release: Notify,
    pub(super) park: AtomicBool,
    calls: AtomicUsize,
    pub(super) fail_publish: AtomicBool,
}

#[async_trait::async_trait]
impl super::super::ports::RuntimeBuildPort for ParkingBuilder {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> super::super::inputs::FrozenProfileContent {
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.park.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.delegate
            .build(revision, inputs, ports, strict_transforms)
            .await
    }
    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), crate::client::runtime::PublishRuntimeError> {
        if self.fail_publish.load(Ordering::SeqCst) {
            return Err(super::scripted_publish_failure());
        }
        self.delegate.publish(snapshot).await
    }
}

pub(super) struct Fixture {
    pub(super) client: ApplicationWorkflowClient,
    pub(super) endpoint: Arc<TestControlEndpoint>,
    pub(super) builder: Arc<ParkingBuilder>,
    pub(super) application: PersistentStateManager<NyanpasuAppConfig>,
    pub(super) clash: PersistentStateManager<ClashConfig>,
    profiles: PersistentStateManager<Profiles>,
    pub(super) store: runtime::RuntimeSnapshotStore,
    pub(super) clash_path: Utf8PathBuf,
    app_path: Utf8PathBuf,
    /// Where the build reads managed profile content from, so a test that
    /// drives the profiles domain can put a real document behind a path.
    profiles_dir: std::path::PathBuf,
    /// The same client the workflow holds, so a test can read which host owns
    /// the runtime at a point of its choosing.
    core: CoreClient,
    /// The same resolver the workflow holds, and the one the rest of the
    /// application reads its endpoint from.
    ports: Arc<SessionPortResolver>,
    /// The endpoint the core is reached through, when a test scripts how its
    /// operation waits answer.
    pub(super) scripted: Option<Arc<ScriptedWaitEndpoint>>,
    /// What the workflow told the effects owner.
    notifications: Arc<RecordingNotifications>,
    /// The workflow's shutdown token, and the task that waits for it to stop
    /// once the token is cancelled.
    pub(super) shutdown: tokio_util::sync::CancellationToken,
    pub(super) tasks: tokio_util::task::TaskTracker,
    _dir: tempfile::TempDir,
}

pub(super) async fn manager<T>(path: Utf8PathBuf, state: T) -> PersistentStateManager<T>
where
    T: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
{
    PersistentStateManagerSetup::<T>::builder()
        .config_path(path)
        .assemble()
        .from_state(state)
        .await
        .map_err(|error| format!("{error:?}"))
        .expect("the state manager should initialize")
}

pub(super) fn temp_path(dir: &tempfile::TempDir, name: &str) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("temp path should be UTF-8")
}

/// The confirmed apply a session normally carries from its boot reconcile.
///
/// Its identity and exact bytes match the fresh fake endpoint observation.
/// A stale or unrelated receipt is tested explicitly rather than admitted by
/// the default fixture.
pub(super) fn adopted_baseline() -> runtime::RuntimeApplyReceipt {
    runtime::RuntimeApplyReceipt {
        revision: runtime::tests::test_revision(),
        config_text: Arc::from("mode: rule\n"),
        config_digest: nyanpasu_core_manager::payload_digest(b"mode: rule\n"),
        target_core: ClashCore::default(),
        core_spec: nyanpasu_core_manager::CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-core"),
            version: None,
            features: Vec::new(),
        },
        host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
        run_intent: super::super::policy::CoreRunIntent::Running,
        local_ipc: nyanpasu_core_manager::LocalIpcSettings {
            policy: nyanpasu_core_manager::LocalIpcPolicy::Disable,
            keep_http_controller: true,
        },
        binding: crate::core::actor_v2::facade::AppliedConfigBinding {
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: nyanpasu_core_manager::payload_digest(b"mode: rule\n"),
                effective_hash: "effective".into(),
            },
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            generation: 0,
        },
        ports: SessionPortResolver::default()
            .resolve_candidate(&crate::client::tests::test_clash_config())
            .expect("the default port strategies resolve"),
        target: None,
    }
}

/// A workflow wired to three real state managers and a fake control endpoint.
///
/// The endpoint publishes a running core with a known applied kind on purpose:
/// the baseline a Cancel restores is verified against what the host says is
/// running, and an absent fact is a mismatch rather than a benefit of the doubt.
pub(super) async fn fixture() -> Fixture {
    fixture_with(true).await
}

/// The same graph with nothing applied yet: a running core this session has no
/// receipt for. That is the R10 evidence gap, and it is what a session whose
/// boot reconcile failed actually looks like.
async fn fixture_without_a_confirmed_apply() -> Fixture {
    fixture_with(false).await
}

async fn fixture_with(confirmed_apply: bool) -> Fixture {
    fixture_with_hosts(confirmed_apply, None).await
}

/// The same graph with a second execution host available, so a mutation that
/// asks for one can actually be given it. `service` is the endpoint the daemon
/// hands out once it is adopted.
async fn fixture_with_hosts(
    confirmed_apply: bool,
    service_host: Option<Arc<TestControlEndpoint>>,
) -> Fixture {
    let daemon = service_host
        .map(|endpoint| Arc::new(host_transition_daemon(endpoint)) as Arc<dyn ServiceHostAdapter>);
    fixture_with_daemon(confirmed_apply, daemon).await
}

/// The ordinary daemon over a service-side control endpoint, which the tests
/// that need to park one of its legs wrap instead of reimplementing.
fn host_transition_daemon(
    endpoint: Arc<TestControlEndpoint>,
) -> crate::client::tests::HostTransitionServiceAdapter {
    crate::client::tests::HostTransitionServiceAdapter {
        endpoint,
        calls: Arc::new(StdMutex::new(Vec::new())),
        stopped: AtomicBool::new(false),
        installed_for: Default::default(),
    }
}

/// The same graph with the caller's own daemon, for a test that has to hold one
/// of the handoff's legs still.
pub(super) async fn fixture_with_daemon(
    confirmed_apply: bool,
    daemon: Option<Arc<dyn ServiceHostAdapter>>,
) -> Fixture {
    fixture_with_parts(confirmed_apply, daemon, false).await
}

/// The default graph, with the core reached through a [`ScriptedWaitEndpoint`]
/// so a test can lose or stall the wait for one chosen operation.
pub(super) async fn scripted_fixture() -> Fixture {
    fixture_with_parts(true, None, true).await
}

async fn fixture_with_parts(
    confirmed_apply: bool,
    daemon: Option<Arc<dyn ServiceHostAdapter>>,
    scripted: bool,
) -> Fixture {
    let service = match daemon {
        Some(daemon) => ServiceClient::spawn(daemon, 0).await.unwrap(),
        None => ServiceClient::spawn(Arc::new(crate::client::tests::IdleServiceAdapter), 0)
            .await
            .unwrap(),
    };
    fixture_from(
        confirmed_apply,
        service,
        scripted,
        Ownership::Established {
            host: ExecutionHost::Local,
        },
    )
    .await
}

/// The graph over the caller's own service client, starting from `ownership`.
/// A test about proving the owner starts `Unproven`, as production does.
pub(super) async fn fixture_from(
    confirmed_apply: bool,
    service: ServiceClient,
    scripted: bool,
    ownership: Ownership,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_source_hash(&nyanpasu_core_manager::payload_digest(b"mode: rule\n"));
    endpoint.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );
    let scripted = scripted.then(|| ScriptedWaitEndpoint::new(endpoint.clone()));
    let core = match &scripted {
        Some(scripted) => CoreClient::spawn(scripted.clone()).await.unwrap(),
        None => CoreClient::spawn(endpoint.clone()).await.unwrap(),
    };

    let clash_path = temp_path(&dir, "clash-config.yaml");
    let app_path = temp_path(&dir, "application.yaml");
    let application = manager(app_path.clone(), NyanpasuAppConfig::default()).await;
    let clash = manager(
        clash_path.clone(),
        crate::client::tests::test_clash_config(),
    )
    .await;
    let profiles = manager(temp_path(&dir, "profiles.yaml"), Profiles::default()).await;

    let paths = runtime::RuntimePaths::from_resolver(&crate::client::tests::test_paths(
        dir.path(),
        dir.path().join("data"),
    ))
    .unwrap();
    let builder = Arc::new(ParkingBuilder {
        delegate: adapters::FsRuntimeBuildAdapter {
            core_specs: Arc::new(crate::client::runtime_core_spec),
            profiles_dir: dir.path().join("profiles"),
            paths: paths.clone(),
            scripts: crate::enhance::ScriptDirs::under(dir.path()),
        },
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        park: AtomicBool::new(false),
        fail_publish: AtomicBool::new(false),
    });
    let store = runtime::RuntimeSnapshotStore::default();
    let ports = Arc::new(SessionPortResolver::new(store.clone()));
    if confirmed_apply {
        // A Try is only admitted while a Cancel would have something to put
        // back (R10). The host here reports a running core, so the session needs
        // the confirmed apply that production gets from its boot reconcile.
        // Nothing ever restores this one: every test that cancels primes its own
        // baseline with a real mutation first, and that receipt supersedes this.
        store.confirm_applied(Arc::new(adopted_baseline()));
    }
    let (shutdown, tasks) = (
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
    );
    let notifications = Arc::new(RecordingNotifications::default());
    let client = ApplicationWorkflowClient::spawn_with_ticks(
        ApplicationWorkflowArgs {
            notifications: notifications.clone(),
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            core: core.clone(),
            service,
            builder: builder.clone(),
            validator: Arc::new(adapters::CoreCheckValidator::new(core.clone(), paths)),
            ports: ports.clone(),
            installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
            ownership,
            instance_config_dir: Default::default(),
            shutdown: shutdown.clone(),
            tasks: tasks.clone(),
        },
        false,
    )
    .await
    .unwrap();
    Fixture {
        client,
        endpoint,
        builder,
        application,
        clash,
        profiles,
        store,
        clash_path,
        app_path,
        profiles_dir: dir.path().join("profiles"),
        core,
        ports,
        scripted,
        notifications,
        shutdown,
        tasks,
        _dir: dir,
    }
}

pub(super) type Decorate<T> = Box<dyn FnOnce(StateParticipant<T>) -> StateParticipant<T> + Send>;

pub(super) fn plain<T: Clone + Send + Sync + 'static>() -> Decorate<T> {
    Box::new(|participant| participant)
}

/// Runs one mutation of `manager` the way a source does: with the workflow as
/// its Required participant when it reaches the runtime, and as a plain save
/// when it does not.
///
/// The attempt identity is the caller's so a test can address a settlement
/// before the attempt has finished, exactly as the domain actor will.
pub(super) async fn mutate<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    operation_id: OperationId,
    next: T,
    class: CommandClass,
    decorate: Decorate<T>,
    local_write: impl FnOnce() -> futures::future::BoxFuture<'static, anyhow::Result<()>>
    + Send
    + 'static,
) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
where
    T: super::super::mutation::MutationDomain
        + Clone
        + Send
        + Sync
        + 'static
        + Serialize
        + DeserializeOwned
        + Default,
    super::super::mutation::DomainChange: From<StateChange<T>>,
{
    mutate_settling(
        manager,
        client,
        operation_id,
        next,
        class,
        decorate,
        local_write,
    )
    .await
    .0
}

/// The same mutation, with the settlement the Runtime sends its source. It
/// arrives once Confirm or Cancel is over, which a test may be holding still,
/// so awaiting it is the caller's choice. A plain save has none.
pub(super) async fn mutate_settling<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    operation_id: OperationId,
    next: T,
    class: CommandClass,
    decorate: Decorate<T>,
    local_write: impl FnOnce() -> futures::future::BoxFuture<'static, anyhow::Result<()>>
    + Send
    + 'static,
) -> (
    Result<ReplaceIfVersionResult, ReplaceIfVersionError>,
    crate::state::mutation::Settlement,
)
where
    T: super::super::mutation::MutationDomain
        + Clone
        + Send
        + Sync
        + 'static
        + Serialize
        + DeserializeOwned
        + Default,
    super::super::mutation::DomainChange: From<StateChange<T>>,
{
    let hints = MutationHints::default();
    let (version, impact) = {
        let current = manager.snapshot_handle().load();
        let impact = runtime_impact(&current.state, &next, &hints, class);
        (current.version, impact)
    };
    let client = client.clone();
    let (settle, settlement) = tokio::sync::oneshot::channel();
    let Some(impact) = impact else {
        let result = manager
            .replace_if_version_with_local_write(version, next, local_write, || async { Ok(()) })
            .await;
        return (result, settlement);
    };
    let result = manager
        .replace_if_version_with_participant(
            version,
            next,
            move |decision| {
                decorate(ApplicationMutationParticipant::<T>::new(
                    operation_id,
                    hints,
                    class,
                    impact,
                    decision,
                    client,
                    settle,
                ))
            },
            local_write,
            || async { Ok(()) },
        )
        .await;
    (result, settlement)
}

/// One mutation carrying request-local hints, which is how a content update
/// says that the bytes behind a managed path moved without the document doing
/// so.
pub(super) async fn mutate_with_hints<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    next: T,
    class: CommandClass,
    hints: MutationHints,
) -> (
    OperationId,
    Result<ReplaceIfVersionResult, ReplaceIfVersionError>,
)
where
    T: super::super::mutation::MutationDomain
        + Clone
        + Send
        + Sync
        + 'static
        + Serialize
        + DeserializeOwned
        + Default,
    super::super::mutation::DomainChange: From<StateChange<T>>,
{
    let operation_id = OperationId::generate();
    let (version, impact) = {
        let current = manager.snapshot_handle().load();
        let impact = runtime_impact(&current.state, &next, &hints, class);
        (current.version, impact)
    };
    let Some(impact) = impact else {
        return (
            operation_id,
            manager.replace_if_version(version, next).await,
        );
    };
    let client = client.clone();
    let (settle, _settlement) = tokio::sync::oneshot::channel();
    let result = manager
        .replace_if_version_with_participant(
            version,
            next,
            move |decision| {
                ApplicationMutationParticipant::<T>::new(
                    operation_id,
                    hints,
                    class,
                    impact,
                    decision,
                    client,
                    settle,
                )
            },
            no_local_write,
            || async { Ok(()) },
        )
        .await;
    (operation_id, result)
}

/// The ordinary case: a fresh identity, no decorator, no local write.
pub(super) async fn simple_mutate<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    next: T,
    class: CommandClass,
) -> (
    OperationId,
    Result<ReplaceIfVersionResult, ReplaceIfVersionError>,
)
where
    T: super::super::mutation::MutationDomain
        + Clone
        + Send
        + Sync
        + 'static
        + Serialize
        + DeserializeOwned
        + Default,
    super::super::mutation::DomainChange: From<StateChange<T>>,
{
    let operation_id = OperationId::generate();
    let result = mutate(
        manager,
        client,
        operation_id,
        next,
        class,
        plain(),
        no_local_write,
    )
    .await;
    (operation_id, result)
}

/// Parks the transaction between prepare and the compare-and-swap, which is
/// exactly the window in which the workflow sits in `AwaitDecision`.
pub(super) fn parked_local_write(
    entered: Arc<Notify>,
    release: Arc<Notify>,
) -> impl FnOnce() -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
    move || {
        Box::pin(async move {
            entered.notify_one();
            release.notified().await;
            Ok(())
        })
    }
}

pub(super) fn refused(result: &Result<ReplaceIfVersionResult, ReplaceIfVersionError>) -> bool {
    matches!(
        result,
        Err(ReplaceIfVersionError::State(StateChangedError::PrepareAck(
            _
        )))
    )
}

/// A host that has the check capability and could not serve it: the class the
/// failure matrix defaults to refusing, and the one that carries a typed
/// retryability the adapter actually observed.
pub(super) fn unserviceable_check() -> nyanpasu_core_manager::CoreError {
    nyanpasu_core_manager::CoreError::new(
        CoreErrorKind::BackendUnavailable,
        "scripted: the config check service is briefly unreachable",
        true,
    )
}

pub(super) fn app_with_core(core: ClashCore) -> NyanpasuAppConfig {
    NyanpasuAppConfig {
        core,
        ..NyanpasuAppConfig::default()
    }
}

pub(super) fn no_local_write() -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
    Box::pin(std::future::ready(Ok(())))
}

/// The structured record of one attempt, once the workflow has settled it.
pub(super) async fn settled(
    client: &ApplicationWorkflowClient,
    operation_id: OperationId,
) -> MutationReceipt {
    let mut journal = client.0.mutations.clone();
    let guard = tokio::time::timeout(
        Duration::from_secs(5),
        journal.wait_for(|journal| {
            journal
                .completed
                .iter()
                .any(|receipt| receipt.operation_id == operation_id)
        }),
    )
    .await
    .expect("the mutation should settle")
    .expect("the workflow should stay alive");
    guard
        .completed
        .iter()
        .find(|receipt| receipt.operation_id == operation_id)
        .cloned()
        .expect("the receipt was just matched")
}

/// The evidence a clash request carries when the user's patch addressed the
/// overrides — which is where every `overrides(...)` document below comes from.
/// A struct-patch field is "named" by being present, so this is what a save of
/// the overrides looks like whether or not the value it carries moved.
pub(super) fn names_overrides() -> MutationHints {
    MutationHints {
        requested: RequestedRuntimeFields::runtime(),
        ..MutationHints::default()
    }
}

/// A clash config that asks for a different mixed port than the default one,
/// so the apply it produces confirms a port binding of its own. `AllowFallback`
/// rather than `Fixed`: a busy port moves the pick instead of failing the
/// resolve, and every claim here is about the binding changing, not about which
/// number it landed on.
fn on_mixed_port(start_port: u16) -> ClashConfig {
    ClashConfig {
        mixed_port: PortStrategy {
            kind: PortStrategyKind::AllowFallback,
            start_port,
        },
        ..ClashConfig::default()
    }
}

pub(super) fn overrides(value: serde_json::Value) -> ClashConfig {
    use struct_patch::Patch;
    let mut config = crate::client::tests::test_clash_config();
    config
        .overrides
        .apply(serde_json::from_value(value).unwrap());
    config
}

// -- participants used to script the transaction ---------------------------

/// A registered subscriber that vetoes every prepare, so the transaction aborts
/// after the workflow's Try has already run.
pub(super) struct Rejector;

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateAckSubscriber<T> for Rejector {
    fn name(&self) -> SubscriberName<'_> {
        "rejector".into()
    }
    async fn on_prepare(&self, _change: StateChange<T>) -> Ack {
        Ack::Rejected(Arc::new(std::io::Error::other("scripted domain veto")))
    }
}

/// Forwards everything except one notification, the shape of a settlement
/// signal that was dropped on its way to the workflow.
struct DropSignal<T: Clone + Send + Sync + 'static> {
    inner: StateParticipant<T>,
    drop_committed: bool,
}

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateAckSubscriber<T> for DropSignal<T> {
    fn name(&self) -> SubscriberName<'_> {
        self.inner.name()
    }
    fn policy(&self) -> AckPolicy {
        self.inner.policy()
    }
    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        self.inner.on_prepare(change).await
    }
    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        if self.drop_committed {
            return Ack::Ok;
        }
        self.inner.on_committed(change).await
    }
    async fn on_rolled_back(&self, change: StateChange<T>, reason: RollbackReason) {
        if !self.drop_committed {
            return;
        }
        self.inner.on_rolled_back(change, reason).await;
    }
}

/// Drops one settlement signal while keeping the participant alive, so the
/// fallback under test is the decision handle and not the participant's own
/// `Drop` repairing the loss a moment later.
fn drop_signal<T: Clone + Send + Sync + 'static>(
    drop_committed: bool,
    kept: Arc<StdMutex<Option<StateParticipant<T>>>>,
) -> Decorate<T> {
    Box::new(move |participant| {
        *kept.lock().unwrap() = Some(Arc::clone(&participant));
        Arc::new(DropSignal {
            inner: participant,
            drop_committed,
        })
    })
}

/// Signals when the participant's prepare begins, which is when it sends its
/// Try to the workflow.
struct PrepareEntered<T: Clone + Send + Sync + 'static> {
    inner: StateParticipant<T>,
    entered: Arc<Notify>,
}

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateAckSubscriber<T> for PrepareEntered<T> {
    fn name(&self) -> SubscriberName<'_> {
        self.inner.name()
    }
    fn policy(&self) -> AckPolicy {
        self.inner.policy()
    }
    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        self.entered.notify_one();
        self.inner.on_prepare(change).await
    }
    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        self.inner.on_committed(change).await
    }
    async fn on_rolled_back(&self, change: StateChange<T>, reason: RollbackReason) {
        self.inner.on_rolled_back(change, reason).await;
    }
}

fn signal_prepare<T: Clone + Send + Sync + 'static>(entered: Arc<Notify>) -> Decorate<T> {
    Box::new(move |participant| {
        Arc::new(PrepareEntered {
            inner: participant,
            entered,
        })
    })
}

// -- R4: admission gates the commit ----------------------------------------

/// Closing refuses new mutations, and it refuses them *before* anything is
/// persisted. Under the pre-participant shape the facade had already committed
/// by the time the workflow could say no.
#[tokio::test]
async fn a_closing_workflow_refuses_a_mutation_before_anything_is_committed() {
    let mut f = fixture().await;
    f.shutdown.cancel();

    let before = f.application.snapshot_handle().load().version;
    let (_, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(f.application.snapshot_handle().load().version, before);
    assert_eq!(f.application.snapshot().core, ClashCore::default());
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "a refused mutation never reaches the core"
    );
}

/// An isolated execution domain refuses mutations too: nothing may be committed
/// against a runtime nobody can describe.
#[tokio::test]
async fn an_isolated_execution_domain_refuses_a_mutation() {
    let mut f = fixture().await;
    f.endpoint.set_result_missing(true);
    f.client
        .reconcile()
        .await
        .expect_err("an unobserved reconcile is not an applied one");
    assert!(f.client.status().uncertain);
    f.endpoint.set_result_missing(false);

    let before = f.clash.snapshot_handle().load().version;
    let (_, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(f.clash.snapshot_handle().load().version, before);
}

// -- R5: commit order is apply order ---------------------------------------

/// Two domains mutate concurrently. The workflow holds its execution domain
/// from the Try through the decision, so the second mutation is admitted only
/// after the first has committed — and then reads what it committed (v2 §5.2,
/// V20).
#[tokio::test]
async fn a_second_domain_is_admitted_only_after_the_first_commits_and_reads_it() {
    let Fixture {
        client,
        builder,
        mut application,
        mut clash,
        store,
        _dir,
        ..
    } = fixture().await;
    builder.park.store(true, Ordering::SeqCst);

    let first = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = simple_mutate(
                &mut application,
                &client,
                app_with_core(ClashCore::ClashRs),
                CommandClass::ExplicitSwitch,
            )
            .await;
            (application, result)
        })
    };
    builder.entered.notified().await;

    let second_prepared = Arc::new(Notify::new());
    let second = {
        let client = client.clone();
        let entered = second_prepared.clone();
        tokio::spawn(async move {
            let operation_id = OperationId::generate();
            let result = mutate(
                &mut clash,
                &client,
                operation_id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                signal_prepare(entered),
                no_local_write,
            )
            .await;
            (clash, (operation_id, result))
        })
    };
    // The second Try is sent as its prepare begins, and waits behind the
    // first in the workflow's mailbox.
    second_prepared.notified().await;
    assert_eq!(
        builder.calls.load(Ordering::SeqCst),
        1,
        "the queued mutation must not build ahead of admission"
    );

    builder.park.store(false, Ordering::SeqCst);
    builder.release.notify_one();
    let (application, (_, first_result)) = first.await.unwrap();
    let (clash, (second_id, second_result)) = second.await.unwrap();
    assert!(matches!(first_result, Ok(ReplaceIfVersionResult::Replaced)));
    assert!(matches!(
        second_result,
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&client, second_id).await.conclusion,
        MutationConclusion::Confirmed
    );

    assert_eq!(application.snapshot().core, ClashCore::ClashRs);
    let published = store.read().promoted.expect("the second confirm publishes");
    assert_eq!(
        published.target_core,
        ClashCore::ClashRs,
        "the second mutation built against the application the first committed"
    );
    assert_eq!(published.config["mode"].as_str(), Some("global"));
    drop(clash);
}

// -- settlement routing ----------------------------------------------------

/// The commit notification never arrives. The authoritative decision still
/// proves the commit, so the workflow confirms rather than hanging or guessing
/// Cancel (V25).
#[tokio::test]
async fn a_lost_commit_notification_is_resolved_by_the_authoritative_decision() {
    let Fixture {
        client,
        mut clash,
        store,
        _dir,
        ..
    } = fixture().await;
    let kept = Arc::new(StdMutex::new(None));

    let operation_id = OperationId::generate();
    let result = mutate(
        &mut clash,
        &client,
        operation_id,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
        drop_signal(true, kept.clone()),
        no_local_write,
    )
    .await;

    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Confirmed);
    // The derived product is published only on the Confirm path, so its
    // presence is the proof that path ran.
    assert_eq!(
        store
            .read()
            .promoted
            .expect("confirm publishes the product")
            .config["mode"]
            .as_str(),
        Some("global")
    );
    assert!(kept.lock().unwrap().is_some());
}

/// The rollback notification never arrives. The authoritative decision says
/// Aborted, so the workflow cancels and puts the baseline back (V26). The
/// execution domain stays held until that restore is done: the next mutation's
/// Try starts only after it.
#[tokio::test]
async fn a_lost_rollback_notification_is_resolved_by_the_authoritative_decision() {
    let Fixture {
        client,
        endpoint,
        builder,
        mut application,
        mut clash,
        store,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let baseline = store
        .last_confirmed_runtime_receipt()
        .expect("the primed apply is the baseline");

    clash.add_subscriber(Box::new(Rejector));
    let kept = Arc::new(StdMutex::new(None));
    let operation_id = OperationId::generate();
    let result = mutate(
        &mut clash,
        &client,
        operation_id,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        drop_signal(false, kept.clone()),
        no_local_write,
    )
    .await;
    assert!(refused(&result), "{result:?}");

    // The veto reaches its caller while the Cancel may still be restoring. The
    // next mutation parks as soon as its Try starts, and by then the restore
    // has to be complete.
    builder.park.store(true, Ordering::SeqCst);
    let next = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = simple_mutate(
                &mut application,
                &client,
                app_with_core(ClashCore::ClashRs),
                CommandClass::ExplicitSwitch,
            )
            .await;
            (application, result)
        })
    };
    builder.entered.notified().await;
    let submitted = endpoint.reconciled_bytes();
    assert_eq!(submitted.len(), 3, "primed, tried, restored");
    assert_eq!(
        submitted[2], submitted[0],
        "the restore resubmits the baseline document, not the withdrawn candidate"
    );
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .expect("the baseline is the checkpoint again")
            .config_digest,
        baseline.config_digest
    );

    builder.park.store(false, Ordering::SeqCst);
    builder.release.notify_one();
    let (_application, (next_id, next_result)) = next.await.unwrap();
    assert!(
        matches!(next_result, Ok(ReplaceIfVersionResult::Replaced)),
        "{next_result:?}"
    );
    assert_eq!(
        settled(&client, next_id).await.conclusion,
        MutationConclusion::Confirmed
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
    assert!(kept.lock().unwrap().is_some());
}

// -- L3-4: the Runtime hands the effects owner only its own slice -----------

/// Confirm and Cancel each hand the effects owner the ports the core is bound
/// to, once, before the source hears back. A source's own slice never comes
/// from the Runtime: the recording refuses it.
#[tokio::test]
async fn confirm_and_cancel_each_hand_the_effects_owner_the_runtime_slice() {
    let mut f = fixture().await;
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settlement.await.unwrap().conclusion,
        MutationConclusion::Confirmed
    );
    assert_eq!(f.notifications.bound(), 1);

    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        plain(),
        || Box::pin(async { Err(anyhow::anyhow!("the disk is full")) }),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        settlement.await.unwrap().conclusion,
        MutationConclusion::Cancelled
    );
    assert_eq!(f.notifications.bound(), 2);
}

// -- V02: the decision is waited for as long as the write takes -------------

/// The Try succeeded and the source write outlives the 120 s the decision wait
/// used to allow. The workflow keeps waiting, and the decision that finally
/// arrives settles the attempt: a committed write is confirmed, a failed one is
/// cancelled back to the baseline. The clock is virtual, so nothing sleeps.
#[tokio::test(start_paused = true)]
async fn a_slow_source_write_is_waited_out_and_its_decision_settles_the_attempt() {
    for commit in [true, false] {
        let mut f = fixture().await;
        let (primed, result) = simple_mutate(
            &mut f.clash,
            &f.client,
            overrides(serde_json::json!({"mode": "direct"})),
            CommandClass::Save,
        )
        .await;
        assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
        settled(&f.client, primed).await;
        let baseline = f.store.last_confirmed_runtime_receipt().unwrap();

        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let id = OperationId::generate();
        let mutation = {
            let client = f.client.clone();
            let (entered, release) = (entered.clone(), release.clone());
            let mut clash = f.clash;
            tokio::spawn(async move {
                let result = mutate(
                    &mut clash,
                    &client,
                    id,
                    overrides(serde_json::json!({"mode": "global"})),
                    CommandClass::Save,
                    plain(),
                    move || {
                        Box::pin(async move {
                            entered.notify_one();
                            release.notified().await;
                            anyhow::ensure!(commit, "scripted source write failure");
                            Ok(())
                        })
                    },
                )
                .await;
                (clash, result)
            })
        };
        entered.notified().await;
        tokio::time::advance(Duration::from_secs(121)).await;
        assert!(
            f.client
                .mutation_journal()
                .completed
                .iter()
                .all(|receipt| receipt.operation_id != id),
            "the attempt is still waiting for its decision"
        );
        assert_eq!(f.client.status().active, Some(id));
        assert!(!f.client.status().uncertain);

        release.notify_one();
        let (clash, result) = mutation.await.unwrap();
        let receipt = settled(&f.client, id).await;
        assert!(!f.client.status().uncertain);
        let promoted = f.store.read().promoted.unwrap();
        if commit {
            assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
            assert_eq!(receipt.conclusion, MutationConclusion::Confirmed);
            assert_eq!(promoted.config["mode"].as_str(), Some("global"));
        } else {
            assert!(
                matches!(result, Err(ReplaceIfVersionError::LocalWrite(_))),
                "{result:?}"
            );
            assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
            assert_eq!(promoted.config["mode"].as_str(), Some("direct"));
            assert_eq!(
                f.store
                    .last_confirmed_runtime_receipt()
                    .unwrap()
                    .config_digest,
                baseline.config_digest
            );
        }
        drop(clash);
    }
}

// -- V07: the Try succeeded and the save did not -----------------------------

/// The runtime accepted the candidate and persisting the source failed. The
/// Cancel restores the *actual* baseline receipt and verifies it, and nothing
/// new is committed.
#[tokio::test]
async fn a_failed_save_restores_the_verified_runtime_baseline() {
    use crate::service::profile_file::SelfProxyPortSource as _;

    let Fixture {
        client,
        endpoint,
        mut clash,
        store,
        clash_path,
        ports,
        _dir,
        ..
    } = fixture().await;

    // The host serves effective-config snapshots here, so the inspected apply
    // advances too and the Cancel has to put all of it back. The fake's first
    // answer is a scripted failure, hence two priming mutations.
    endpoint.set_effective_enabled(true);
    for mode in ["direct", "global"] {
        let (_, primed) = simple_mutate(
            &mut clash,
            &client,
            overrides(serde_json::json!({ "mode": mode })),
            CommandClass::Save,
        )
        .await;
        assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    }
    let baseline = store.last_confirmed_runtime_receipt().unwrap();
    let inspected = store
        .read()
        .applied
        .expect("the second priming apply was inspected");
    let committed = clash.snapshot_handle().load().version;

    // The config file cannot be written any more.
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();

    let (result, settlement) = mutate_settling(
        &mut clash,
        &client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settlement.await.expect("the Runtime settles its Try");
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
    // V08: the persistence cause, and that the runtime went back.
    let aborted = classify(result.unwrap_err(), &receipt);
    let CommitAborted::WriteConfig { runtime, source } = &aborted else {
        panic!("{aborted:?}");
    };
    assert!(
        matches!(runtime, RuntimeAftermath::RolledBack),
        "{runtime:?}"
    );
    assert!(
        source.to_string().contains("failed to write config"),
        "{source}"
    );
    assert_eq!(clash.snapshot_handle().load().version, committed);
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest
    );
    assert!(
        store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .binding
            .revision
            .generation
            > baseline.binding.revision.generation,
        "the restored baseline describes the instance the restore produced, not \
         the one it replaced"
    );
    // The inspected apply is what the user-facing runtime inspection reports,
    // so it follows the runtime back as well: leaving the withdrawn candidate
    // there would have the application name a configuration it just undid.
    assert_eq!(
        store
            .read()
            .applied
            .expect("the baseline apply is inspected again")
            .revision,
        inspected.revision,
        "the withdrawn candidate must not stay visible as the applied runtime"
    );
    assert!(
        store.read().pending.is_none(),
        "the withdrawn candidate is not awaiting an inspection either"
    );
    let submitted = endpoint.reconciled_bytes();
    assert_eq!(submitted[3], submitted[1]);
    assert!(!client.status().uncertain);
    // A restore the core confirmed is the one thing that says a listener
    // exists, so this path re-confirms the binding from the restored receipt
    // rather than leaving the application without an endpoint (D1).
    assert!(
        ports.confirmed().is_some(),
        "a verified restore confirms the baseline receipt's ports again"
    );
    assert!(ports.mixed_port().is_some());
}

/// The same failure, with a restore that cannot be verified. The enum name of
/// the restore request is never the proof, so an unconfirmed baseline isolates
/// the execution domain instead of reporting success (C4/D10, V04).
#[tokio::test]
async fn a_drifted_baseline_is_refused_without_overwriting_the_actual_runtime() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        mut application,
        clash_path,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));

    // Whatever is running now is not the document the baseline receipt
    // describes, and the host says so.
    endpoint.set_source_hash("drifted");
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();

    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(refused(&result));
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert!(!client.status().uncertain);
    assert!(client.mutation_journal().recovery.is_none());
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        1,
        "drift must not be overwritten"
    );

    let before = application.snapshot_handle().load().version;
    let (_, refused_result) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&refused_result));
    assert_eq!(application.snapshot_handle().load().version, before);
}

// -- §11.3: Closing is orthogonal ------------------------------------------

/// Closing rejects new mutations and stops producing new work, but it never
/// destroys a transaction that is still waiting for its decision.
#[tokio::test]
async fn closing_keeps_an_undecided_transaction_and_still_rejects_new_ones() {
    let Fixture {
        client,
        mut clash,
        mut application,
        store,
        shutdown,
        tasks,
        _dir,
        ..
    } = fixture().await;

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let in_flight = OperationId::generate();
    let undecided = {
        let client = client.clone();
        let (entered, release) = (entered.clone(), release.clone());
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                in_flight,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };
    entered.notified().await;

    shutdown.cancel();
    tasks.close();

    // The next Try is refused once it is taken, which is after the undecided
    // one has settled.
    let before = application.snapshot_handle().load().version;
    let rejected = {
        let client = client.clone();
        tokio::spawn(async move {
            let (_, result) = simple_mutate(
                &mut application,
                &client,
                app_with_core(ClashCore::ClashRs),
                CommandClass::ExplicitSwitch,
            )
            .await;
            (application, result)
        })
    };

    release.notify_one();
    let (clash, result) = undecided.await.unwrap();
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, in_flight).await.conclusion,
        MutationConclusion::Confirmed,
        "closing does not destroy an undecided transaction"
    );
    assert_eq!(
        store
            .read()
            .promoted
            .expect("the confirm still publishes")
            .config["mode"]
            .as_str(),
        Some("global")
    );
    let (application, rejected) = rejected.await.unwrap();
    assert!(refused(&rejected), "{rejected:?}");
    assert_eq!(application.snapshot_handle().load().version, before);
    tasks.wait().await;
    drop(clash);
}

// -- I1: an abort that owes a resource recovery ----------------------------

/// The source transaction's own local write fails, and so does the recovery
/// that would have put its resource back. The runtime goes back to the
/// committed configuration all the same, as for any abort, and the caller is
/// told both: the resource recovery failed, and the runtime was rolled back
/// (U7, Q-D).
#[tokio::test]
async fn an_abort_that_owes_a_resource_recovery_still_rolls_the_runtime_back() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        store,
        _dir,
        ..
    } = fixture().await;
    let (primed, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&client, primed).await;
    let baseline = store.last_confirmed_runtime_receipt().unwrap();

    let version = clash.snapshot_handle().load().version;
    let workflow = client.clone();
    let operation_id = OperationId::generate();
    let (settle, settlement) = tokio::sync::oneshot::channel();
    let result = clash
        .replace_if_version_with_participant(
            version,
            overrides(serde_json::json!({"mode": "global"})),
            move |decision| {
                ApplicationMutationParticipant::new(
                    operation_id,
                    MutationHints::default(),
                    CommandClass::Save,
                    RuntimeImpact::Reconcile,
                    decision,
                    workflow,
                    settle,
                )
            },
            || async { Err(anyhow::anyhow!("disk full").context("resource write failed")) },
            || async { Err(anyhow::anyhow!("file locked").context("resource recovery failed")) },
        )
        .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::ResourceRecovery { .. })),
        "{result:?}"
    );
    let receipt = settlement.await.expect("the Runtime settles its Try");
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
    assert!(!client.status().uncertain);
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest
    );
    assert_eq!(
        endpoint.reconciled_bytes().last(),
        Some(&baseline.config_text.as_bytes().to_vec()),
        "the Cancel put the baseline back"
    );
    let aborted = classify(result.unwrap_err(), &receipt);
    let CommitAborted::RecoverAfterWriteFailure {
        runtime,
        source:
            ReplaceIfVersionError::ResourceRecovery {
                cause,
                recovery_error,
            },
    } = &aborted
    else {
        panic!("{aborted:?}");
    };
    // Both failures reach the caller with their causes.
    assert!(
        matches!(runtime, RuntimeAftermath::RolledBack),
        "{runtime:?}"
    );
    assert!(
        format!("{cause:#}").contains("resource write failed: disk full"),
        "{cause:#}"
    );
    assert!(
        format!("{recovery_error:#}").contains("resource recovery failed: file locked"),
        "{recovery_error:#}"
    );
    drop(clash);
}

// -- R10: a Try needs a baseline its Cancel could restore -------------------

/// A core is running that this session never applied to, so no receipt says
/// what it is running. The gap is visible before anything is submitted, so the
/// mutation is refused there rather than submitted and then discovered to be
/// un-cancellable. A refusal, not an isolation: nothing was tried, the source
/// keeps its version, and the next mutation is admitted normally.
#[tokio::test]
async fn a_running_core_with_no_confirmed_apply_refuses_a_critical_mutation() {
    let Fixture {
        client,
        endpoint,
        mut application,
        store,
        _dir,
        ..
    } = fixture_without_a_confirmed_apply().await;

    let before = application.snapshot_handle().load().version;
    let (operation_id, result) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(application.snapshot_handle().load().version, before);
    assert!(
        endpoint.reconciled_bytes().is_empty(),
        "nothing is submitted against a baseline that could not be put back"
    );
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    let refused = refusals(&classify(result.unwrap_err(), &receipt));
    assert!(
        matches!(
            refused.as_slice(),
            [error] if matches!(
                error.as_ref(),
                RuntimeError::UnsettledBaseline {
                    gap: EvidenceGap::NoRestorableBaseline
                }
            )
        ),
        "{refused:?}"
    );
    assert!(
        !client.status().uncertain,
        "a refusal before the Try latches nothing"
    );

    // The evidence arrives, and the same command goes through.
    store.confirm_applied(Arc::new(adopted_baseline()));
    let (retried, retry) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(
        matches!(retry, Ok(ReplaceIfVersionResult::Replaced)),
        "{retry:?}"
    );
    assert_eq!(
        settled(&client, retried).await.conclusion,
        MutationConclusion::Confirmed
    );
}

/// The service host never names the core it applied. A session whose boot
/// reconcile was confirmed there still has a restorable baseline, so a
/// critical mutation is admitted rather than refused as if nothing had been
/// applied.
#[tokio::test]
async fn a_host_that_does_not_name_its_core_admits_a_critical_mutation() {
    let Fixture {
        client,
        endpoint,
        mut application,
        _dir,
        ..
    } = fixture().await;
    endpoint.set_status(Some(CoreStateDetail::Running { epoch: 1, pid: 7 }), None);

    let (operation_id, result) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, operation_id).await.conclusion,
        MutationConclusion::Confirmed
    );
}

// -- the check is never skipped --------------------------------------------

/// A candidate the core rejects is refused before it reaches the runtime, and a
/// check that could not run is not a passing one (v2 §2.4, V01/V02).
#[tokio::test]
async fn a_rejected_check_refuses_the_mutation_without_touching_the_runtime() {
    let mut f = fixture().await;
    f.endpoint.set_check_answer(TestCheckAnswer::Reject(
        nyanpasu_core_manager::CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "scripted: the core will not run this document",
            false,
        ),
    ));

    let before = f.clash.snapshot_handle().load().version;
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(f.clash.snapshot_handle().load().version, before);
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "a rejected candidate is never submitted"
    );
    let receipt = settlement.await.expect("a Try that ran is settled");
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert_eq!(receipt.conclusion, MutationConclusion::Withdrawn);
    // V07: the caller is told why.
    // A Try that ran and was refused, not an evidence gap.
    let refused = refusals(&classify(result.unwrap_err(), &receipt));
    assert!(
        matches!(
            refused.as_slice(),
            [error] if matches!(
                error.as_ref(),
                RuntimeError::CoreRejectedConfig { message, .. }
                    if message.contains("the core will not run this document")
            )
        ),
        "{refused:?}"
    );
    assert!(!f.client.status().uncertain);
}

/// A core the user stopped is an intent. The candidate is validated and saved,
/// nothing is started, and no retry loop is armed against the Stop (R7, V11).
#[tokio::test]
async fn a_stopped_core_saves_the_checked_target_without_starting_it() {
    let mut f = fixture().await;
    f.endpoint
        .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);

    let (operation_id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(f.application.snapshot().core, ClashCore::ClashRs);
    let receipt = settled(&f.client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::SavedInactive);
    assert_eq!(receipt.conclusion, MutationConclusion::Confirmed);
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "a save must not start a core the user stopped"
    );
    assert_eq!(
        f.endpoint.checked().len(),
        1,
        "the target is still validated before it is saved"
    );
}

// -- an absent fact is never read as "running" ------------------------------

/// The host publishes no runtime state and no stop was asked for. That is not
/// evidence of a stopped core and not evidence of a running one, so it must not
/// be read as either: it cannot license starting a core (V11) and it cannot
/// establish the baseline a Cancel would have to put back. The mutation is
/// refused and the execution domain released, because nothing was submitted.
#[tokio::test]
async fn a_host_that_publishes_nothing_refuses_a_critical_mutation() {
    let mut f = fixture().await;
    f.endpoint.set_status(None, None);
    let before = f.clash.snapshot_handle().load().version;

    let (operation_id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(f.clash.snapshot_handle().load().version, before);
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "an unsettled state is not a baseline to apply against"
    );
    let receipt = settled(&f.client, operation_id).await;
    assert_ne!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    let refused = refusals(&classify(result.unwrap_err(), &receipt));
    assert!(
        matches!(
            refused.as_slice(),
            [error] if matches!(
                error.as_ref(),
                RuntimeError::UnsettledBaseline {
                    gap: EvidenceGap::BaselineUnconfirmed
                }
            )
        ),
        "{refused:?}"
    );
    assert!(
        !f.client.status().uncertain,
        "a refusal before any submission does not isolate the execution domain"
    );

    // And the next mutation is admitted once the host answers again.
    f.endpoint.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );
    let (_, retried) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(retried, Ok(ReplaceIfVersionResult::Replaced)),
        "{retried:?}"
    );
}

/// A stop the user asked for is recorded evidence, so it settles the question
/// even when the host goes quiet afterwards: the target is validated and saved,
/// and nothing is started behind the Stop (R7, §2.3 `SavedInactive`).
#[tokio::test]
async fn a_recorded_user_stop_saves_without_starting_even_when_the_host_is_quiet() {
    let mut f = fixture().await;
    f.client.stop_core().await.unwrap();
    f.endpoint.set_status(None, None);
    let submitted = f.endpoint.reconciled_bytes().len();

    let (operation_id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    let receipt = settled(&f.client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::SavedInactive);
    assert_eq!(
        f.endpoint.reconciled_bytes().len(),
        submitted,
        "a save must not start a core the user stopped"
    );
    assert!(!f.client.status().uncertain);
}

/// A core mid-transition is an answer, but not a settled one. It is not a stop,
/// so an explicit switch issued during a restart must not be quietly saved,
/// answered `Ok` and never applied; and it is not a baseline either, so the
/// mutation is refused. Refused, not isolated: nothing was submitted, so the
/// caller may simply try again once the core settles.
#[tokio::test]
async fn a_transitional_core_state_refuses_a_mutation_without_isolating_the_domain() {
    for state in [
        CoreStateDetail::Restarting {
            epoch: 1,
            attempt: 1,
        },
        CoreStateDetail::Switching {
            from: Some(1),
            to: 2,
        },
        CoreStateDetail::Starting { epoch: 2 },
        CoreStateDetail::Stopping { epoch: 1 },
    ] {
        let mut f = fixture().await;
        f.endpoint
            .set_status(Some(state.clone()), Some(CoreKind::Mihomo));

        let before = f.application.snapshot_handle().load().version;
        let (operation_id, result) = simple_mutate(
            &mut f.application,
            &f.client,
            app_with_core(ClashCore::ClashRs),
            CommandClass::ExplicitSwitch,
        )
        .await;

        assert!(refused(&result), "{state:?}: {result:?}");
        assert_eq!(f.application.snapshot_handle().load().version, before);
        assert!(
            f.endpoint.reconciled_bytes().is_empty(),
            "{state:?}: a core mid-transition is not a baseline to apply against"
        );
        let receipt = settled(&f.client, operation_id).await;
        assert_ne!(
            receipt.outcome,
            MutationOutcomeKind::SavedInactive,
            "{state:?}: a transition is not a stop"
        );
        assert_ne!(receipt.outcome, MutationOutcomeKind::Applied);
        assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
        let refused = refusals(&classify(result.unwrap_err(), &receipt));
        assert!(
            matches!(
                refused.as_slice(),
                [error] if matches!(
                    error.as_ref(),
                    RuntimeError::UnsettledBaseline {
                        gap: EvidenceGap::CoreTransitioning
                    }
                )
            ),
            "{state:?}: {refused:?}"
        );
        assert!(
            !f.client.status().uncertain,
            "{state:?}: nothing was submitted, so nothing is uncertain"
        );

        // The execution domain was released normally: once the core settles,
        // back on the verified baseline, the very same command goes through.
        f.endpoint.set_status(
            Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
            Some(CoreKind::Mihomo),
        );
        let (_, retried) = simple_mutate(
            &mut f.application,
            &f.client,
            app_with_core(ClashCore::ClashRs),
            CommandClass::ExplicitSwitch,
        )
        .await;
        assert!(
            matches!(retried, Ok(ReplaceIfVersionResult::Replaced)),
            "{state:?}: {retried:?}"
        );
    }
}

// -- R8: the check is advisory, and an absent one is not a verdict ----------

/// A host with no check capability does not refuse the mutation. The check
/// never enters the mutating queue and is never a precondition for a change, so
/// the Try is the authority and the receipt records that none ran.
#[tokio::test]
async fn an_absent_check_capability_is_skipped_rather_than_refusing_the_mutation() {
    let mut f = fixture().await;
    f.endpoint.set_check_answer(TestCheckAnswer::Unsupported);

    let (operation_id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    let receipt = settled(&f.client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert!(
        matches!(receipt.check, CheckRecord::Skipped(_)),
        "{:?}",
        receipt.check
    );
    assert_eq!(
        f.endpoint.reconciled_bytes().len(),
        1,
        "the apply is the only authority left, and it ran"
    );
}

/// A host that has the check and could not serve it is the other class. The
/// failure matrix defaults to a refusal there, and `MustApply` has no deferral
/// to fall back on.
#[tokio::test]
async fn a_check_the_host_could_not_serve_refuses_a_must_apply_mutation() {
    let mut f = fixture().await;
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));

    let before = f.application.snapshot_handle().load().version;
    let (operation_id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(f.application.snapshot_handle().load().version, before);
    assert!(
        f.endpoint.reconciled_bytes().is_empty(),
        "an unserviceable check leaves the runtime alone"
    );
    let receipt = settled(&f.client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    // A check was owed and it ran; it just produced no verdict. Recording that
    // as "none was owed" would have the receipt claim the Try was the only
    // authority available, which is the opposite of what happened.
    assert!(
        matches!(receipt.check, CheckRecord::Unserviceable(_)),
        "{:?}",
        receipt.check
    );
}

// -- §4.4 Degraded: committing a target the core is not running -------------

/// The `Ack::Degraded` row, end to end. A typed-transient failure with a
/// confirmed safe baseline under `AllowDeferredWhenSafe` commits the new
/// desired value, keeps the old applied one, and registers the gap.
#[tokio::test]
async fn a_transient_failure_under_a_safe_baseline_commits_the_target_as_deferred() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        store,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let baseline = store.last_confirmed_runtime_receipt().unwrap();
    let submitted = endpoint.reconciled_bytes().len();

    endpoint.set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;

    // Only `Ack::Degraded` lets a transaction commit with a Deferred outcome;
    // a Rejected or Failed one would have aborted here.
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Deferred);
    assert_eq!(receipt.conclusion, MutationConclusion::Confirmed);
    assert_eq!(
        serde_json::to_value(clash.snapshot().overrides.clone()).unwrap()["mode"],
        "direct",
        "desired advances"
    );
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest,
        "applied does not"
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        submitted,
        "a deferred target is never submitted"
    );
    let deferred = client
        .mutation_journal()
        .deferred
        .expect("the gap between desired and applied is registered");
    assert_eq!(deferred.operation_id, operation_id);
    assert_eq!(deferred.attempts_remaining, DEFERRED_RETRY_BUDGET);
}

/// Explicit saves and unavailable dependency checks do not consume the
/// automatic apply budget reserved for the future scheduler.
#[tokio::test]
async fn a_repeated_manual_deferral_preserves_automatic_budget() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    endpoint.set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));

    // One desired value, offered again and again. It is built once because
    // `ClashConfig::default()` mints a fresh controller secret, and two values
    // that differ there are two different targets.
    //
    // Every repeat after the first has an empty diff, so it is the request's
    // own evidence that it named the overrides again — and nothing else — that
    // makes it a resubmission rather than an unrelated save (R15, §9.2).
    let target = overrides(serde_json::json!({"mode": "direct"}));
    let mut seen = Vec::new();
    for _ in 0..=DEFERRED_RETRY_BUDGET {
        let (operation_id, result) = mutate_with_hints(
            &mut clash,
            &client,
            target.clone(),
            CommandClass::Save,
            names_overrides(),
        )
        .await;
        assert!(
            matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
            "{result:?}"
        );
        assert_eq!(
            settled(&client, operation_id).await.outcome,
            MutationOutcomeKind::Deferred
        );
        let deferred = client.mutation_journal().deferred.unwrap();
        seen.push(deferred.attempts_remaining);
    }
    assert_eq!(
        seen,
        vec![DEFERRED_RETRY_BUDGET; usize::from(DEFERRED_RETRY_BUDGET) + 1],
        "manual retries must not spend the automatic budget"
    );

    // Another explicit save still reaches the source transaction.
    let before = clash.snapshot_handle().load().version;
    let (operation_id, result) = mutate_with_hints(
        &mut clash,
        &client,
        target,
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_ne!(clash.snapshot_handle().load().version, before);
    assert_eq!(
        settled(&client, operation_id).await.outcome,
        MutationOutcomeKind::Deferred
    );
}

/// The ACK each outcome owes the state transaction (v2 §4.4). The Degraded row
/// is the only one that lets a value the core is not running be committed.
#[test]
fn the_ack_of_an_outcome_follows_the_failure_matrix() {
    use super::super::{
        mutation::{ApplyFailure, RefusalCause, RetryableCause, RuntimePrepareOutcome},
        policy::TryCauseKind,
    };

    assert!(matches!(
        RuntimePrepareOutcome::SavedInactive {
            identity: String::new()
        }
        .ack(),
        Ack::Ok
    ));
    assert!(matches!(
        RuntimePrepareOutcome::Deferred {
            digest: "digest".into(),
            cause: RetryableCause {
                stage: super::super::mutation::MutationStage::TryingCritical,
                message: "briefly unreachable".into(),
            },
            error: Arc::new(RuntimeError::ShuttingDown),
        }
        .ack(),
        Ack::Degraded(error) if refusal_of("test", &error).to_string()
            == RuntimeError::ShuttingDown.to_string()
    ));
    assert!(matches!(
        RuntimePrepareOutcome::Rejected {
            cause: ApplyFailure {
                stage: super::super::mutation::MutationStage::TryingCritical,
                cause: RefusalCause::Try(TryCauseKind::Deterministic),
                error: Arc::new(RuntimeError::Isolated),
            },
            restored: super::super::mutation::KnownRuntimeState::Stopped,
        }
        .ack(),
        Ack::Rejected(error) if matches!(
            refusal_of("test", &error).as_ref(),
            RuntimeError::Isolated
        )
    ));
}

// -- R14: a host switch is applied, not assumed -----------------------------

/// Every critical mutation reconciles on whichever host owns the runtime, so a
/// mutation whose whole point is to move execution to the *other* host would
/// otherwise commit `Applied` without the move ever happening. The transition
/// belongs inside the Try, and the Cancel undoes it through the same path.
#[tokio::test]
async fn a_host_switch_moves_the_runtime_inside_the_try_and_back_on_cancel() {
    let service_host = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    service_host.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
        Some(CoreKind::Mihomo),
    );
    let Fixture {
        client,
        core,
        mut clash,
        mut application,
        app_path,
        _dir,
        ..
    } = fixture_with_hosts(true, Some(service_host.clone())).await;

    // A real baseline apply on the local host, so the Cancel has a receipt that
    // names the host it has to be put back on.
    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(core.status().host, ExecutionHost::Local);
    let submitted_to_service = service_host.submissions();

    // The mutation that asks for the service host. Its own write fails, so the
    // transaction aborts after the Try has already moved execution.
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let mutation = {
        let client = client.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        tokio::spawn(async move {
            let version = application.snapshot_handle().load().version;
            let (settle, _settlement) = tokio::sync::oneshot::channel();
            let result = application
                .replace_if_version_with_participant(
                    version,
                    NyanpasuAppConfig {
                        enable_service_mode: true,
                        ..NyanpasuAppConfig::default()
                    },
                    move |decision| {
                        ApplicationMutationParticipant::<NyanpasuAppConfig>::new(
                            operation_id,
                            MutationHints::default(),
                            CommandClass::ExplicitSwitch,
                            RuntimeImpact::HostSwitch,
                            decision,
                            client,
                            settle,
                        )
                    },
                    parked_local_write(entered, release),
                    || async { Ok(()) },
                )
                .await;
            (application, result)
        })
    };

    // Parked between the Try's verdict and the compare-and-swap: the verdict
    // the transaction is about to act on has been given, so whatever it claimed
    // about the runtime is true by now or never will be.
    entered.notified().await;
    assert_eq!(
        core.status().host,
        ExecutionHost::Service,
        "the Try answers for a host switch only once execution has actually moved"
    );
    assert!(
        service_host.submissions() > submitted_to_service,
        "and the candidate is applied on the host the user asked for"
    );

    // Nothing has written this domain's config yet, so there may be no file to
    // take away — only a directory to put in its place.
    let _ = std::fs::remove_file(&app_path);
    std::fs::create_dir_all(&app_path).unwrap();
    release.notify_one();
    let (application, result) = mutation.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::Cancelled,
        "{:?}",
        receipt.detail
    );
    assert_eq!(
        core.status().host,
        ExecutionHost::Local,
        "a cancelled host switch leaves execution where the mutation found it"
    );
    assert!(!client.status().uncertain);
    assert!(!application.snapshot().enable_service_mode);
}

/// A completed handoff is an effect of its own. Once it has run, the core the
/// mutation found running has been stopped and ownership sits with the other
/// host — whether or not the candidate that asked for the move was ever
/// applied there. A `Rejected` on top of that would claim the runtime was left
/// alone while the user's traffic runs nowhere, so a known failure compensates
/// through the same path a Cancel uses and reports the refusal only once the
/// baseline is verified back.
#[tokio::test]
async fn a_failed_apply_after_a_handoff_puts_the_original_runtime_back() {
    let service_host = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    service_host.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
        Some(CoreKind::Mihomo),
    );
    // The service host accepts the check and then refuses to start the
    // candidate, keeping the revision it already had. That is a known, terminal
    // failure: the candidate is not running and the host says so.
    service_host.set_rolls_back(true);
    let Fixture {
        client,
        endpoint,
        core,
        mut clash,
        mut application,
        _dir,
        ..
    } = fixture_with_hosts(true, Some(service_host.clone())).await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(core.status().host, ExecutionHost::Local);
    let restored_from = endpoint.reconciled_bytes().len();
    let before = application.snapshot_handle().load().version;

    let (operation_id, result) = simple_mutate(
        &mut application,
        &client,
        NyanpasuAppConfig {
            enable_service_mode: true,
            ..NyanpasuAppConfig::default()
        },
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    assert_eq!(application.snapshot_handle().load().version, before);
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::Withdrawn,
        "{:?}",
        receipt.detail
    );
    assert_eq!(
        core.status().host,
        ExecutionHost::Local,
        "a refused host switch leaves execution where the mutation found it"
    );
    assert!(
        endpoint.reconciled_bytes().len() > restored_from,
        "and the baseline is put back on that host rather than merely claimed"
    );
    assert!(!client.status().uncertain);
}

/// The same handoff, and an apply whose outcome nobody observed. Putting a
/// baseline back on top of a candidate that may be running is exactly what the
/// isolated state forbids, so the recovery context is kept and nothing is
/// compensated.
#[tokio::test]
async fn an_unobserved_apply_after_a_handoff_keeps_its_recovery_context() {
    let service_host = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    service_host.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
        Some(CoreKind::Mihomo),
    );
    // The host admits the operation and then loses its result.
    service_host.set_result_missing(true);
    let Fixture {
        client,
        endpoint,
        core,
        mut clash,
        mut application,
        _dir,
        ..
    } = fixture_with_hosts(true, Some(service_host.clone())).await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let untouched = endpoint.reconciled_bytes().len();

    let (operation_id, result) = simple_mutate(
        &mut application,
        &client,
        NyanpasuAppConfig {
            enable_service_mode: true,
            ..NyanpasuAppConfig::default()
        },
        CommandClass::ExplicitSwitch,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::RecoveryRequired);
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::RecoveryRequired,
        "{:?}",
        receipt.detail
    );
    assert!(client.status().uncertain);
    assert_eq!(
        core.status().host,
        ExecutionHost::Service,
        "the handoff is not undone on top of a candidate that may be running"
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        untouched,
        "nothing is put back while the candidate may be running"
    );
    assert_eq!(
        client
            .mutation_journal()
            .recovery
            .expect("an isolated domain names why")
            .operation_id,
        operation_id
    );
    assert!(!application.snapshot().enable_service_mode);
}

// -- C4: a restore is proven by a fresh observation, never by a cached one ---

/// The Try applied, the save failed, the restore was submitted, and then the
/// host stopped answering. The published projection still describes the
/// baseline — it was taken before the Try — so reading it here would report a
/// clean cancel for a runtime nobody has looked at since.
#[tokio::test]
async fn a_restore_that_cannot_be_observed_is_not_a_clean_cancel() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        clash_path,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let mutation = {
        let client = client.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        let target = overrides(serde_json::json!({"mode": "direct"}));
        tokio::spawn(async move {
            let version = clash.snapshot_handle().load().version;
            let result = mutate(
                &mut clash,
                &client,
                operation_id,
                target,
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (version, result)
        })
    };

    // The Try has applied. From here the source write fails and every status
    // read fails with it.
    entered.notified().await;
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    endpoint.set_status_fails(true);
    release.notify_one();
    let (_, result) = mutation.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::RecoveryRequired,
        "an unobservable runtime cannot prove the baseline is back: {:?}",
        receipt.detail
    );
    assert!(client.status().uncertain);
    assert_eq!(
        client
            .mutation_journal()
            .recovery
            .expect("an isolated domain names why")
            .operation_id,
        operation_id
    );
}

/// The Try applied a candidate that binds its own ports, the save failed, and
/// the restore's own result was lost. Nothing knows what is listening now: the
/// baseline may be back on its ports, the candidate may still hold its own, or
/// neither may be running. The confirmed binding is what the rest of the
/// application reads its endpoint from, so it ends here rather than keeping the
/// withdrawn candidate's ports on offer through the isolated state (v2 §6.2).
#[tokio::test]
async fn an_unverified_restore_takes_the_confirmed_ports_away() {
    use crate::service::profile_file::SelfProxyPortSource as _;

    let Fixture {
        client,
        endpoint,
        mut clash,
        clash_path,
        ports,
        _dir,
        ..
    } = fixture().await;

    let (primed_id, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&client, primed_id).await;
    let baseline = ports
        .confirmed()
        .expect("the priming apply confirmed its own ports");

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let mutation = {
        let client = client.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                operation_id,
                on_mixed_port(7897),
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };

    // The Try has applied, but its binding is not accepted until the source commits.
    entered.notified().await;
    assert!(ports.confirmed().is_none());
    assert_ne!(baseline.mixed_port, 7897);

    // From here the source write fails and the restore's result disappears.
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    endpoint.set_result_missing(true);
    release.notify_one();
    let (_clash, result) = mutation.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::RecoveryRequired,
        "an unobserved restore cannot prove the baseline is back: {:?}",
        receipt.detail
    );
    assert!(client.status().uncertain);
    let error = client.retry_runtime().await.unwrap_err();
    assert!(
        error.to_string().contains("core operation"),
        "an unknown restore retains its lower operation identity: {}",
        error
    );
    assert!(
        ports.confirmed().is_none(),
        "the withdrawn candidate's binding must not stay confirmed"
    );
    assert!(
        ports.mixed_port().is_none(),
        "and the rest of the application must be told there is no endpoint"
    );
}

/// The same rule where the two documents ask for exactly the same ports, which
/// is what an ordinary configuration or core change looks like.
///
/// Matching port numbers are not evidence that anything is listening on them.
/// The restoration can have stopped the candidate and then failed to start or
/// recover the baseline, which leaves the numbers identical and the endpoint
/// gone; nothing short of an apply the core confirmed says otherwise (D1).
#[tokio::test]
async fn an_unverified_restore_takes_unchanged_ports_away_too() {
    use crate::service::profile_file::SelfProxyPortSource as _;

    let Fixture {
        client,
        endpoint,
        mut clash,
        clash_path,
        ports,
        store,
        _dir,
        ..
    } = fixture().await;

    let (primed_id, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&client, primed_id).await;
    let baseline = ports
        .confirmed()
        .expect("the priming apply confirmed its own ports");

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let mutation = {
        let client = client.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                operation_id,
                // Only the mode moves: every port strategy is the baseline's.
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };

    entered.notified().await;
    assert!(ports.confirmed().is_none());
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .ports
            .bindings(),
        &baseline,
        "the candidate asked for exactly the ports the baseline holds"
    );

    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    endpoint.set_result_missing(true);
    release.notify_one();
    let (_clash, result) = mutation.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::RecoveryRequired,
        "{:?}",
        receipt.detail
    );
    assert!(
        ports.confirmed().is_none(),
        "asking for the same ports is not evidence that anything holds them"
    );
    assert!(
        ports.mixed_port().is_none(),
        "and the rest of the application must be told there is no endpoint"
    );
}

/// The same rule on the forward path, which is where the exception came from.
///
/// An unobserved apply replaces the instance holding the ports whether or not
/// the candidate asked for different numbers, and it can have stopped the old
/// one without starting the new. So the binding ends there too (D1).
#[tokio::test]
async fn an_unobserved_apply_takes_unchanged_ports_away() {
    use crate::service::profile_file::SelfProxyPortSource as _;

    let Fixture {
        client,
        endpoint,
        mut clash,
        ports,
        _dir,
        ..
    } = fixture().await;

    let (primed_id, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&client, primed_id).await;
    assert!(ports.confirmed().is_some());

    // The host admits the reconcile and then loses its result. Only the mode
    // moves, so the candidate asks for exactly the confirmed ports.
    endpoint.set_result_missing(true);
    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(refused(&result), "{result:?}");
    assert_eq!(
        settled(&client, operation_id).await.outcome,
        MutationOutcomeKind::RecoveryRequired
    );
    assert!(
        ports.confirmed().is_none(),
        "an unobserved apply of the same port numbers still replaces whatever \
         was holding them"
    );
    assert!(ports.mixed_port().is_none());
}

// -- v2 §5.4: a restore re-inspects the build it actually put back ----------

/// The store holds the inspected build of an older apply and the *pending*
/// build of the one the baseline receipt describes. A Cancel restores that
/// receipt, so the graph it re-binds has to be the receipt's build: re-binding
/// the older inspected one would have the application describe the running
/// runtime with a document it is not running.
#[tokio::test]
async fn a_restore_re_inspects_the_build_its_receipt_names() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        store,
        clash_path,
        _dir,
        ..
    } = fixture().await;

    // Two applies with the host serving effective-config snapshots — the fake's
    // first answer is a scripted failure — so the second one lands in `applied`.
    endpoint.set_effective_enabled(true);
    for mode in ["direct", "global"] {
        let (_, primed) = simple_mutate(
            &mut clash,
            &client,
            overrides(serde_json::json!({ "mode": mode })),
            CommandClass::Save,
        )
        .await;
        assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    }
    let older = store
        .read()
        .applied
        .expect("the second priming apply was inspected");

    // And one more with no snapshot to be had, so the baseline receipt's own
    // build is still waiting for its inspection.
    endpoint.set_effective_enabled(false);
    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let baseline = store
        .read()
        .pending
        .expect("the last apply is still awaiting its inspection");
    assert_ne!(baseline.revision, older.revision);
    assert_eq!(
        store.read().applied.map(|applied| applied.revision),
        None,
        "the current runtime has not been inspected; older inspection must not stand in for it"
    );

    // The Try applies and the save fails, so the Cancel restores that receipt.
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "script"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, operation_id).await.conclusion,
        MutationConclusion::Cancelled
    );

    assert_eq!(
        store
            .read()
            .pending
            .expect("the restored runtime is awaiting a new inspection")
            .revision,
        baseline.revision,
        "the restore re-inspects the build its receipt names, not the last one \
         that happened to be inspected"
    );
    assert_eq!(
        store
            .read()
            .pending
            .expect("a restored build with no effective document still owes one")
            .revision,
        baseline.revision,
    );
}

/// The same rule where nothing was ever inspected. The baseline receipt's build
/// sits in `pending` and `applied` is empty, so reading `applied` alone would
/// find no graph at all and drop the inspection the restored instance owes.
#[tokio::test]
async fn a_restore_with_no_prior_inspection_still_owes_one() {
    let Fixture {
        client,
        mut clash,
        store,
        clash_path,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let baseline = store
        .read()
        .pending
        .expect("the only apply is awaiting its inspection");
    assert!(
        store.read().applied.is_none(),
        "nothing has ever been inspected in this session"
    );

    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, operation_id).await.conclusion,
        MutationConclusion::Cancelled
    );

    assert_eq!(
        store
            .read()
            .pending
            .expect("the restored instance still owes its inspection")
            .revision,
        baseline.revision,
        "an obligation is rescheduled against the restored build, never dropped"
    );
}

// -- R16: a target's identity is not a property of the request --------------

/// A content update says "the bytes behind this path moved" through a
/// request-local hint; the save that follows it says nothing at all. Both ask
/// the core for the same runtime, so both are the same target: identity belongs
/// to the desired runtime, and a key that travelled with the hints would lose
/// the outstanding target the moment they were gone and open a fresh
/// convergence budget for it.
#[tokio::test]
async fn a_content_deferral_keeps_its_identity_once_the_hints_are_gone() {
    let Fixture {
        client,
        endpoint,
        mut profiles,
        profiles_dir,
        _dir,
        ..
    } = fixture().await;
    std::fs::create_dir_all(&profiles_dir).unwrap();
    std::fs::write(
        profiles_dir.join("sel.yaml"),
        "proxies: []\nproxy-groups: []\nrules: []\n",
    )
    .unwrap();
    endpoint.set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));

    // The document that selects the profile, committed unapplied. Its budget is
    // what everything below is measured against.
    let mut selected = Profiles::default();
    selected.append_item(crate::enhance::golden_support::file_config(
        "sel",
        "sel.yaml",
        &[],
    ));
    selected.current = Some(ProfileId("sel".into()));
    let (first, result) =
        simple_mutate(&mut profiles, &client, selected.clone(), CommandClass::Save).await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, first).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .expect("the gap is registered")
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET
    );

    // A subscription refresh: the document is byte-identical and the bytes
    // behind the selected profile's path are not.
    let (second, result) = mutate_with_hints(
        &mut profiles,
        &client,
        selected.clone(),
        CommandClass::Save,
        MutationHints {
            touched: vec![TouchedContent {
                path: ManagedProfilePath::new("sel.yaml").expect("managed path"),
                content_digest: Some(ContentDigest::new("sha256:refreshed")),
                resource: None,
            }],
            ..MutationHints::default()
        },
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, second).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .unwrap()
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET,
        "a content update asks for the outstanding target without spending automatic budget"
    );

    // The same committed document saved again, described by a different hint:
    // the user picked the profile that is already selected. It carries no
    // touched content at all, and it still asks for the same runtime.
    let (third, result) = mutate_with_hints(
        &mut profiles,
        &client,
        selected,
        CommandClass::Save,
        MutationHints {
            activation: ActivationIntent::Activate(ProfileId("sel".into())),
            ..MutationHints::default()
        },
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, third).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .unwrap()
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET,
        "identity is what the core is asked to run, never what the request \
         said it changed"
    );
}

// -- R15/§9.2: an unrelated save is not a resubmission ----------------------

/// The budget belongs to the runtime target, and only a request that asks for
/// that target again may spend it.
///
/// A save that moved a field no build stage reads carries the same runtime
/// target as the one that is still outstanding, but it did not ask for it: it
/// never reaches the runtime, so it preserves the target and its budget,
/// drives nothing, and commits as the plain save it is. Saving that document back unchanged does not ask for it
/// either — an empty diff proves nothing about what the request wanted. What
/// does ask for it again is a request that named a runtime field, and that one
/// is re-evaluated, empty diff and all, without spending automatic budget.
#[tokio::test]
async fn an_unrelated_save_keeps_the_outstanding_targets_identity_and_budget() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    endpoint.set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));

    let target = overrides(serde_json::json!({"mode": "direct"}));
    let (first, result) =
        simple_mutate(&mut clash, &client, target.clone(), CommandClass::Save).await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, first).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .expect("the gap is registered")
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET
    );
    let checked = endpoint.checked().len();
    let submitted = endpoint.reconciled_bytes().len();

    // A dashboard list no build stage reads. The document is different; what
    // the core is being asked to run is not, and this request did not ask for
    // it.
    let mut unrelated = target.clone();
    unrelated
        .web_ui_list
        .push("http://127.0.0.1:9090/ui".into());
    assert_eq!(
        runtime_impact(
            &target,
            &unrelated,
            &MutationHints::default(),
            CommandClass::Save
        ),
        None,
        "a save that moved no build input does not reach the runtime"
    );
    let (second, result) =
        simple_mutate(&mut clash, &client, unrelated.clone(), CommandClass::Save).await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert!(
        !client
            .mutation_journal()
            .completed
            .iter()
            .any(|receipt| receipt.operation_id == second),
        "so it runs no operation"
    );
    assert_eq!(
        endpoint.checked().len(),
        checked,
        "and it asks the core about nothing"
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        submitted,
        "and it drives no runtime"
    );
    let deferred = client.mutation_journal().deferred.unwrap();
    assert_eq!(
        deferred.operation_id, first,
        "the outstanding target survives an unrelated save"
    );
    assert_eq!(
        deferred.attempts_remaining, DEFERRED_RETRY_BUDGET,
        "an unrelated field neither spends nor refills the budget of the \
         target it did not ask for"
    );

    // The same unrelated document saved back unchanged. It produces a
    // byte-identical candidate, and it is still not a resubmission: nothing in
    // it asked the runtime for anything, so reading it out of document
    // equality would spend an outstanding budget on a no-op save (C1/R15).
    assert_eq!(
        runtime_impact(
            &unrelated,
            &unrelated,
            &MutationHints::default(),
            CommandClass::Save
        ),
        None,
        "an empty diff is not evidence that the runtime was asked for anything"
    );
    let (no_op, result) =
        simple_mutate(&mut clash, &client, unrelated.clone(), CommandClass::Save).await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert!(
        !client
            .mutation_journal()
            .completed
            .iter()
            .any(|receipt| receipt.operation_id == no_op)
    );
    assert_eq!(endpoint.checked().len(), checked);
    assert_eq!(endpoint.reconciled_bytes().len(), submitted);
    let deferred = client.mutation_journal().deferred.unwrap();
    assert_eq!(deferred.operation_id, first);
    assert_eq!(deferred.attempts_remaining, DEFERRED_RETRY_BUDGET);

    // The same document again, this time from a request that named the
    // overrides. Its diff is empty too, and §9.2 is explicit that this owes the
    // unconverged target one re-evaluation rather than a skip.
    let (third, result) = mutate_with_hints(
        &mut clash,
        &client,
        unrelated,
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, third).await.outcome,
        MutationOutcomeKind::Deferred
    );
    let deferred = client.mutation_journal().deferred.unwrap();
    assert_eq!(deferred.operation_id, third);
    assert_eq!(
        deferred.attempts_remaining, DEFERRED_RETRY_BUDGET,
        "a manual resubmission preserves the automatic budget"
    );

    // A genuinely different runtime target is a different gap, with a budget of
    // its own.
    let (fourth, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, fourth).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .unwrap()
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET
    );
}

/// The other half of the same rule: a request that resubmits the deferred
/// field unchanged *and* moves an unrelated one asks for the outstanding target.
///
/// The two documents differ, so no diff of them can find the resubmission, and
/// the runtime projection is identical, so no diff of that can find it either.
/// Only the request's own evidence — it named the overrides — says the user
/// asked for the mode that is committed and not running, and that is what owes
/// the outstanding target an attempt (C1/R15).
#[tokio::test]
async fn a_deferred_field_resubmitted_beside_an_unrelated_one_is_re_evaluated() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        _dir,
        ..
    } = fixture().await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    endpoint.set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));

    let target = overrides(serde_json::json!({"mode": "direct"}));
    let (first, result) = mutate_with_hints(
        &mut clash,
        &client,
        target.clone(),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, first).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(
        client
            .mutation_journal()
            .deferred
            .expect("the gap is registered")
            .attempts_remaining,
        DEFERRED_RETRY_BUDGET
    );

    // The same `mode` the user is still waiting for, resubmitted next to a
    // dashboard list no build stage reads.
    let mut resubmitted = target.clone();
    resubmitted
        .web_ui_list
        .push("http://127.0.0.1:9090/ui".into());
    let (second, result) = mutate_with_hints(
        &mut clash,
        &client,
        resubmitted,
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, second).await.outcome,
        MutationOutcomeKind::Deferred,
        "an explicit resubmission of the deferred field owes it an attempt"
    );
    let deferred = client.mutation_journal().deferred.unwrap();
    assert_eq!(
        deferred.operation_id, second,
        "and it is the same target, not a new one"
    );
    assert_eq!(
        deferred.attempts_remaining, DEFERRED_RETRY_BUDGET,
        "manual reevaluation preserves the target's automatic budget"
    );
}

#[tokio::test]
async fn a_preflight_failure_or_changed_revision_never_submits_or_isolates() {
    for revision_changed in [false, true] {
        let f = fixture().await;
        f.builder.park.store(true, Ordering::SeqCst);
        let client = f.client.clone();
        let mut clash = f.clash;
        let task = tokio::spawn(async move {
            simple_mutate(
                &mut clash,
                &client,
                overrides(serde_json::json!({"mode":"direct"})),
                CommandClass::Save,
            )
            .await
        });
        f.builder.entered.notified().await;
        if revision_changed {
            f.endpoint.set_effective_hash("external-revision");
        } else {
            f.endpoint.set_status_fails(true);
        }
        f.builder.release.notify_one();
        let (id, result) = task.await.unwrap();
        assert!(refused(&result), "{result:?}");
        assert_eq!(
            settled(&f.client, id).await.outcome,
            MutationOutcomeKind::Rejected
        );
        assert!(f.endpoint.reconciled_bytes().is_empty());
        assert!(!f.client.status().uncertain);
    }
}

/// V12: a pure UI save never reaches the Runtime. With a Runtime owner that
/// fails as soon as it is called, the save still commits, runs no operation
/// and reads nothing from the core; a save that does need the Runtime is
/// refused by the same owner.
#[tokio::test]
async fn a_gui_save_never_reaches_the_runtime() {
    use nyanpasu_config::application::ThemeMode;
    use struct_patch::Patch;
    let f = fixture().await;
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        f.client.clone(),
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
    );
    let application = crate::client::application::ApplicationClient::from_manager(
        mutations,
        f.application,
        crate::bundle::Channel::Stable,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    f.client.0.actor.kill_and_wait(None).await.unwrap();
    let status_reads = f.endpoint.status_reads();
    let before = application.snapshot();

    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.theme_mode = Some(ThemeMode::Dark);
    let saved = application.patch(patch).await.unwrap();
    assert!(saved.version > before.version);
    assert_eq!(saved.state.theme_mode, ThemeMode::Dark);
    let receipt = saved.receipt.expect("a commit has a receipt");
    assert_eq!(receipt.operation_id, None);
    assert_eq!(
        receipt.runtime,
        crate::client::runtime::RuntimeCommitStatus::Unchanged
    );
    assert!(saved.degradations.is_empty());
    assert_eq!(f.endpoint.status_reads(), status_reads);

    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
    assert!(application.patch(patch).await.is_err());
    assert_eq!(application.snapshot().version, saved.version);
}

/// V13, through the owner that decides it: a request reaches the runtime when
/// its diff or its intent does, and one that does keeps every promise it had.
/// Re-selecting the running core or the current host still owes a Try, a
/// patch that spans a UI field and a build input commits or fails as one, and
/// a core the user stopped is never started by either kind of save.
#[tokio::test]
async fn the_source_takes_the_runtime_only_into_requests_that_reach_it() {
    use crate::client::runtime::RuntimeCommitStatus;
    use nyanpasu_config::application::ThemeMode;
    use struct_patch::Patch;
    let f = fixture().await;
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        f.client.clone(),
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
    );
    let application = crate::client::application::ApplicationClient::from_manager(
        mutations,
        f.application,
        crate::bundle::Channel::Stable,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let current = application.snapshot().state;

    // Irrelevant: committed on its own, and the core is not asked anything.
    let checked = f.endpoint.checked().len();
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.theme_mode = Some(ThemeMode::Dark);
    let receipt = application.patch(patch).await.unwrap().receipt.unwrap();
    assert_eq!(receipt.operation_id, None);
    assert_eq!(receipt.runtime, RuntimeCommitStatus::Unchanged);
    assert_eq!(f.endpoint.checked().len(), checked);

    // Relevant, and the same value re-selected for the core and for the host.
    let mut relevant = NyanpasuAppConfig::new_empty_patch();
    relevant.enable_builtin_enhanced = Some(!current.enable_builtin_enhanced);
    let mut core = NyanpasuAppConfig::new_empty_patch();
    core.core = Some(current.core);
    let mut host = NyanpasuAppConfig::new_empty_patch();
    host.enable_service_mode = Some(current.enable_service_mode);
    for (case, patch) in [("relevant", relevant), ("core", core), ("host", host)] {
        let checked = f.endpoint.checked().len();
        let receipt = application.patch(patch).await.unwrap().receipt.unwrap();
        assert!(receipt.operation_id.is_some(), "{case}");
        assert_eq!(receipt.runtime, RuntimeCommitStatus::Applied, "{case}");
        assert_eq!(f.endpoint.checked().len(), checked + 1, "{case}");
    }

    // Mixed: the core refuses the build input, so the UI field is not
    // committed either.
    f.endpoint.set_check_answer(TestCheckAnswer::Reject(
        nyanpasu_core_manager::CoreError::new(
            CoreErrorKind::InvalidConfig,
            "invalid candidate",
            false,
        ),
    ));
    let before = application.snapshot();
    let mut mixed = NyanpasuAppConfig::new_empty_patch();
    mixed.theme_mode = Some(ThemeMode::Light);
    mixed.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
    assert!(application.patch(mixed).await.is_err());
    let after = application.snapshot();
    assert_eq!(after.version, before.version);
    assert_eq!(after.state.theme_mode, ThemeMode::Dark);
    f.endpoint.set_check_answer(TestCheckAnswer::Pass);

    // Stopped by the user: a relevant save is checked and saved inactive, an
    // irrelevant one is a plain save, and neither starts the core.
    f.client.stop_core().await.unwrap();
    let submitted = f.endpoint.reconciled_bytes().len();
    let mut relevant = NyanpasuAppConfig::new_empty_patch();
    relevant.enable_builtin_enhanced = Some(!after.state.enable_builtin_enhanced);
    let receipt = application.patch(relevant).await.unwrap().receipt.unwrap();
    assert!(receipt.operation_id.is_some());
    assert_eq!(receipt.runtime, RuntimeCommitStatus::SavedInactive);
    let mut irrelevant = NyanpasuAppConfig::new_empty_patch();
    irrelevant.theme_mode = Some(ThemeMode::System);
    let receipt = application
        .patch(irrelevant)
        .await
        .unwrap()
        .receipt
        .unwrap();
    assert_eq!(receipt.operation_id, None);
    assert_eq!(receipt.runtime, RuntimeCommitStatus::Unchanged);
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted);
}

#[tokio::test]
async fn cross_domain_deferrals_share_the_complete_latest_target() {
    let mut f = fixture().await;
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    let mut app = f.application.snapshot().as_ref().clone();
    app.enable_builtin_enhanced = !app.enable_builtin_enhanced;
    let (first, result) = simple_mutate(
        &mut f.application,
        &f.client,
        app.clone(),
        CommandClass::Save,
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        settled(&f.client, first).await.outcome,
        MutationOutcomeKind::Deferred
    );
    let (second, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        settled(&f.client, second).await.outcome,
        MutationOutcomeKind::Deferred
    );
    let deferred = f.client.mutation_journal().deferred.unwrap();
    let checks = f.endpoint.checked().len();
    let (third, result) = mutate_with_hints(
        &mut f.application,
        &f.client,
        app,
        CommandClass::Save,
        MutationHints {
            requested: RequestedRuntimeFields::runtime(),
            ..MutationHints::default()
        },
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        settled(&f.client, third).await.outcome,
        MutationOutcomeKind::Deferred
    );
    assert_eq!(f.endpoint.checked().len(), checks + 1);
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().identity,
        deferred.identity
    );
}

#[tokio::test]
async fn selecting_the_saved_host_again_moves_the_actual_host() {
    let service = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    let mut f = fixture_with_hosts(true, Some(service.clone())).await;
    let mut desired = f.application.snapshot().as_ref().clone();
    desired.enable_service_mode = true;
    // A previously saved desired value can differ from the actual local owner.
    f.application
        .replace_if_version(
            f.application.snapshot_handle().load().version,
            desired.clone(),
        )
        .await
        .unwrap();
    assert_eq!(f.core.status().host, ExecutionHost::Local);
    let (id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        desired,
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        settled(&f.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );
    assert_eq!(f.core.status().host, ExecutionHost::Service);
    assert_eq!(service.reconciled_bytes().len(), 1);
}
#[tokio::test]
async fn successful_confirm_keeps_the_promoted_inspection() {
    let mut f = fixture().await;
    f.endpoint.set_effective_enabled(true);
    let _ = crate::core::actor_v2::endpoint::ControlEndpoint::effective_config(f.endpoint.as_ref())
        .await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    let runtime = f.store.read();
    assert!(runtime.applied.as_ref().unwrap().effective.is_some());
    assert!(runtime.pending.is_none());
    assert!(
        runtime.promoted.as_ref().unwrap().effective.is_some(),
        "Confirm published the pre-inspection snapshot even though the core inspection already arrived"
    );
}

struct RefusedInstall;

#[async_trait::async_trait]
impl ServiceHostAdapter for RefusedInstall {
    async fn probe(
        &self,
    ) -> Result<
        nyanpasu_ipc::types::StatusInfo<'static>,
        crate::core::service::control::ServiceCommandError,
    > {
        crate::client::tests::IdleServiceAdapter.probe().await
    }
    async fn install(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        Err(crate::core::service::control::ServiceCommandError::mock(
            "the user cancelled the elevation prompt",
        ))
    }
    async fn uninstall(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        unreachable!()
    }
    async fn start_daemon(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        unreachable!()
    }
    async fn stop_daemon(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        unreachable!()
    }
    async fn update(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        unreachable!()
    }
    fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
        unreachable!()
    }
}

#[tokio::test]
async fn refused_service_install_does_not_isolate_the_local_runtime() {
    let mut f = fixture_with_daemon(true, Some(Arc::new(RefusedInstall))).await;
    let mut next = f.application.snapshot().as_ref().clone();
    next.enable_service_mode = true;
    let (id, result) = simple_mutate(
        &mut f.application,
        &f.client,
        next,
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&result), "{result:?}");
    let receipt = settled(&f.client, id).await;
    assert_eq!(f.core.status().host, ExecutionHost::Local);
    assert!(f.endpoint.reconciled_bytes().is_empty());
    assert_eq!(
        receipt.outcome,
        MutationOutcomeKind::Rejected,
        "service preparation was refused before any core handoff: {receipt:?}"
    );
    assert!(!f.client.status().uncertain);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
}

#[tokio::test]
async fn invalid_overlay_is_rejected_before_source_commit() {
    let mut f = fixture().await;
    std::fs::create_dir_all(&f.profiles_dir).unwrap();
    std::fs::write(f.profiles_dir.join("invalid.yaml"), "rules: [").unwrap();
    let item = crate::enhance::golden_support::overlay("invalid", "invalid.yaml");
    let mut next = f.profiles.snapshot().as_ref().clone();
    next.global_transforms.push(item.uid.clone());
    next.items.insert(item.uid.clone(), item);
    let (id, result) = simple_mutate(&mut f.profiles, &f.client, next, CommandClass::Save).await;
    let receipt = settled(&f.client, id).await;
    assert!(
        refused(&result),
        "invalid overlay committed after its parse error was converted into passthrough: {receipt:?}"
    );
    assert!(f.endpoint.reconciled_bytes().is_empty());
}

#[tokio::test]
async fn frozen_content_preserves_lenient_build_and_strict_candidate_policy() {
    use super::super::ports::RuntimeBuildPort;
    let f = fixture().await;
    let item = crate::enhance::golden_support::overlay("missing", "missing.yaml");
    let mut profiles = Profiles::default();
    profiles.global_transforms.push(item.uid.clone());
    profiles.items.insert(item.uid.clone(), item);
    let input = crate::enhance::RuntimeBuildInput {
        profiles: Arc::new(profiles.clone()),
        clash: ClashConfig::default(),
        app: NyanpasuAppConfig::default(),
        resolved_ports: nyanpasu_config::runtime::executor::ResolvedPortBindings {
            mixed_port: 7890,
            ..Default::default()
        },
    };
    let old_content = crate::enhance::FsProfileContentSource::new(f.profiles_dir.clone());
    let script_dirs = f.builder.delegate.scripts.clone();
    let built = tokio::task::spawn_blocking(move || {
        let scripts = crate::enhance::EnhanceScriptRunner::new(script_dirs).unwrap();
        crate::enhance::RuntimeBuilder::build(&input, &old_content, &scripts)
    })
    .await
    .unwrap();
    assert!(built.is_ok());
    let captured = f.builder.capture_content(&profiles).await;
    let mut revisions = runtime::RuntimeRevisionAllocator::new();
    let build = |revision, strict| {
        f.builder.build(
            revision,
            super::super::inputs::RuntimeInputs {
                profiles: Arc::new(profiles.clone()),
                clash: ClashConfig::default(),
                app: NyanpasuAppConfig::default(),
                content: captured.clone(),
            },
            nyanpasu_config::runtime::executor::ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
            strict,
        )
    };
    assert!(
        build(revisions.allocate(), false).await.is_ok(),
        "ordinary build keeps D7 passthrough"
    );
    assert!(
        build(revisions.allocate(), true).await.is_err(),
        "TCC candidate rejects missing transform"
    );
}

#[tokio::test]
async fn uncommitted_runtime_ports_are_not_available_to_peripheral_readers() {
    let Fixture {
        client,
        mut clash,
        store,
        ports,
        _dir,
        ..
    } = fixture().await;
    let source = clash.snapshot_handle();
    let version = source.load().version;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let id = OperationId::generate();
    let work = {
        let client = client.clone();
        let entered = entered.clone();
        let release = release.clone();
        tokio::spawn(async move {
            mutate(
                &mut clash,
                &client,
                id,
                on_mixed_port(7897),
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await
        })
    };
    entered.notified().await;
    assert_eq!(source.load().version, version);
    let actual = store
        .last_confirmed_runtime_receipt()
        .expect("actual apply evidence");
    assert_eq!(actual.ports.bindings().mixed_port, 7897);
    assert!(store.read().pending.is_some() || store.read().applied.is_some());
    let provisional = ports.confirmed();
    release.notify_one();
    assert!(matches!(
        work.await.unwrap(),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert_eq!(
        settled(&client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert!(
        provisional.is_none(),
        "peripheral reader received an uncommitted runtime binding: {provisional:?}"
    );
    assert_eq!(ports.confirmed().unwrap().mixed_port, 7897);
}

// T6 exercises the production domain actor, including its own participant
// construction, instead of supplying a participant directly from the test.
#[tokio::test]
async fn application_actor_rejection_keeps_source_version_and_bytes() {
    use struct_patch::Patch;
    let f = fixture().await;
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        f.client.clone(),
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
    );
    let snapshot = f.application.snapshot_handle();
    let before = snapshot.load();
    let bytes = std::fs::read(&f.app_path).ok();
    let application = crate::client::application::ApplicationClient::from_manager(
        mutations,
        f.application,
        crate::bundle::Channel::Stable,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    f.endpoint.set_check_answer(TestCheckAnswer::Reject(
        nyanpasu_core_manager::CoreError::new(
            CoreErrorKind::InvalidConfig,
            "invalid candidate",
            false,
        ),
    ));
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
    assert!(application.patch(patch).await.is_err());
    assert_eq!(snapshot.load().version, before.version);
    assert_eq!(std::fs::read(&f.app_path).ok(), bytes);
    assert_eq!(f.endpoint.submissions(), 0);
}

#[tokio::test]
async fn application_actor_prepare_does_not_block_committed_reads() {
    use struct_patch::Patch;
    let f = fixture().await;
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        f.client.clone(),
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
    );
    let application = crate::client::application::ApplicationClient::from_manager(
        mutations,
        f.application,
        crate::bundle::Channel::Stable,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let before = application.snapshot();
    f.builder.park.store(true, Ordering::SeqCst);
    let writer = application.clone();
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_builtin_enhanced = Some(!before.state.enable_builtin_enhanced);
    let save = tokio::spawn(async move { writer.patch(patch).await });
    f.builder.entered.notified().await;
    assert_eq!(application.snapshot().version, before.version);
    assert_eq!(
        application.snapshot().state.enable_builtin_enhanced,
        before.state.enable_builtin_enhanced
    );
    f.builder.release.notify_one();
    save.await.unwrap().unwrap();
    assert!(application.snapshot().version > before.version);
}

#[tokio::test]
async fn domain_actor_refuses_writes_before_composition_is_ready() {
    use struct_patch::Patch;
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(
        temp_path(&dir, "application.yaml"),
        NyanpasuAppConfig::default(),
    )
    .await;
    let application = crate::client::application::ApplicationClient::from_manager(
        crate::state::mutation::MutationCoordinator::pending(),
        manager,
        crate::bundle::Channel::Stable,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .unwrap();
    let before = application.snapshot().version;
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_silent_start = Some(true);
    assert!(
        application
            .patch(patch)
            .await
            .unwrap_err()
            .to_string()
            .contains("not ready")
    );
    assert_eq!(application.snapshot().version, before);
}

async fn deferred_fixture() -> Fixture {
    let mut f = fixture().await;
    f.endpoint.set_failure(Some("queue_full"));
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Replaced)),
        "{result:?}"
    );
    assert_eq!(
        settled(&f.client, id).await.outcome,
        MutationOutcomeKind::Deferred
    );
    f
}

#[tokio::test]
async fn native_store_failure_refuses_the_save_without_scheduling_retries() {
    let mut f = fixture().await;
    f.endpoint.set_failure(Some("native_store_unavailable"));
    let before = f.clash.snapshot_handle().load().version;

    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    let receipt = settled(&f.client, id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert_eq!(receipt.conclusion, MutationConclusion::Withdrawn);
    assert_eq!(f.clash.snapshot_handle().load().version, before);
    assert!(f.client.mutation_journal().deferred.is_none());
    let attempts = f.endpoint.submissions();
    f.client
        .call(super::super::Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    assert_eq!(f.endpoint.submissions(), attempts);
}

#[tokio::test]
async fn runtime_automatic_budget_exhausts_and_retry_now_never_writes_source() {
    use crate::client::convergence::ConvergenceHealth;
    let f = deferred_fixture().await;
    let source = f.clash.snapshot_handle().load().version;
    for remaining in [2, 1, 0] {
        f.client
            .call(super::super::Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
        assert_eq!(
            f.client
                .mutation_journal()
                .deferred
                .unwrap()
                .attempts_remaining,
            remaining
        );
    }
    let attempts = f.endpoint.submissions();
    f.client
        .call(super::super::Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    assert_eq!(f.endpoint.submissions(), attempts);
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().health,
        ConvergenceHealth::Blocked
    );
    f.endpoint.set_failure(None);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.mutation_journal().deferred.is_none());
    assert_eq!(f.clash.snapshot_handle().load().version, source);
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_text
            .contains("mode: direct"),
        true
    );
}

#[tokio::test]
async fn runtime_retry_waits_for_check_dependency_without_spending_apply_budget() {
    use crate::client::convergence::ConvergenceHealth;
    let f = deferred_fixture().await;
    f.endpoint.set_failure(None);
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    let before = f.endpoint.submissions();
    f.client
        .call(super::super::Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    let gap = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(gap.health, ConvergenceHealth::WaitingDependency);
    assert_eq!(gap.attempts_remaining, DEFERRED_RETRY_BUDGET);
    assert_eq!(gap.attempts, 0);
    assert_eq!(f.endpoint.submissions(), before);
}

/// S17 on a committed target: consecutive dependency results count up
/// without touching the budget or the attempt count, and the first
/// application result starts the count over.
#[tokio::test]
async fn dependency_retries_count_waits_until_an_application_result() {
    let f = deferred_fixture().await;
    f.endpoint.set_failure(None);
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    for waits in 1..=3 {
        f.client
            .call(super::super::Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
        let gap = f.client.mutation_journal().deferred.unwrap();
        assert_eq!(gap.waits, waits);
        assert_eq!(gap.attempts_remaining, DEFERRED_RETRY_BUDGET);
        assert_eq!(gap.attempts, 0);
    }

    // The check answers again and the apply fails transiently: an attempt
    // that reached the runtime leaves the backoff and pays for itself.
    f.endpoint.set_check_answer(TestCheckAnswer::Pass);
    f.endpoint.set_failure(Some("queue_full"));
    f.client
        .call(super::super::Command::RetryRuntime { explicit: false })
        .await
        .unwrap();
    let gap = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(gap.waits, 0);
    assert_eq!(gap.attempts_remaining, DEFERRED_RETRY_BUDGET - 1);
    assert_eq!(gap.attempts, 1);
}

/// T10 §1.7 #2 (D11): an automatic retry never moves the runtime to another
/// host, since that can mean installing or starting the daemon behind the
/// user's back. It waits for the owner instead, and spends nothing.
#[tokio::test]
async fn an_automatic_retry_never_moves_the_runtime_to_another_host() {
    use crate::client::convergence::ConvergenceHealth;
    let service = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    let mut f = fixture_with_hosts(true, Some(service.clone())).await;
    f.endpoint.set_failure(Some("queue_full"));
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&f.client, id).await.outcome,
        MutationOutcomeKind::Deferred
    );
    f.endpoint.set_failure(None);
    // The runtime now runs on the other host, on an apply this session
    // confirmed there: a baseline a Try would accept, on a host the committed
    // configuration does not ask for.
    f.client.change_host(ExecutionHost::Service).await.unwrap();
    service.set_source_hash(&nyanpasu_core_manager::payload_digest(b"mode: rule\n"));
    service.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );
    let mut receipt = adopted_baseline();
    receipt.host = ExecutionHost::Service;
    receipt.binding.host = ExecutionHost::Service;
    receipt.binding.generation = f.core.status().generation;
    f.store.confirm_applied(Arc::new(receipt));
    let before = f.client.mutation_journal().deferred.unwrap();

    f.client
        .call(super::super::Command::RetryRuntime { explicit: false })
        .await
        .unwrap();

    let target = f.client.mutation_journal().deferred.unwrap();
    assert_eq!(target.health, ConvergenceHealth::WaitingDependency);
    assert_eq!(
        (target.attempts_remaining, target.attempts),
        (before.attempts_remaining, before.attempts)
    );
    assert_eq!(f.core.status().host, ExecutionHost::Service);
    assert!(service.reconciled_bytes().is_empty());
}

#[tokio::test]
async fn stopped_runtime_is_not_started_by_retry_now() {
    use crate::client::convergence::ConvergenceHealth;
    let f = deferred_fixture().await;
    f.endpoint.set_failure(None);
    f.endpoint
        .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
    let before = f.endpoint.submissions();
    f.client.retry_runtime().await.unwrap();
    assert_eq!(f.endpoint.submissions(), before);
    assert_eq!(
        f.client.mutation_journal().deferred.unwrap().health,
        ConvergenceHealth::WaitingDependency
    );
}

#[tokio::test]
async fn unknown_retry_queries_original_before_restoring_aborted_source() {
    let mut f = fixture().await;
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&f.client, id).await;
    let source = f.clash.snapshot_handle().load().version;
    let baseline = f.store.last_confirmed_runtime_receipt().unwrap();
    f.endpoint.set_result_missing(true);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(refused(&result));
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::RecoveryRequired
    );
    let before = f.endpoint.submissions();
    assert!(f.client.retry_runtime().await.is_err());
    assert_eq!(f.endpoint.submissions(), before);
    f.endpoint.set_result_missing(false);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.mutation_journal().recovery.is_none());
    assert!(!f.client.status().uncertain);
    assert_eq!(f.clash.snapshot_handle().load().version, source);
    assert_eq!(
        f.store
            .last_confirmed_runtime_receipt()
            .unwrap()
            .config_digest,
        baseline.config_digest
    );
}

#[tokio::test]
async fn committed_product_retry_does_not_resubmit_or_rewrite_source() {
    let mut f = fixture().await;
    f.builder.fail_publish.store(true, Ordering::SeqCst);
    let (id, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode":"global"})),
        CommandClass::Save,
    )
    .await;
    assert!(result.is_ok());
    settled(&f.client, id).await;
    assert!(f.client.mutation_journal().maintenance.is_some());
    let submitted = f.endpoint.reconciled_bytes().len();
    let version = f.clash.snapshot_handle().load().version;
    f.builder.fail_publish.store(false, Ordering::SeqCst);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.mutation_journal().maintenance.is_none());
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted);
    assert_eq!(f.clash.snapshot_handle().load().version, version);
}

// -- the settlement goes back to the source ------------------------------------

/// V05: a transaction that fails its version check never calls its
/// participant, so the Runtime never hears of it. The settlement its source
/// waits for resolves as soon as the transaction returns, empty.
#[tokio::test]
async fn a_conflicting_version_never_waits_for_a_settlement() {
    let mut f = fixture().await;
    let stale = f.clash.snapshot_handle().load().version;
    let (_, primed) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let submitted = f.endpoint.reconciled_bytes().len();

    let client = f.client.clone();
    let operation_id = OperationId::generate();
    let (settle, settlement) = tokio::sync::oneshot::channel();
    let result = f
        .clash
        .replace_if_version_with_participant(
            stale,
            overrides(serde_json::json!({"mode": "direct"})),
            move |decision| {
                ApplicationMutationParticipant::new(
                    operation_id,
                    MutationHints::default(),
                    CommandClass::Save,
                    RuntimeImpact::Reconcile,
                    decision,
                    client,
                    settle,
                )
            },
            no_local_write,
            || async { Ok(()) },
        )
        .await;

    assert!(
        matches!(result, Ok(ReplaceIfVersionResult::Conflict { .. })),
        "{result:?}"
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(5), settlement)
            .await
            .expect("the settlement resolves once the transaction returns")
            .is_err(),
        "nothing was settled"
    );
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted, "no Try ran");
}

/// V06: a Try that never reached the workflow did not run. The participant
/// refuses it rather than failing it, the source keeps its version, the core
/// is untouched, and the error says nothing was committed.
#[tokio::test]
async fn a_try_the_workflow_never_received_is_refused_as_not_run() {
    let mut f = fixture().await;
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;

    let before = f.clash.snapshot_handle().load().version;
    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    let settlement = settlement.await.ok();
    assert!(settlement.is_none(), "the Runtime never took the Try");
    let Err(ReplaceIfVersionError::State(StateChangedError::PrepareAck(refusal))) = &result else {
        panic!("{result:?}");
    };
    assert!(
        matches!(
            refusal.report.subscriber_acks.as_slice(),
            [ack] if matches!(&ack.status, AckStatus::Rejected { reason }
                if matches!(refusal_of("test", reason).as_ref(), RuntimeError::OwnerUnavailable))
        ),
        "{:?}",
        refusal.report
    );
    assert_eq!(f.clash.snapshot_handle().load().version, before);
    assert!(f.endpoint.reconciled_bytes().is_empty());
}

/// V09: the save fails and the rollback cannot be verified either. The error
/// names both, and the execution domain is isolated.
#[tokio::test]
async fn a_failed_save_whose_rollback_fails_reports_both() {
    let mut f = scripted_fixture().await;
    let (primed, result) = simple_mutate(
        &mut f.clash,
        &f.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&f.client, primed).await;
    let scripted = f.scripted.clone().expect("a scripted fixture");
    // The Try applies; the Cancel's restore loses its answer.
    scripted.queue(super::WaitScript::Deliver);
    scripted.queue(super::WaitScript::Missing);
    std::fs::remove_file(&f.clash_path).unwrap();
    std::fs::create_dir_all(&f.clash_path).unwrap();

    let (result, settlement) = mutate_settling(
        &mut f.clash,
        &f.client,
        OperationId::generate(),
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
        plain(),
        no_local_write,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );
    let receipt = settlement.await.expect("the Runtime settles its Try");
    assert_eq!(receipt.conclusion, MutationConclusion::RecoveryRequired);
    let aborted = classify(result.unwrap_err(), &receipt);
    let CommitAborted::WriteConfig { runtime, source } = &aborted else {
        panic!("{aborted:?}");
    };
    assert!(
        matches!(runtime, RuntimeAftermath::RollbackFailed { .. }),
        "{runtime:?}"
    );
    assert!(
        source.to_string().contains("failed to write config"),
        "{source}"
    );
    assert!(f.client.status().uncertain);
}

/// V10: the journal is only a display. Every source hears its own result on
/// its own channel, even once the journal's history has moved past it.
#[tokio::test]
async fn every_settlement_arrives_whatever_the_journal_keeps() {
    let mut f = fixture().await;
    let mut attempts = Vec::new();
    for index in 0..=super::super::HISTORY_LEN {
        let mode = if index % 2 == 0 { "global" } else { "direct" };
        let operation_id = OperationId::generate();
        let (result, settlement) = mutate_settling(
            &mut f.clash,
            &f.client,
            operation_id,
            overrides(serde_json::json!({ "mode": mode })),
            CommandClass::Save,
            plain(),
            no_local_write,
        )
        .await;
        assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
        attempts.push((operation_id, settlement));
    }
    // Nothing is read until every attempt ran: the Runtime sends a receipt
    // before its handler returns, and the oneshot keeps it. It journals the
    // receipt before sending it, so once the last one has arrived the history
    // has moved past the first.
    let (last_id, last) = attempts.pop().expect("more than one attempt ran");
    let receipt = last.await.expect("every attempt is settled to its source");
    assert_eq!(receipt.operation_id, last_id);
    let first = attempts[0].0;
    assert!(
        f.client
            .mutation_journal()
            .completed
            .iter()
            .all(|receipt| receipt.operation_id != first),
        "the journal's history no longer holds the first attempt"
    );
    for (operation_id, settlement) in attempts {
        let receipt = settlement
            .await
            .expect("every attempt is settled to its source");
        assert_eq!(receipt.operation_id, operation_id);
    }
}

/// V11: the Runtime owner is gone after it accepted the Try. The source has
/// committed, and its caller is told that the runtime needs recovery.
#[tokio::test]
async fn a_runtime_owner_gone_after_the_try_leaves_the_commit_to_recover() {
    let f = fixture().await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let operation_id = OperationId::generate();
    let mutation = {
        let client = f.client.clone();
        let (entered, release) = (entered.clone(), release.clone());
        let mut clash = f.clash;
        tokio::spawn(async move {
            let settled = mutate_settling(
                &mut clash,
                &client,
                operation_id,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, settled)
        })
    };
    entered.notified().await;
    f.client.0.actor.kill();
    release.notify_one();

    let (clash, (result, settlement)) = mutation.await.unwrap();
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    let settlement = settlement.await.ok();
    assert!(settlement.is_none());
    let coordinator = crate::state::mutation::MutationCoordinator::pending();
    coordinator.connect(
        f.client.clone(),
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
    );
    let (commit, degradations) = coordinator.committed(
        Some(operation_id),
        "clash",
        *clash.snapshot_handle().load().version.as_ref(),
        settlement,
    );
    assert_eq!(
        commit.runtime,
        crate::client::runtime::RuntimeCommitStatus::RecoveryRequired
    );
    assert_eq!(degradations.len(), 1);
    assert!(matches!(
        degradations[0].reason,
        crate::client::runtime::DegradationReason::RuntimeRecoveryRequired { .. }
    ));
}
