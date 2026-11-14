//! Committed-source execution, independent checks, and recovery through typed owners.

use nyanpasu_contracts::service::ServiceProcessControl;
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::client::test_support::workflow::Installer;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig, NyanpasuAppConfigPatch, ReleaseChannel},
    clash::config::ClashConfig,
    profile::{ManagedProfilePath, Profiles},
};
use nyanpasu_core_manager::{CoreErrorKind, CoreKind};
use nyanpasu_ipc::api::status::CoreStateDetail;
use nyanpasu_migration::modules::application::ApplicationFormat;
use nyanpasu_state_engine::{PersistentStateManager, PersistentStateManagerSetup};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::Notify;

use super::{
    super::{ApplicationWorkflowArgs, ApplicationWorkflowClient, adapters},
    RecordingNotifications, ScriptedWaitEndpoint, WaitScript,
};
use crate::{
    actor_v2::{CoreClient, service_actor::ServiceClient},
    client::{
        SessionPortResolver,
        core_lifecycle::Ownership,
        runtime,
        test_support::{TestCheckAnswer, TestControlEndpoint, runtime_build::TestRuntimeBuildPort},
    },
    state::mutation::runtime_apply::RuntimeApplyReceipt,
};
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    mutation::{CommitAborted, RuntimeAftermath},
    runtime_error::RuntimeError,
};

// -- fixture ---------------------------------------------------------------

/// The real build, with a switch that holds a candidate inside it.
///
/// Parking the build is how these tests keep a Try in flight for as long as they
/// need without a sleep: the tracked task is genuinely mid-operation, which is
/// the state every ordering claim here is about.
pub(super) struct ParkingBuilder {
    delegate: TestRuntimeBuildPort,
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
    ) -> Result<nyanpasu_core_manager::CoreSpec, nyanpasu_runtime_values::core_spec::CoreSpecError>
    {
        self.delegate.core_spec(core)
    }
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, nyanpasu_profile_transform::RuntimeBuildError> {
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
    ) -> Result<(), nyanpasu_runtime_values::publish::PublishRuntimeError> {
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
    pub(super) application: crate::client::application::ApplicationClient,
    pub(super) clash: crate::client::clash_config::ClashConfigClient,
    pub(super) store: runtime::RuntimeSnapshotStore,
    pub(super) ports: Arc<SessionPortResolver>,
    app_path: Utf8PathBuf,
    /// Where the build reads managed profile content from, so a test that
    /// drives the profiles domain can put a real document behind a path.
    /// The same client the workflow holds, so a test can read which host owns
    /// the runtime at a point of its choosing.
    /// The same resolver the workflow holds, and the one the rest of the
    /// application reads its endpoint from.
    /// The endpoint the core is reached through, when a test scripts how its
    /// operation waits answer.
    pub(super) scripted: Option<Arc<ScriptedWaitEndpoint>>,
    /// What the workflow told the effects owner.
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

pub(super) async fn clash_manager(
    path: Utf8PathBuf,
    state: ClashConfig,
) -> PersistentStateManager<ClashConfig, nyanpasu_migration::modules::clash_config::ClashConfigFormat>
{
    PersistentStateManagerSetup::builder()
        .config_path(path)
        .assemble()
        .from_state(state)
        .await
        .unwrap()
}

/// [`manager`] for the application config, which the app persists stamped.
pub(super) async fn application_manager(
    path: Utf8PathBuf,
    state: NyanpasuAppConfig,
) -> PersistentStateManager<NyanpasuAppConfig, ApplicationFormat> {
    PersistentStateManagerSetup::<NyanpasuAppConfig, ApplicationFormat>::builder()
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
pub(super) fn adopted_baseline() -> RuntimeApplyReceipt {
    RuntimeApplyReceipt {
        instance_id: None,
        revision: runtime::tests::test_revision(),
        target_core: ClashCore::default(),
        host: nyanpasu_runtime_values::execution_host::ExecutionHost::Local,

        binding: crate::actor_v2::facade::AppliedConfigBinding {
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: nyanpasu_core_manager::payload_digest(b"mode: rule\n"),
                effective_hash: "effective".into(),
            },
            host: nyanpasu_runtime_values::execution_host::ExecutionHost::Local,
            generation: 0,
        },
        ports: SessionPortResolver::default()
            .resolve_candidate(&crate::client::test_support::test_clash_config())
            .expect("the default port strategies resolve"),
        target: None,
    }
}

pub(super) async fn fixture() -> Fixture {
    fixture_with(true).await
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
    let daemon = service_host.map(|endpoint| {
        let daemon = Arc::new(host_transition_daemon(endpoint));
        (
            daemon.clone() as Arc<dyn ServiceProcessControl>,
            daemon as Arc<dyn crate::actor_v2::service_actor::ServiceEndpointFactory>,
        )
    });
    fixture_with_daemon(confirmed_apply, daemon).await
}

/// The ordinary daemon over a service-side control endpoint, which the tests
/// that need to park one of its legs wrap instead of reimplementing.
fn host_transition_daemon(
    endpoint: Arc<TestControlEndpoint>,
) -> crate::actor_v2::test_support::HostTransitionServiceAdapter {
    crate::actor_v2::test_support::HostTransitionServiceAdapter {
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
    daemon: Option<(
        Arc<dyn ServiceProcessControl>,
        Arc<dyn crate::actor_v2::service_actor::ServiceEndpointFactory>,
    )>,
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
    daemon: Option<(
        Arc<dyn ServiceProcessControl>,
        Arc<dyn crate::actor_v2::service_actor::ServiceEndpointFactory>,
    )>,
    scripted: bool,
) -> Fixture {
    let service = match daemon {
        Some((process_control, endpoint_factory)) => {
            ServiceClient::spawn(process_control, endpoint_factory, 0)
                .await
                .unwrap()
        }
        None => {
            let process_control = Arc::new(crate::actor_v2::test_support::IdleServiceAdapter);
            ServiceClient::spawn(process_control.clone(), process_control, 0)
        }
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
    core.refresh_status().await.unwrap();

    let clash_path = temp_path(&dir, "clash-config.yaml");
    let app_path = temp_path(&dir, "application.yaml");
    let application = application_manager(app_path.clone(), NyanpasuAppConfig::default()).await;
    let clash = clash_manager(
        clash_path.clone(),
        crate::client::test_support::test_clash_config(),
    )
    .await;
    let profiles = manager(temp_path(&dir, "profiles.yaml"), Profiles::default()).await;

    let paths = runtime::RuntimePaths::from_resolver(&crate::utils::test_support::test_paths(
        dir.path(),
        dir.path().join("data"),
    ));
    let builder = Arc::new(ParkingBuilder {
        delegate: TestRuntimeBuildPort::from_directory(dir.path().join("profiles"), paths),
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        park: AtomicBool::new(false),
        fail_publish: AtomicBool::new(false),
    });
    let store = runtime::RuntimeSnapshotStore::default();
    let ports = Arc::new(SessionPortResolver::new(store.clone()));
    if confirmed_apply {
        // The endpoint starts with the apply evidence a session normally gets at startup.
        let mut receipt = adopted_baseline();
        receipt.target = super::super::inputs::RuntimeInputs {
            app: application.snapshot_handle().load().state.clone(),
            clash: clash.snapshot_handle().load().state.clone(),
            profiles: Arc::new(profiles.snapshot_handle().load().state.clone()),
            content: super::super::inputs::FrozenProfileContent::default(),
        }
        .target_key();
        store.confirm_applied(Arc::new(receipt));
    }
    let (shutdown, tasks) = (
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
    );
    let notifications = Arc::new(RecordingNotifications::default());
    let revisions = runtime::RuntimeRevisionAllocator::new();
    let content_lock = crate::state::mutation::ProfileContentLock::default();
    let client = ApplicationWorkflowClient::spawn_with_ticks(
        ApplicationWorkflowArgs {
            revisions: revisions.clone(),
            notifications: notifications.clone(),
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            content_lock: content_lock.clone(),
            core: core.clone(),
            service,
            builder: builder.clone(),
            ports: ports.clone(),
            installer: Arc::new(Installer::succeeding()),
            ownership,
            instance_config_dir: Default::default(),
            shutdown: shutdown.clone(),
            tasks: tasks.clone(),
        },
        false,
    )
    .await
    .unwrap();
    let gate: Arc<dyn crate::state::mutation::ports::RuntimeGate> = Arc::new(
        super::super::check::CandidateCheck::new(super::super::check::Args {
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            content_lock: content_lock.clone(),
            builder: builder.clone(),
            ports: ports.clone(),
            revisions: revisions.clone(),
            validator: Arc::new(adapters::CoreCheckValidator::new(&core)),
            workflow: client.clone(),
        }),
    );
    let application = application_owner(application, gate.clone(), &shutdown, &tasks).await;
    let clash = clash_owner(clash, gate, &shutdown, &tasks).await;
    Fixture {
        client,
        endpoint,
        builder,
        application,
        clash,
        store,
        ports,
        app_path,
        scripted,
        shutdown,
        tasks,
        _dir: dir,
    }
}

pub(super) fn overrides(value: serde_json::Value) -> ClashConfig {
    use struct_patch::Patch;
    let mut config = crate::client::test_support::test_clash_config();
    config
        .overrides
        .apply(serde_json::from_value(value).unwrap());
    config
} // Exercise the production domain actor's independent gate before source persistence.
#[tokio::test]
async fn application_actor_rejection_keeps_source_version_and_bytes() {
    use struct_patch::Patch;
    let f = fixture().await;
    let snapshot = f.application.snapshot_handle();
    let before = snapshot.load();
    let bytes = std::fs::read(&f.app_path).ok();
    let application = f.application.clone();
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
async fn application_actor_check_does_not_block_committed_reads() {
    use struct_patch::Patch;
    let f = fixture().await;
    let application = f.application.clone();
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
    assert!(application.snapshot().version.as_ref() > before.version.as_ref());
}

#[tokio::test]
async fn domain_actor_refuses_writes_before_composition_is_ready() {
    use struct_patch::Patch;
    let dir = tempfile::tempdir().unwrap();
    let manager = application_manager(
        temp_path(&dir, "application.yaml"),
        NyanpasuAppConfig::default(),
    )
    .await;
    let application = crate::client::application::ApplicationClient::from_manager(
        crate::state::mutation::MutationCoordinator::pending(),
        manager,
        ReleaseChannel::Stable,
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
    let f = fixture().await;
    f.endpoint.set_failure(Some("queue_full"));
    let id = commit_source(&f.clash, overrides(serde_json::json!({"mode":"direct"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, id).await.conclusion.label(),
        "NotInEffect"
    );
    f
}

#[tokio::test]
async fn runtime_retry_applies_committed_bytes_without_an_independent_check() {
    let f = deferred_fixture().await;
    let checked = f.endpoint.checked();
    let source = f.clash.snapshot().version;
    f.endpoint.set_failure(None);
    f.endpoint
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    let before = f.endpoint.submissions();
    f.client.retry_runtime().await.unwrap();
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Converged");
    assert_eq!(f.endpoint.checked().len(), checked.len());
    assert_eq!(f.endpoint.submissions(), before + 1);
    assert_eq!(
        f.endpoint.reconciled_bytes(),
        [checked[0].config_bytes.clone()]
    );
    assert_eq!(f.clash.snapshot().version, source);
}

#[tokio::test]
async fn explicitly_stopped_runtime_is_not_started_by_retry_now() {
    let f = fixture().await;
    f.client.stop_core().await.unwrap();
    let before = f.endpoint.submissions();
    f.client.retry_runtime().await.unwrap();
    assert_eq!(f.endpoint.submissions(), before);
    assert_eq!(f.client.reconciler_status().conclusion.label(), "Inactive");
}

#[tokio::test]
async fn committed_product_retry_republishes_without_rewriting_source() {
    let f = fixture().await;
    f.builder.fail_publish.store(true, Ordering::SeqCst);
    let version = commit_source(&f.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    settled(&f.client, version).await;
    assert!(f.client.reconciler_status().maintenance.is_some());
    let submitted = f.endpoint.reconciled_bytes().len();
    let version = f.clash.snapshot().version;
    f.builder.fail_publish.store(false, Ordering::SeqCst);
    f.client.retry_runtime().await.unwrap();
    assert!(f.client.reconciler_status().maintenance.is_none());
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted + 1);
    assert_eq!(f.clash.snapshot().version, version);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_engine_is_rejected_before_source_persistence() {
    use std::error::Error as _;

    use nyanpasu_config::{
        profile::ScriptRuntime,
        runtime::{
            executor::{
                PortError, ProfileContentSource, RuntimePipelineError, ScriptExecutionError,
                ScriptOperation, ScriptRunner, StepLogEntry,
            },
            value::ConfigValue,
        },
    };
    use nyanpasu_profile_transform::RuntimeBuildError;
    use nyanpasu_runtime_values::profiles::ProfilesError;
    use struct_patch::Patch as _;

    use crate::{
        client::{ClientError, NyanpasuClient, runtime_build::RuntimeBuildService, test_support},
        state::config_error::ConfigError,
    };

    struct Scripts(Arc<AtomicBool>);
    impl ScriptRunner for Scripts {
        fn run(
            &self,
            runtime: ScriptRuntime,
            _: &str,
            config: &ConfigValue,
            _: &mut Vec<StepLogEntry>,
        ) -> Result<ConfigValue, PortError> {
            if self.0.load(Ordering::SeqCst) {
                Err(ScriptExecutionError::StartEngine {
                    runtime,
                    operation: ScriptOperation::Transform,
                }
                .into())
            } else {
                Ok(config.clone())
            }
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            panic!("builtin transform")
        }
        fn eval_item_expr(&self, _: &str, _: &ConfigValue) -> Result<ConfigValue, PortError> {
            panic!("builtin transform")
        }
    }
    struct Content;
    impl ProfileContentSource for Content {
        fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
            Ok("proxies: []".into())
        }
    }

    for (core, runtime) in [
        (ClashCore::Mihomo, ScriptRuntime::JavaScript),
        (ClashCore::ClashRs, ScriptRuntime::Lua),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let mut args = test_support::test_client_args_with_endpoint(
            &dir,
            endpoint.clone(),
            test_support::profile_test_adapters(&dir),
        )
        .await;
        let unavailable = Arc::new(AtomicBool::new(false));
        args.host_adapters.runtime_build = Arc::new(RuntimeBuildService::new(
            Arc::new(Content),
            Arc::new(test_support::runtime_core_spec),
            args.runtime_paths.clone(),
            {
                let unavailable = unavailable.clone();
                Arc::new(move || Ok(Box::new(Scripts(unavailable.clone()))))
            },
        ));
        let app_path = args.paths.application_config_path();
        let profiles_path = args.paths.profiles_path();
        let shutdown = args.shutdown.clone();
        let tasks = args.tasks.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        endpoint.prime(&client).await;
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.core = Some(core);
        patch.enable_builtin_enhanced = Some(true);
        client.patch_app_config(patch).await.unwrap();
        let uid = client
            .add_profile(
                test_support::minimal_file_profile_request(),
                Some("proxies: []".into()),
            )
            .await
            .unwrap()
            .into_value();
        let before = client.configuration_status().source_versions;
        let app_bytes = std::fs::read(&app_path).ok();
        let profiles_bytes = std::fs::read(&profiles_path).ok();
        let checked = endpoint.checked().len();
        unavailable.store(true, Ordering::SeqCst);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_service_mode = Some(true);
        let config_error = client.patch_app_config(patch).await.unwrap_err();
        let profile_error = client.activate_profile(Some(uid)).await.unwrap_err();
        for (domain, error) in [("config", config_error), ("profiles", profile_error)] {
            let commit = match &error {
                ClientError::Config(ConfigError::Commit { source, .. }) if domain == "config" => {
                    source
                }
                ClientError::Profiles(ProfilesError::Commit { source, .. })
                    if domain == "profiles" =>
                {
                    source
                }
                other => panic!("unexpected {domain} error: {other:?}"),
            };
            let CommitAborted::RuntimeRefused {
                errors,
                runtime: aftermath,
                ..
            } = commit
            else {
                panic!("{commit:?}")
            };
            assert!(matches!(aftermath, RuntimeAftermath::Untouched));
            assert_eq!(errors.len(), 1);
            let RuntimeError::BuildRuntime { source: build } = errors[0].as_ref() else {
                panic!("{errors:?}")
            };
            let RuntimeBuildError::RunPipeline { source: pipeline } = build else {
                panic!("{build:?}")
            };
            assert!(
                matches!(
                    pipeline,
                    RuntimePipelineError::ExecuteScript {
                        source: ScriptExecutionError::StartEngine {
                            runtime: actual,
                            operation: ScriptOperation::Transform,
                        }
                    } if *actual == runtime
                ),
                "{pipeline:?}"
            );
            let source = errors[0]
                .source()
                .unwrap()
                .downcast_ref::<RuntimeBuildError>()
                .unwrap();
            let source = source
                .source()
                .unwrap()
                .downcast_ref::<RuntimePipelineError>()
                .unwrap();
            let source = source
                .source()
                .unwrap()
                .downcast_ref::<ScriptExecutionError>()
                .unwrap();
            assert!(errors[0].to_string().contains(&source.to_string()));
            assert!(
                commit.source().is_some(),
                "coordinator cause must remain available"
            );
            assert_eq!(
                serde_json::to_value(errors[0].as_ref()).unwrap()["source"]["source"]["source"],
                serde_json::json!({
                    "kind": "start_engine", "runtime": runtime, "operation": "transform",
                })
            );
        }
        let after = client.configuration_status().source_versions;
        assert_eq!(
            (after.application, after.clash, after.profiles),
            (before.application, before.clash, before.profiles)
        );
        assert_eq!(std::fs::read(&app_path).ok(), app_bytes);
        assert_eq!(std::fs::read(&profiles_path).ok(), profiles_bytes);
        assert_eq!(
            endpoint.checked().len(),
            checked,
            "failure precedes core validation"
        );
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }
}

// Guards retaining the engine versions read before an apply, rather than re-reading at publication.
#[tokio::test]
async fn a_round_does_not_settle_another_domains_later_commit() {
    use futures_util::FutureExt as _;
    let f = scripted_fixture().await;
    let application = f.application.clone();
    let scripted = f.scripted.as_ref().unwrap();
    let before = f.clash.snapshot_handle().load().version;
    scripted.queue(WaitScript::Held);
    let saved = application
        .patch(NyanpasuAppConfigPatch {
            enable_builtin_enhanced: Some(!application.snapshot().state.enable_builtin_enhanced),
            ..Default::default()
        })
        .await
        .unwrap();
    scripted.held().await;
    f.clash
        .replace(overrides(serde_json::json!({"mode": "direct"})))
        .await
        .unwrap();
    let committed = f.clash.snapshot_handle().load().version;
    scripted.queue(WaitScript::Held);
    let mut waiting = Box::pin(f.client.settled(
        nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
        committed,
    ));
    assert!(waiting.as_mut().now_or_never().is_none());
    scripted.release();
    scripted.held().await;
    let status = f.client.0.reconciler.borrow().clone();
    assert_eq!(
        status.seen.clash, before,
        "the first round did not use the later source commit"
    );
    assert_eq!(status.seen.application, saved.version);
    assert!(waiting.as_mut().now_or_never().is_none());
    scripted.release();
    assert_eq!(waiting.await.seen.clash, committed);
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}

pub(super) async fn application_owner(
    manager: PersistentStateManager<NyanpasuAppConfig, ApplicationFormat>,
    gate: Arc<dyn crate::state::mutation::ports::RuntimeGate>,
    shutdown: &tokio_util::sync::CancellationToken,
    tasks: &tokio_util::task::TaskTracker,
) -> crate::client::application::ApplicationClient {
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
        gate,
    );
    crate::client::application::ApplicationClient::from_manager(
        mutations,
        manager,
        ReleaseChannel::Stable,
        shutdown.child_token(),
        tasks,
    )
    .await
    .unwrap()
}
pub(super) async fn clash_owner(
    manager: PersistentStateManager<
        ClashConfig,
        nyanpasu_migration::modules::clash_config::ClashConfigFormat,
    >,
    gate: Arc<dyn crate::state::mutation::ports::RuntimeGate>,
    shutdown: &tokio_util::sync::CancellationToken,
    tasks: &tokio_util::task::TaskTracker,
) -> crate::client::clash_config::ClashConfigClient {
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations.connect(
        Arc::new(crate::client::effects::ports::NoopCommitNotifications),
        gate,
    );
    crate::client::clash_config::ClashConfigClient::from_manager(
        mutations,
        manager,
        shutdown.child_token(),
        tasks,
    )
    .await
    .unwrap()
}

#[derive(Clone, Copy)]
pub(super) struct CommittedSource {
    domain: nyanpasu_runtime_values::mutation::ConfigDomain,
    version: nyanpasu_state_engine::Version,
    reaches_runtime: bool,
}
#[async_trait::async_trait]
pub(super) trait SourceOwner {
    type Value: Send;
    async fn replace(
        &self,
        next: Self::Value,
    ) -> Result<CommittedSource, crate::state::config_error::ConfigError>;
}
#[async_trait::async_trait]
impl SourceOwner for crate::client::application::ApplicationClient {
    type Value = NyanpasuAppConfig;
    async fn replace(
        &self,
        next: Self::Value,
    ) -> Result<CommittedSource, crate::state::config_error::ConfigError> {
        let saved = self.replace(next).await?;
        Ok(CommittedSource {
            domain: nyanpasu_runtime_values::mutation::ConfigDomain::Application,
            version: saved.version,
            reaches_runtime: saved.reaches_runtime,
        })
    }
}
#[async_trait::async_trait]
impl SourceOwner for crate::client::clash_config::ClashConfigClient {
    type Value = ClashConfig;
    async fn replace(
        &self,
        next: Self::Value,
    ) -> Result<CommittedSource, crate::state::config_error::ConfigError> {
        let saved = self.replace(next).await?;
        Ok(CommittedSource {
            domain: nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
            version: saved.version,
            reaches_runtime: saved.reaches_runtime,
        })
    }
}
pub(super) async fn commit_source<S: SourceOwner>(
    owner: &S,
    next: S::Value,
) -> Result<CommittedSource, crate::state::config_error::ConfigError> {
    owner.replace(next).await
}
pub(super) async fn settled(
    client: &ApplicationWorkflowClient,
    source: CommittedSource,
) -> super::super::ReconcilerStatus {
    if source.reaches_runtime {
        client.settled(source.domain, source.version).await
    } else {
        client.0.reconciler.borrow().clone()
    }
}
pub(super) fn unserviceable_check() -> nyanpasu_core_manager::CoreError {
    nyanpasu_core_manager::CoreError::new(
        CoreErrorKind::BackendUnavailable,
        "scripted check unavailable",
        true,
    )
}

pub(super) fn app_with_core(core: ClashCore) -> NyanpasuAppConfig {
    NyanpasuAppConfig {
        core,
        ..Default::default()
    }
}

#[tokio::test]
async fn a_gui_save_never_reaches_the_runtime() {
    use nyanpasu_config::application::ThemeMode;

    let f = fixture().await;
    f.client.0.actor.kill_and_wait(None).await.unwrap();
    let reads = f.endpoint.status_reads();
    let before = f.application.snapshot();
    let saved = f
        .application
        .patch(NyanpasuAppConfigPatch {
            theme_mode: Some(ThemeMode::Dark),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(saved.version.as_ref() > before.version.as_ref());
    assert_eq!(saved.state.theme_mode, ThemeMode::Dark);
    assert_eq!(saved.operation_id, None);
    assert!(!saved.reaches_runtime);
    assert_eq!(f.endpoint.status_reads(), reads);
    let runtime_saved = f
        .application
        .patch(NyanpasuAppConfigPatch {
            enable_builtin_enhanced: Some(!before.state.enable_builtin_enhanced),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(runtime_saved.reaches_runtime);
    assert!(runtime_saved.version.as_ref() > saved.version.as_ref());
    assert!(matches!(
        f.client
            .settled(
                nyanpasu_runtime_values::mutation::ConfigDomain::Application,
                runtime_saved.version,
            )
            .await
            .conclusion,
        super::super::Conclusion::Closed
    ));
    f.shutdown.cancel();
    f.tasks.close();
    f.tasks.wait().await;
}

#[tokio::test]
async fn a_host_that_does_not_name_its_core_admits_a_critical_mutation() {
    let f = fixture().await;
    f.endpoint
        .set_status(Some(CoreStateDetail::Running { epoch: 1, pid: 7 }), None);
    let source = commit_source(&f.application, app_with_core(ClashCore::ClashRs))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Converged
    ));
    assert_eq!(f.application.snapshot().state.core, ClashCore::ClashRs);
}

#[tokio::test]
async fn a_stopped_core_starts_the_new_checked_target() {
    let f = fixture().await;
    f.endpoint
        .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
    let version = commit_source(&f.application, app_with_core(ClashCore::ClashRs))
        .await
        .unwrap();
    assert_eq!(
        settled(&f.client, version).await.conclusion.label(),
        "Converged"
    );
    assert_eq!(f.application.snapshot().state.core, ClashCore::ClashRs);
    assert_eq!(f.endpoint.reconciled_bytes().len(), 1);
    assert_eq!(f.endpoint.checked().len(), 1);
}

#[tokio::test]
async fn a_recorded_user_stop_saves_without_starting_even_when_the_host_is_quiet() {
    let f = fixture().await;
    f.client.stop_core().await.unwrap();
    f.endpoint.set_status(None, None);
    let submitted = f.endpoint.reconciled_bytes().len();
    let source = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Waiting { .. }
    ));
    assert_eq!(f.endpoint.reconciled_bytes().len(), submitted);
    assert!(!f.client.status().uncertain);
}

#[tokio::test]
async fn an_unobserved_apply_takes_unchanged_ports_away() {
    use nyanpasu_contracts::net::SelfProxyPortSource as _;

    let f = fixture().await;
    let source = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    settled(&f.client, source).await;
    assert!(f.ports.confirmed().is_some());
    f.endpoint.set_result_missing(true);
    let source = commit_source(&f.clash, overrides(serde_json::json!({"mode": "direct"})))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Unknown { .. }
    ));
    assert_eq!(
        f.clash.snapshot().state.overrides.mode(),
        nyanpasu_config::clash::config::overrides::Mode::Direct
    );
    assert!(f.ports.confirmed().is_none());
    assert!(f.ports.mixed_port().is_none());
}

#[tokio::test]
async fn successful_application_keeps_the_promoted_inspection() {
    let f = fixture().await;
    f.endpoint.set_effective_enabled(true);
    let _ = crate::actor_v2::endpoint::ControlEndpoint::effective_config(f.endpoint.as_ref()).await;
    let source = commit_source(&f.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert!(matches!(
        settled(&f.client, source).await.conclusion,
        super::super::Conclusion::Converged
    ));
    let runtime = f.store.read();
    assert!(runtime.applied.as_ref().unwrap().effective.is_some());
    assert!(runtime.pending.is_none());
    assert!(runtime.promoted.as_ref().unwrap().effective.is_some());
}

#[tokio::test]
async fn invalid_overlay_is_rejected_before_source_commit() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let mut args = crate::client::test_support::test_client_args_with_endpoint(
        &dir,
        endpoint.clone(),
        crate::client::test_support::profile_test_adapters(&dir),
    )
    .await;
    let profiles_dir = dir.path().join("profiles");
    std::fs::create_dir_all(&profiles_dir).unwrap();
    std::fs::write(profiles_dir.join("invalid.yaml"), "rules: [").unwrap();
    args.host_adapters.runtime_build = Arc::new(TestRuntimeBuildPort::from_directory(
        profiles_dir,
        args.runtime_paths.clone(),
    ));
    let client = crate::client::NyanpasuClient::try_new(args).await.unwrap();
    let before = client.get_profiles().await.unwrap();
    let mut next = before.as_ref().clone();
    let item = nyanpasu_profile_transform::test_support::overlay("invalid", "invalid.yaml");
    next.global_transforms.push(item.uid.clone());
    next.items.insert(item.uid.clone(), item);
    let error = client.inner.profiles.replace(next).await.unwrap_err();
    assert!(matches!(
        error,
        nyanpasu_runtime_values::profiles::ProfilesError::Commit {
            source: CommitAborted::RuntimeRefused {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        client.get_profiles().await.unwrap().revision(),
        before.revision()
    );
    assert!(endpoint.reconciled_bytes().is_empty());
    client.request_shutdown();
    client.wait_shutdown().await;
}
