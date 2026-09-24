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
    Ack, AckOptions, PersistentStateManager, PersistentStateManagerSetup, ReplaceIfVersionError,
    ReplaceIfVersionResult, RollbackReason, StateAckSubscriber, StateChange, StateParticipant,
    SubscriberName, error::StateChangedError,
};
use nyanpasu_core_manager::{CoreErrorKind, CoreKind, OperationId};
use nyanpasu_ipc::api::status::CoreStateDetail;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::Notify;

use super::{
    super::{
        ApplicationWorkflowArgs, ApplicationWorkflowClient, DirtyNotifier, adapters,
        impact::{
            ActivationIntent, ContentDigest, MutationHints, RequestedRuntimeFields, TouchedContent,
        },
        mutation::{
            CheckRecord, DEFERRED_RETRY_BUDGET, EvidenceGap, MutationBudgets, MutationConclusion,
            MutationOutcomeKind, MutationReceipt, RefusalCause,
        },
        participant::ApplicationMutationParticipant,
        policy::{CommandClass, CommandPolicy},
    },
    live_mutation_contexts,
};
use crate::{
    client::{
        SessionPortResolver, runtime,
        tests::{TestCheckAnswer, TestControlEndpoint},
    },
    core::actor_v2::{
        CoreClient,
        endpoint::ExecutionHost,
        service_actor::{ServiceClient, ServiceHostAdapter},
    },
};

// -- fixture ---------------------------------------------------------------

/// The real build, with a switch that holds a candidate inside it.
///
/// Parking the build is how these tests keep a Try in flight for as long as they
/// need without a sleep: the tracked task is genuinely mid-operation, which is
/// the state every ordering claim here is about.
struct ParkingBuilder {
    delegate: adapters::FsRuntimeBuildAdapter,
    entered: Notify,
    release: Notify,
    park: AtomicBool,
    /// Scripts the build panicking, which is the one way a mutation finishes
    /// with no receipt at all.
    panic: AtomicBool,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl super::super::ports::RuntimeBuildPort for ParkingBuilder {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> anyhow::Result<super::super::inputs::FrozenProfileContent> {
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.park.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        assert!(!self.panic.load(Ordering::SeqCst), "scripted build panic");
        self.delegate
            .build(revision, inputs, ports, strict_transforms)
            .await
    }
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()> {
        self.delegate.publish(snapshot).await
    }
}

/// The daemon preparation, with its first probe held still.
///
/// `change_execution_host` asks the service host whether it is ready before it
/// can own anything, and that leg is the one the participant's ACK budget
/// cannot bound: it can install and start a daemon. Parking it is how the ACK
/// budget is made to elapse while the Try is genuinely mid-handoff.
struct ParkingDaemon {
    delegate: crate::client::tests::HostTransitionServiceAdapter,
    entered: Notify,
    release: Notify,
    park: AtomicBool,
}

#[async_trait::async_trait]
impl ServiceHostAdapter for ParkingDaemon {
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, String> {
        if self.park.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), String> {
        self.delegate.install().await
    }
    async fn uninstall(&self) -> Result<(), String> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), String> {
        self.delegate.start_daemon().await
    }
    async fn stop_daemon(&self) -> Result<(), String> {
        self.delegate.stop_daemon().await
    }
    async fn update(&self) -> Result<(), String> {
        self.delegate.update().await
    }
    fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
        self.delegate.endpoint()
    }
}

struct Fixture {
    client: ApplicationWorkflowClient,
    endpoint: Arc<TestControlEndpoint>,
    builder: Arc<ParkingBuilder>,
    application: PersistentStateManager<NyanpasuAppConfig>,
    clash: PersistentStateManager<ClashConfig>,
    profiles: PersistentStateManager<Profiles>,
    store: runtime::RuntimeSnapshotStore,
    clash_path: Utf8PathBuf,
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
    _dir: tempfile::TempDir,
}

async fn manager<T>(path: Utf8PathBuf, state: T) -> PersistentStateManager<T>
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

fn temp_path(dir: &tempfile::TempDir, name: &str) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("temp path should be UTF-8")
}

/// The confirmed apply a session normally carries from its boot reconcile.
///
/// Its identity and exact bytes match the fresh fake endpoint observation.
/// A stale or unrelated receipt is tested explicitly rather than admitted by
/// the default fixture.
fn adopted_baseline() -> runtime::RuntimeApplyReceipt {
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
            .resolve_candidate(&ClashConfig::default())
            .expect("the default port strategies resolve"),
    }
}

/// A workflow wired to three real state managers and a fake control endpoint.
///
/// The endpoint publishes a running core with a known applied kind on purpose:
/// the baseline a Cancel restores is verified against what the host says is
/// running, and an absent fact is a mismatch rather than a benefit of the doubt.
async fn fixture(budgets: MutationBudgets) -> Fixture {
    fixture_with(budgets, true).await
}

/// The same graph with nothing applied yet: a running core this session has no
/// receipt for. That is the R10 evidence gap, and it is what a session whose
/// boot reconcile failed actually looks like.
async fn fixture_without_a_confirmed_apply(budgets: MutationBudgets) -> Fixture {
    fixture_with(budgets, false).await
}

async fn fixture_with(budgets: MutationBudgets, confirmed_apply: bool) -> Fixture {
    fixture_with_hosts(budgets, confirmed_apply, None).await
}

/// The same graph with a second execution host available, so a mutation that
/// asks for one can actually be given it. `service` is the endpoint the daemon
/// hands out once it is adopted.
async fn fixture_with_hosts(
    budgets: MutationBudgets,
    confirmed_apply: bool,
    service_host: Option<Arc<TestControlEndpoint>>,
) -> Fixture {
    let daemon = service_host
        .map(|endpoint| Arc::new(host_transition_daemon(endpoint)) as Arc<dyn ServiceHostAdapter>);
    fixture_with_daemon(budgets, confirmed_apply, daemon).await
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
    }
}

/// The same graph with the caller's own daemon, for a test that has to hold one
/// of the handoff's legs still.
async fn fixture_with_daemon(
    budgets: MutationBudgets,
    confirmed_apply: bool,
    daemon: Option<Arc<dyn ServiceHostAdapter>>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_source_hash(&nyanpasu_core_manager::payload_digest(b"mode: rule\n"));
    endpoint.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let service = match daemon {
        Some(daemon) => ServiceClient::spawn(daemon, 0).await.unwrap(),
        None => ServiceClient::spawn(Arc::new(crate::client::tests::IdleServiceAdapter), 0)
            .await
            .unwrap(),
    };

    let clash_path = temp_path(&dir, "clash-config.yaml");
    let app_path = temp_path(&dir, "application.yaml");
    let application = manager(app_path.clone(), NyanpasuAppConfig::default()).await;
    let clash = manager(clash_path.clone(), ClashConfig::default()).await;
    let profiles = manager(temp_path(&dir, "profiles.yaml"), Profiles::default()).await;

    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let builder = Arc::new(ParkingBuilder {
        delegate: adapters::FsRuntimeBuildAdapter {
            profiles_dir: dir.path().join("profiles"),
            paths: paths.clone(),
        },
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        park: AtomicBool::new(false),
        panic: AtomicBool::new(false),
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
    let (_notifier, dirty) = DirtyNotifier::channel();
    let client = ApplicationWorkflowClient::spawn_with_ticks(
        ApplicationWorkflowArgs {
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            core: core.clone(),
            service,
            builder: builder.clone(),
            validator: Arc::new(adapters::CoreCheckValidator::new(core.clone(), paths)),
            ports: ports.clone(),
            installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
            ui: Arc::new(crate::client::NoopUiEventSink),
            dirty,
            budgets,
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
        _dir: dir,
    }
}

/// Budgets that keep every wait short enough to observe without a sleep. The
/// admission budget stays generous; the tests that exercise it set it to zero.
fn test_budgets() -> MutationBudgets {
    MutationBudgets {
        admission: Duration::from_secs(10),

        decision_wait: Duration::from_secs(5),
    }
}

type Decorate<T> = Box<dyn FnOnce(StateParticipant<T>) -> StateParticipant<T> + Send>;

fn plain<T: Clone + Send + Sync + 'static>() -> Decorate<T> {
    Box::new(|participant| participant)
}

/// Runs one mutation of `manager` as the workflow's Required participant.
///
/// The attempt identity is the caller's so a test can address a settlement
/// before the attempt has finished, exactly as the domain actor will.
#[allow(clippy::too_many_arguments)]
async fn mutate<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    operation_id: OperationId,
    next: T,
    class: CommandClass,
    ack_timeout: Duration,
    decorate: Decorate<T>,
    local_write: impl FnOnce() -> futures::future::BoxFuture<'static, anyhow::Result<()>>
    + Send
    + 'static,
) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
where
    T: super::super::mutation::MutationDomain + Serialize + DeserializeOwned + Default,
{
    let version = manager.snapshot_handle().load().version;
    let client = client.clone();
    manager
        .replace_if_version_with_participant(
            version,
            next,
            move |decision| {
                decorate(ApplicationMutationParticipant::<T>::with_ack_timeout(
                    operation_id,
                    MutationHints::default(),
                    class,
                    decision,
                    client,
                    ack_timeout,
                ))
            },
            local_write,
            || async { Ok(()) },
        )
        .await
}

/// One mutation carrying request-local hints, which is how a content update
/// says that the bytes behind a managed path moved without the document doing
/// so.
async fn mutate_with_hints<T>(
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
    T: super::super::mutation::MutationDomain + Serialize + DeserializeOwned + Default,
{
    let operation_id = OperationId::generate();
    let version = manager.snapshot_handle().load().version;
    let client = client.clone();
    let result = manager
        .replace_if_version_with_participant(
            version,
            next,
            move |decision| {
                ApplicationMutationParticipant::<T>::with_ack_timeout(
                    operation_id,
                    hints,
                    class,
                    decision,
                    client,
                    Duration::from_secs(10),
                )
            },
            no_local_write,
            || async { Ok(()) },
        )
        .await;
    (operation_id, result)
}

/// The ordinary case: a fresh identity, no decorator, no local write.
async fn simple_mutate<T>(
    manager: &mut PersistentStateManager<T>,
    client: &ApplicationWorkflowClient,
    next: T,
    class: CommandClass,
) -> (
    OperationId,
    Result<ReplaceIfVersionResult, ReplaceIfVersionError>,
)
where
    T: super::super::mutation::MutationDomain + Serialize + DeserializeOwned + Default,
{
    let operation_id = OperationId::generate();
    let result = mutate(
        manager,
        client,
        operation_id,
        next,
        class,
        Duration::from_secs(10),
        plain(),
        no_local_write,
    )
    .await;
    (operation_id, result)
}

/// Parks the transaction between prepare and the compare-and-swap, which is
/// exactly the window in which the workflow sits in `AwaitDecision`.
fn parked_local_write(
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

async fn wait_queued(client: &ApplicationWorkflowClient, depth: usize) {
    let mut status = client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|status| status.queued.len() == depth),
    )
    .await
    .expect("the queue should reach the expected depth")
    .expect("the workflow should stay alive");
}

fn refused(result: &Result<ReplaceIfVersionResult, ReplaceIfVersionError>) -> bool {
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
fn unserviceable_check() -> nyanpasu_core_manager::CoreError {
    nyanpasu_core_manager::CoreError::new(
        CoreErrorKind::BackendUnavailable,
        "scripted: the config check service is briefly unreachable",
        true,
    )
}

fn app_with_core(core: ClashCore) -> NyanpasuAppConfig {
    NyanpasuAppConfig {
        core,
        ..NyanpasuAppConfig::default()
    }
}

fn no_local_write() -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
    Box::pin(std::future::ready(Ok(())))
}

async fn barrier(client: &ApplicationWorkflowClient) {
    super::barrier(client).await;
}

/// The structured record of one attempt, once the workflow has settled it.
async fn settled(client: &ApplicationWorkflowClient, operation_id: OperationId) -> MutationReceipt {
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
fn names_overrides() -> MutationHints {
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

fn overrides(value: serde_json::Value) -> ClashConfig {
    use struct_patch::Patch;
    let mut config = ClashConfig::default();
    config
        .overrides
        .apply(serde_json::from_value(value).unwrap());
    config
}

// -- participants used to script the transaction ---------------------------

/// A registered subscriber that vetoes every prepare, so the transaction aborts
/// after the workflow's Try has already run.
struct Rejector;

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateAckSubscriber<T> for Rejector {
    fn name(&self) -> SubscriberName<'_> {
        "rejector".into()
    }
    async fn on_prepare(&self, _change: StateChange<T>) -> Ack {
        Ack::Rejected("scripted domain veto".to_string())
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
    fn ack_options(&self) -> AckOptions {
        self.inner.ack_options()
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

// -- R4: admission gates the commit ----------------------------------------

/// Closing refuses new mutations, and it refuses them *before* anything is
/// persisted. Under the pre-participant shape the facade had already committed
/// by the time the workflow could say no.
#[tokio::test]
async fn a_closing_workflow_refuses_a_mutation_before_anything_is_committed() {
    let mut f = fixture(test_budgets()).await;
    f.client.shutdown().await.unwrap();

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

/// An execution domain that is already busy refuses the mutation once its
/// admission budget is spent, rather than holding the source transaction's
/// writer permit open behind an unbounded queue.
#[tokio::test]
async fn an_occupied_domain_refuses_a_mutation_whose_admission_budget_is_spent() {
    let mut budgets = test_budgets();
    budgets.admission = Duration::ZERO;
    let mut f = fixture(budgets).await;
    f.builder.park.store(true, Ordering::SeqCst);
    let reconcile = {
        let client = f.client.clone();
        tokio::spawn(async move { client.reconcile().await })
    };
    f.builder.entered.notified().await;

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
    assert_eq!(
        f.builder.calls.load(Ordering::SeqCst),
        1,
        "the refused mutation never built a candidate"
    );

    f.builder.park.store(false, Ordering::SeqCst);
    f.builder.release.notify_one();
    reconcile.await.unwrap().unwrap();
    f.client.shutdown().await.unwrap();
}

/// An isolated execution domain refuses mutations too: nothing may be committed
/// against a runtime nobody can describe.
#[tokio::test]
async fn an_isolated_execution_domain_refuses_a_mutation() {
    let mut f = fixture(test_budgets()).await;
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
    } = fixture(test_budgets()).await;
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

    let second = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = simple_mutate(
                &mut clash,
                &client,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
            )
            .await;
            (clash, result)
        })
    };
    wait_queued(&client, 1).await;
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

/// A settlement carries the identity of the attempt that created it, so one
/// arriving late for a finished attempt cannot cancel the attempt running now
/// (V19).
#[tokio::test]
async fn a_late_cancel_for_a_settled_attempt_never_touches_the_current_one() {
    let Fixture {
        client,
        mut clash,
        _dir,
        ..
    } = fixture(test_budgets()).await;

    let (first, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, first).await.conclusion,
        MutationConclusion::Confirmed
    );

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let second = OperationId::generate();
    let current = {
        let client = client.clone();
        let (entered, release) = (entered.clone(), release.clone());
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                second,
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
                Duration::from_secs(10),
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };
    // The current attempt is past its Try and waiting for its own decision.
    entered.notified().await;

    client.wake_mutation(first);
    barrier(&client).await;

    release.notify_one();
    let (clash, result) = current.await.unwrap();
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, second).await.conclusion,
        MutationConclusion::Confirmed,
        "the current attempt follows its own decision"
    );
    assert!(
        !client.status().uncertain,
        "a settlement matching the authoritative decision is not a conflict"
    );
    assert_eq!(
        serde_json::to_value(clash.snapshot().overrides.clone()).unwrap()["mode"],
        "direct"
    );
}

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
    } = fixture(test_budgets()).await;
    let kept = Arc::new(StdMutex::new(None));

    let operation_id = OperationId::generate();
    let result = mutate(
        &mut clash,
        &client,
        operation_id,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
        Duration::from_secs(10),
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
/// Aborted, so the workflow cancels and puts the baseline back (V26).
#[tokio::test]
async fn a_lost_rollback_notification_is_resolved_by_the_authoritative_decision() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        store,
        _dir,
        ..
    } = fixture(test_budgets()).await;

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
        Duration::from_secs(10),
        drop_signal(false, kept.clone()),
        no_local_write,
    )
    .await;

    assert!(refused(&result), "{result:?}");
    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
    assert_eq!(
        store
            .last_confirmed_runtime_receipt()
            .expect("the baseline is the checkpoint again")
            .config_digest,
        baseline.config_digest
    );
    let submitted = endpoint.reconciled_bytes();
    assert_eq!(submitted.len(), 3, "primed, tried, restored");
    assert_eq!(
        submitted[2], submitted[0],
        "the restore resubmits the baseline document, not the withdrawn candidate"
    );
    assert!(kept.lock().unwrap().is_some());
}

// -- 图 13: an abandoned prepare never abandons the Try ---------------------

/// The ACK budget elapses while the Try is in flight. The transaction aborts,
/// but the Try is a tracked task: the workflow waits for its real terminal
/// result, restores the baseline, and only then releases the execution domain.
/// The next attempt is admitted after that, never during it (V24).
#[tokio::test]
async fn an_abandoned_prepare_waits_for_the_try_before_restoring_and_releasing() {
    let Fixture {
        client,
        endpoint,
        builder,
        mut application,
        mut clash,
        store,
        _dir,
        ..
    } = fixture(test_budgets()).await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let committed = clash.snapshot_handle().load().version;

    builder.park.store(true, Ordering::SeqCst);
    let abandoned = OperationId::generate();
    let timed_out = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                abandoned,
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
                // Short enough that the budget elapses while the Try is parked.
                Duration::from_millis(50),
                plain(),
                no_local_write,
            )
            .await;
            (clash, result)
        })
    };
    builder.entered.notified().await;

    // The transaction gives up on the prepare here; the next mutation must
    // still wait for the Try that is holding the execution domain.
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
    wait_queued(&client, 1).await;
    let (clash, abandoned_result) = timed_out.await.unwrap();
    assert!(refused(&abandoned_result), "{abandoned_result:?}");
    assert_eq!(
        builder.calls.load(Ordering::SeqCst),
        2,
        "the queued mutation has not started while the abandoned Try runs"
    );

    builder.park.store(false, Ordering::SeqCst);
    builder.release.notify_one();
    let (application, (next_id, next_result)) = next.await.unwrap();
    assert!(matches!(next_result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&client, next_id).await.conclusion,
        MutationConclusion::Confirmed
    );

    let receipt = settled(&client, abandoned).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
    assert_eq!(
        clash.snapshot_handle().load().version,
        committed,
        "the abandoned attempt committed nothing"
    );
    let submitted = endpoint.reconciled_bytes();
    assert_eq!(submitted.len(), 4, "primed, tried, restored, next");
    assert_eq!(
        submitted[2], submitted[0],
        "the baseline is restored before the next mutation is admitted"
    );
    assert_eq!(
        store
            .read()
            .promoted
            .expect("the next mutation publishes")
            .target_core,
        ClashCore::ClashRs
    );
    drop(application);
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
    } = fixture(test_budgets()).await;

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

    let (operation_id, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "rule"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );

    let receipt = settled(&client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(receipt.conclusion, MutationConclusion::Cancelled);
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
    } = fixture(test_budgets()).await;

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
        _dir,
        ..
    } = fixture(test_budgets()).await;

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
                Duration::from_secs(10),
                plain(),
                parked_local_write(entered, release),
            )
            .await;
            (clash, result)
        })
    };
    entered.notified().await;

    let shutdown = {
        let client = client.clone();
        tokio::spawn(async move { client.shutdown().await })
    };
    let mut status = client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|status| status.shutting_down),
    )
    .await
    .unwrap()
    .unwrap();

    let before = application.snapshot_handle().load().version;
    let (_, rejected) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&rejected), "{rejected:?}");
    assert_eq!(application.snapshot_handle().load().version, before);

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
    assert!(shutdown.await.unwrap().unwrap().stop.is_ok());
    drop(clash);
}

// -- I1: an abort whose persistence outcome is unknown ---------------------

/// The source transaction is abandoned while its own local write is
/// outstanding. The store was never swapped, which is no evidence that nothing
/// reached disk: the file may already hold the candidate. Treating that as an
/// ordinary Cancel would take the candidate off the runtime and leave the next
/// boot to load it back, so the attempt isolates the execution domain instead
/// (v2 §4.2).
#[tokio::test]
async fn an_abort_whose_persistence_outcome_is_unknown_isolates_the_domain() {
    let Fixture {
        client,
        endpoint,
        mut clash,
        mut application,
        _dir,
        ..
    } = fixture(test_budgets()).await;

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let abandoned = OperationId::generate();
    let version = clash.snapshot_handle().load().version;
    let workflow = client.clone();
    {
        let writing = entered.clone();
        let released = release.clone();
        let mut pending = Box::pin(clash.replace_if_version_with_participant(
            version,
            overrides(serde_json::json!({"mode": "global"})),
            move |decision| {
                ApplicationMutationParticipant::new(
                    abandoned,
                    MutationHints::default(),
                    CommandClass::Save,
                    decision,
                    workflow,
                )
            },
            move || async move {
                writing.notify_one();
                released.notified().await;
                Ok(())
            },
            || async { Err(anyhow::anyhow!("resource recovery failed")) },
        ));
        tokio::select! {
            result = &mut pending => panic!("write should stay pending: {result:?}"),
            _ = entered.notified() => {}
        }
        drop(pending);
    }
    release.notify_one();

    let receipt = settled(&client, abandoned).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Applied);
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::RecoveryRequired,
        "an abort that may have written the candidate is not a Cancel"
    );
    assert!(client.status().uncertain);
    let recovery = client
        .mutation_journal()
        .recovery
        .expect("an isolated domain names why");
    assert_eq!(recovery.operation_id, abandoned);
    assert_eq!(
        recovery.stage,
        super::super::mutation::MutationStage::AwaitDecision
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        1,
        "the candidate was applied and nothing undid it"
    );

    // And the isolation holds: nothing new may be committed against a runtime
    // nobody can describe.
    let before = application.snapshot_handle().load().version;
    let (_, refused_result) = simple_mutate(
        &mut application,
        &client,
        app_with_core(ClashCore::ClashRs),
        CommandClass::ExplicitSwitch,
    )
    .await;
    assert!(refused(&refused_result), "{refused_result:?}");
    assert_eq!(application.snapshot_handle().load().version, before);
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
    } = fixture_without_a_confirmed_apply(test_budgets()).await;

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
    assert_eq!(
        receipt.refusal,
        Some(RefusalCause::Evidence(EvidenceGap::NoRestorableBaseline))
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

// -- the check is never skipped --------------------------------------------

/// A candidate the core rejects is refused before it reaches the runtime, and a
/// check that could not run is not a passing one (v2 §2.4, V01/V02).
#[tokio::test]
async fn a_rejected_check_refuses_the_mutation_without_touching_the_runtime() {
    let mut f = fixture(test_budgets()).await;
    f.endpoint.set_check_answer(TestCheckAnswer::Reject(
        nyanpasu_core_manager::CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "scripted: the core will not run this document",
            false,
        ),
    ));

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
        "a rejected candidate is never submitted"
    );
    let receipt = settled(&f.client, operation_id).await;
    assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
    assert_eq!(receipt.conclusion, MutationConclusion::Withdrawn);
    // A Try that ran and was refused, not an evidence gap: the two refusals are
    // different facts and the receipt is where they stay apart.
    assert_eq!(
        receipt.refusal,
        Some(RefusalCause::Try(
            super::super::policy::TryCauseKind::Deterministic
        ))
    );
    assert!(!f.client.status().uncertain);
}

/// A core the user stopped is an intent. The candidate is validated and saved,
/// nothing is started, and no retry loop is armed against the Stop (R7, V11).
#[tokio::test]
async fn a_stopped_core_saves_the_checked_target_without_starting_it() {
    let mut f = fixture(test_budgets()).await;
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
    let mut f = fixture(test_budgets()).await;
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
    assert_eq!(
        receipt.refusal,
        Some(RefusalCause::Evidence(EvidenceGap::BaselineUnconfirmed))
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
    let mut f = fixture(test_budgets()).await;
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
        let mut f = fixture(test_budgets()).await;
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
        assert_eq!(
            receipt.policy,
            CommandPolicy::MustApply,
            "{state:?}: a transition is not a stop, so the command keeps its policy"
        );
        assert_ne!(receipt.outcome, MutationOutcomeKind::SavedInactive);
        assert_ne!(receipt.outcome, MutationOutcomeKind::Applied);
        assert_eq!(receipt.outcome, MutationOutcomeKind::Rejected);
        assert_eq!(
            receipt.refusal,
            Some(RefusalCause::Evidence(EvidenceGap::CoreTransitioning)),
            "{state:?}"
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
    let mut f = fixture(test_budgets()).await;
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
    let mut f = fixture(test_budgets()).await;
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
    } = fixture(test_budgets()).await;

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
    } = fixture(test_budgets()).await;

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
        mutation::{ApplyFailure, RefusalCause, RetryableCause, RuntimePrepareOutcome, TryAck},
        policy::TryCauseKind,
    };

    assert_eq!(RuntimePrepareOutcome::Saved.ack(), TryAck::Ok);
    assert_eq!(RuntimePrepareOutcome::SavedInactive.ack(), TryAck::Ok);
    assert_eq!(
        RuntimePrepareOutcome::Deferred {
            baseline: super::super::mutation::KnownRuntimeState::Stopped,
            digest: "digest".into(),
            cause: RetryableCause {
                stage: super::super::mutation::MutationStage::TryingCritical,
                message: "briefly unreachable".into(),
            },
        }
        .ack(),
        TryAck::Degraded("briefly unreachable".into())
    );
    assert_eq!(
        RuntimePrepareOutcome::Rejected {
            cause: ApplyFailure {
                stage: super::super::mutation::MutationStage::TryingCritical,
                cause: RefusalCause::Try(TryCauseKind::Deterministic),
                message: "the core rejected it".into(),
            },
            restored: super::super::mutation::KnownRuntimeState::Stopped,
        }
        .ack(),
        TryAck::Rejected("the core rejected it".into())
    );
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
    } = fixture_with_hosts(test_budgets(), true, Some(service_host.clone())).await;

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
            let result = application
                .replace_if_version_with_participant(
                    version,
                    NyanpasuAppConfig {
                        enable_service_mode: true,
                        ..NyanpasuAppConfig::default()
                    },
                    move |decision| {
                        ApplicationMutationParticipant::<NyanpasuAppConfig>::with_ack_timeout(
                            operation_id,
                            MutationHints::default(),
                            CommandClass::ExplicitSwitch,
                            decision,
                            client,
                            Duration::from_secs(10),
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
    assert_eq!(
        receipt.impact,
        super::super::impact::RuntimeImpact::HostSwitch
    );
    assert_eq!(receipt.policy, CommandPolicy::MustApply);
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
    } = fixture_with_hosts(test_budgets(), true, Some(service_host.clone())).await;

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
    } = fixture_with_hosts(test_budgets(), true, Some(service_host.clone())).await;

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
            .stage,
        super::super::mutation::MutationStage::TryingCritical
    );
    assert!(!application.snapshot().enable_service_mode);
}

/// B5 / 图 13, on the leg the ACK budget most plainly cannot bound.
///
/// The participant's ACK budget bounds the *transaction's wait* for a verdict,
/// never the Try. Here it elapses while the daemon preparation is still
/// running, so the transaction aborts with nothing decided about the runtime.
/// An elapsed budget is not a cancellation: the handoff completes, the
/// candidate is applied on the host it asked for, and the execution domain
/// stays held until that real terminal result arrives and the Cancel has put
/// the original host and runtime back. The existing parked-build coverage
/// stops short of these service transitions.
#[tokio::test]
async fn an_expired_ack_keeps_the_domain_until_the_handoff_is_compensated() {
    let service_host = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    service_host.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
        Some(CoreKind::Mihomo),
    );
    let daemon = Arc::new(ParkingDaemon {
        delegate: host_transition_daemon(service_host.clone()),
        entered: Notify::new(),
        release: Notify::new(),
        // Armed after the graph is up: the service actor probes on its own
        // while it starts, and the leg this test holds is the handoff's.
        park: AtomicBool::new(false),
    });
    let Fixture {
        client,
        endpoint,
        core,
        mut clash,
        mut application,
        _dir,
        ..
    } = fixture_with_daemon(
        test_budgets(),
        true,
        Some(daemon.clone() as Arc<dyn ServiceHostAdapter>),
    )
    .await;

    let (_, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    let restored_from = endpoint.reconciled_bytes().len();
    let before = application.snapshot_handle().load().version;
    daemon.park.store(true, Ordering::SeqCst);

    let abandoned = OperationId::generate();
    let timed_out = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = mutate(
                &mut application,
                &client,
                abandoned,
                NyanpasuAppConfig {
                    enable_service_mode: true,
                    ..NyanpasuAppConfig::default()
                },
                CommandClass::ExplicitSwitch,
                // Short enough that the budget elapses inside the preparation.
                Duration::from_millis(50),
                plain(),
                no_local_write,
            )
            .await;
            (application, result)
        })
    };
    daemon.entered.notified().await;

    // The transaction gives up on the prepare here. The Try is mid-handoff, so
    // the next mutation must queue rather than be admitted onto a runtime
    // nobody can describe yet.
    let next = {
        let client = client.clone();
        tokio::spawn(async move {
            let result = simple_mutate(
                &mut clash,
                &client,
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
            )
            .await;
            (clash, result)
        })
    };
    wait_queued(&client, 1).await;
    let (application, abandoned_result) = timed_out.await.unwrap();
    assert!(refused(&abandoned_result), "{abandoned_result:?}");
    assert_eq!(
        application.snapshot_handle().load().version,
        before,
        "an elapsed ACK budget commits nothing"
    );
    assert_eq!(
        endpoint.reconciled_bytes().len(),
        restored_from,
        "and it restores nothing while the handoff it started is still running"
    );

    daemon.release.notify_one();
    let receipt = settled(&client, abandoned).await;
    assert_eq!(
        receipt.conclusion,
        MutationConclusion::Cancelled,
        "the abandoned attempt is settled by the Try's own terminal result: {:?}",
        receipt.detail
    );
    assert_eq!(
        core.status().host,
        ExecutionHost::Local,
        "and the host it moved is put back"
    );
    assert!(
        endpoint.reconciled_bytes().len() > restored_from,
        "together with the runtime that was running on it"
    );

    // Only now is the queued mutation admitted, and it reads a runtime the
    // compensation has already settled.
    let (_clash, (queued, queued_result)) = next.await.unwrap();
    assert!(
        matches!(queued_result, Ok(ReplaceIfVersionResult::Replaced)),
        "{queued_result:?}"
    );
    assert_eq!(
        settled(&client, queued).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert!(!client.status().uncertain);
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
    } = fixture(test_budgets()).await;

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
                Duration::from_secs(10),
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
            .stage,
        super::super::mutation::MutationStage::Cancelling
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
    } = fixture(test_budgets()).await;

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
                Duration::from_secs(10),
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
    assert!(
        client
            .mutation_journal()
            .recovery
            .unwrap()
            .runtime_operation
            .is_some(),
        "an unknown restore retains its lower operation identity"
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
    } = fixture(test_budgets()).await;

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
                Duration::from_secs(10),
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
    } = fixture(test_budgets()).await;

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
    } = fixture(test_budgets()).await;

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
    } = fixture(test_budgets()).await;

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
    } = fixture(test_budgets()).await;
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
/// preserves the target and its budget, drives nothing, and commits as the
/// plain save it is. Saving that document back unchanged does not ask for it
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
    } = fixture(test_budgets()).await;

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
    let (second, result) =
        simple_mutate(&mut clash, &client, unrelated.clone(), CommandClass::Save).await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    let receipt = settled(&client, second).await;
    assert_eq!(
        receipt.outcome,
        MutationOutcomeKind::Saved,
        "a save that moved no build input has nothing critical to try"
    );
    assert_eq!(receipt.policy, CommandPolicy::SaveOnly);
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
    let (no_op, result) =
        simple_mutate(&mut clash, &client, unrelated.clone(), CommandClass::Save).await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    let receipt = settled(&client, no_op).await;
    assert_eq!(
        receipt.outcome,
        MutationOutcomeKind::Saved,
        "an empty diff is not evidence that the runtime was asked for anything"
    );
    assert_eq!(receipt.policy, CommandPolicy::SaveOnly);
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
    } = fixture(test_budgets()).await;

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

// -- §11.4: every terminal path retires its context -------------------------

/// A mutation drained by an isolated execution domain is a finished attempt.
/// Its context has to move into the bounded history with it: nothing retires it
/// afterwards, because the queue it would be withdrawn from no longer holds it.
#[tokio::test]
async fn an_isolated_domain_retires_the_contexts_of_the_mutations_it_drains() {
    let Fixture {
        client,
        endpoint,
        builder,
        mut clash,
        mut application,
        clash_path,
        _dir,
        ..
    } = fixture(test_budgets()).await;

    let (priming, primed) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(primed, Ok(ReplaceIfVersionResult::Replaced)));
    settled(&client, priming).await;
    assert_eq!(live_mutation_contexts(&client).await, 0);

    // The first attempt parks inside its build, so the second one queues behind
    // it and is still in the FIFO when the first isolates the domain.
    builder.park.store(true, Ordering::SeqCst);
    let first = OperationId::generate();
    let isolating = {
        let client = client.clone();
        let restore_endpoint = endpoint.clone();
        tokio::spawn(async move {
            let result = mutate(
                &mut clash,
                &client,
                first,
                overrides(serde_json::json!({"mode": "direct"})),
                CommandClass::Save,
                Duration::from_secs(10),
                plain(),
                move || {
                    Box::pin(async move {
                        restore_endpoint.set_result_missing(true);
                        Ok(())
                    })
                },
            )
            .await;
            (clash, result)
        })
    };
    builder.entered.notified().await;

    let queued = {
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
    wait_queued(&client, 1).await;
    assert_eq!(live_mutation_contexts(&client).await, 2);

    // From here the first attempt's save fails and its restore cannot be
    // verified, which is what isolates the execution domain.
    std::fs::remove_file(&clash_path).unwrap();
    std::fs::create_dir_all(&clash_path).unwrap();
    builder.release.notify_one();

    let (_clash, result) = isolating.await.unwrap();
    assert!(
        matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))),
        "{result:?}"
    );
    assert_eq!(
        settled(&client, first).await.conclusion,
        MutationConclusion::RecoveryRequired
    );

    let (_application, (_, queued_result)) = queued.await.unwrap();
    assert!(refused(&queued_result), "{queued_result:?}");
    barrier(&client).await;
    assert_eq!(
        live_mutation_contexts(&client).await,
        0,
        "a drained mutation must not leave a context nothing will ever retire"
    );
}

/// A panicking mutation produces no receipt, and the attempt is over all the
/// same. The context follows it out.
#[tokio::test]
async fn a_panicking_mutation_retires_its_context() {
    let Fixture {
        client,
        builder,
        mut clash,
        _dir,
        ..
    } = fixture(test_budgets()).await;

    builder.panic.store(true, Ordering::SeqCst);
    let (_, result) = simple_mutate(
        &mut clash,
        &client,
        overrides(serde_json::json!({"mode": "direct"})),
        CommandClass::Save,
    )
    .await;
    assert!(
        matches!(result, Err(ReplaceIfVersionError::State(_))),
        "{result:?}"
    );

    barrier(&client).await;
    assert!(client.status().uncertain);
    assert_eq!(
        live_mutation_contexts(&client).await,
        0,
        "a panicked attempt must not leave a context behind either"
    );
}

#[tokio::test]
async fn a_preflight_failure_or_changed_revision_never_submits_or_isolates() {
    for revision_changed in [false, true] {
        let f = fixture(test_budgets()).await;
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

#[tokio::test]
async fn a_gui_save_does_not_query_core_status() {
    let mut f = fixture(test_budgets()).await;
    f.core.refresh_status().await.unwrap();
    let before = f.endpoint.status_reads();
    f.endpoint.set_status_fails(true);
    let mut app = f.application.snapshot().as_ref().clone();
    app.language = nyanpasu_config::application::I18nLanguage::English;
    let (id, result) = simple_mutate(&mut f.application, &f.client, app, CommandClass::Save).await;
    assert!(result.is_ok());
    assert_eq!(
        settled(&f.client, id).await.conclusion,
        MutationConclusion::Confirmed
    );
    assert_eq!(f.endpoint.status_reads(), before);
}

#[tokio::test]
async fn cross_domain_deferrals_share_the_complete_latest_target() {
    let mut f = fixture(test_budgets()).await;
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
        f.client.mutation_journal().deferred.unwrap().digest,
        deferred.digest
    );
}

#[tokio::test]
async fn selecting_the_saved_host_again_moves_the_actual_host() {
    let service = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    let mut f = fixture_with_hosts(test_budgets(), true, Some(service.clone())).await;
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
    let mut f = fixture(test_budgets()).await;
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
    async fn probe(&self) -> Result<nyanpasu_ipc::types::StatusInfo<'static>, String> {
        crate::client::tests::IdleServiceAdapter.probe().await
    }
    async fn install(&self) -> Result<(), String> {
        Err("the user cancelled the elevation prompt".into())
    }
    async fn uninstall(&self) -> Result<(), String> {
        unreachable!()
    }
    async fn start_daemon(&self) -> Result<(), String> {
        unreachable!()
    }
    async fn stop_daemon(&self) -> Result<(), String> {
        unreachable!()
    }
    async fn update(&self) -> Result<(), String> {
        unreachable!()
    }
    fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
        unreachable!()
    }
}

#[tokio::test]
async fn refused_service_install_does_not_isolate_the_local_runtime() {
    let mut f = fixture_with_daemon(test_budgets(), true, Some(Arc::new(RefusedInstall))).await;
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
    let mut f = fixture(test_budgets()).await;
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
    let f = fixture(test_budgets()).await;
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
    let built = tokio::task::spawn_blocking(move || {
        let scripts = crate::enhance::EnhanceScriptRunner::new().unwrap();
        crate::enhance::RuntimeBuilder::build(&input, &old_content, &scripts)
    })
    .await
    .unwrap();
    assert!(built.is_ok());
    let captured = f.builder.capture_content(&profiles).await.unwrap();
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
        build(revisions.allocate().unwrap(), false).await.is_ok(),
        "ordinary build keeps D7 passthrough"
    );
    assert!(
        build(revisions.allocate().unwrap(), true).await.is_err(),
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
    } = fixture(test_budgets()).await;
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
                Duration::from_secs(10),
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
