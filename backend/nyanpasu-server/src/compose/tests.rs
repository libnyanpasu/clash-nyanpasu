//! Real-client refusal paths with intercepted OS commands and scripted runtime replies.

use std::{
    io,
    path::PathBuf,
    process::{ExitStatus, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use nyanpasu_config::application::{ClashCore, CoreLogSettings, NyanpasuAppConfig};
use nyanpasu_contracts::system_proxy::{
    MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig,
};
use nyanpasu_core::{
    actor_v2::{
        endpoint::{
            CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
        },
        service_actor::{ServiceClient, ServiceEndpointFactory},
    },
    client::{
        NyanpasuClient,
        effects::{executor::ApplicationEffectExecutor, ports::MockLoggerRefresher},
        system_proxy::{SystemProxyArgs, SystemProxyClient},
        test_support::{self, TestControlEndpoint},
    },
};
use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::{api::core::v2::OperationInfo, types::ServiceStatus};
use nyanpasu_os::elevation::runner::CommandRunner;
use nyanpasu_runtime_values::execution_host::ExecutionHost;
use struct_patch::Patch as _;
use tempfile::TempDir;

use super::{Server, ServerSetupArgs, elevation_adapters, logging};

struct Runner {
    elevated: AtomicUsize,
    probes: AtomicUsize,
    stopped: AtomicBool,
    installed_for: PathBuf,
}

fn success() -> ExitStatus {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt as _;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt as _;
    ExitStatus::from_raw(0)
}

#[async_trait::async_trait]
impl CommandRunner for Runner {
    fn runas(&self, _: &mut runas::Command) -> io::Result<ExitStatus> {
        self.elevated.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("intercepted elevated command"))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn command(&self, _: &mut std::process::Command) -> io::Result<ExitStatus> {
        self.elevated.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("intercepted authorization helper"))
    }

    async fn output(&self, command: &mut tokio::process::Command) -> io::Result<Output> {
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            ["status", "--json"]
        );
        self.probes.fetch_add(1, Ordering::SeqCst);
        let status = if self.stopped.load(Ordering::SeqCst) {
            ServiceStatus::Stopped
        } else {
            ServiceStatus::Running
        };
        let server =
            (status == ServiceStatus::Running).then(|| nyanpasu_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: "2.0.3".into(),
                core_infos: nyanpasu_ipc::api::status::CoreInfos {
                    instance_id: None,
                    r#type: None,
                    state: nyanpasu_ipc::api::status::CoreState::Running,
                    state_changed_at: 0,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped {
                        reason: None,
                    }),
                },
                runtime_infos: nyanpasu_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(PathBuf::new()),
                    service_config_dir: std::borrow::Cow::Owned(PathBuf::new()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(self.installed_for.clone()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(PathBuf::new()),
                },
                logs: None,
            });
        Ok(Output {
            status: success(),
            stdout: serde_json::to_vec(&nyanpasu_ipc::types::StatusInfo {
                name: "test-service".into(),
                version: "2.0.3".into(),
                status,
                server,
            })
            .unwrap(),
            stderr: Vec::new(),
        })
    }
}

struct ServiceEndpoint {
    delegate: Arc<TestControlEndpoint>,
    runner: Arc<Runner>,
    stop_daemon_with_core: AtomicBool,
}

#[async_trait::async_trait]
impl ControlEndpoint for ServiceEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        if matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Stop
        ) && self.stop_daemon_with_core.load(Ordering::SeqCst)
        {
            self.runner.stopped.store(true, Ordering::SeqCst);
        }
        self.delegate.submit(submission).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.delegate.wait_operation(id, timeout).await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

struct EndpointFactory(Arc<ServiceEndpoint>);
impl ServiceEndpointFactory for EndpointFactory {
    fn endpoint(&self) -> nyanpasu_core::actor_v2::endpoint::EndpointHandle {
        self.0.clone()
    }
}

struct Fixture {
    client: NyanpasuClient,
    runner: Arc<Runner>,
    local: Arc<TestControlEndpoint>,
    service: Arc<ServiceEndpoint>,
    _dir: TempDir,
}

async fn fixture(running: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let local = TestControlEndpoint::succeeding();
    let mut args = test_support::test_client_args_with_endpoint(
        &dir,
        local.clone(),
        test_support::profile_test_adapters(&dir),
    )
    .await;
    let runner = Arc::new(Runner {
        elevated: AtomicUsize::new(0),
        probes: AtomicUsize::new(0),
        stopped: AtomicBool::new(!running),
        installed_for: args.paths.app_config_dir().as_std_path().to_owned(),
    });
    let adapters = elevation_adapters(
        &args.paths,
        PathBuf::from("intercepted-service"),
        Some(runner.clone()),
    );
    let service_endpoint = Arc::new(ServiceEndpoint {
        delegate: TestControlEndpoint::succeeding(),
        runner: runner.clone(),
        stop_daemon_with_core: AtomicBool::new(false),
    });
    let service = ServiceClient::spawn(
        adapters.service,
        Arc::new(EndpointFactory(service_endpoint.clone())),
        0,
    )
    .await
    .unwrap();
    service.drain_on_shutdown(&args.tasks, args.shutdown.child_token());
    args.service = service;
    args.system_dns = adapters.dns;
    args.binary_installer = adapters.installer;
    args.window = None;
    args.accelerators = None;
    args.app_update_backend_factory = None;
    args.app_update_event_sink = None;
    let mut os = MockOsProxyPort::new();
    os.expect_get().returning(|| {
        Ok(OsProxyConfig {
            enable: false,
            host: "localhost".into(),
            port: 7890,
            bypass: String::new(),
        })
    });
    os.expect_set().returning(|_| Ok(()));
    os.expect_default_bypass().return_const("localhost");
    let mut auto = MockAutoLaunchPort::new();
    auto.expect_is_enabled().returning(|| Ok(false));
    auto.expect_set_enabled().returning(|_| Ok(()));
    let proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: Arc::new(os),
            auto_launch: Arc::new(auto),
            pac: Arc::new(MockPacPort::new()),
            schedule_guard_ticks: false,
            shutdown: args.shutdown.child_token(),
        },
        &args.tasks,
    )
    .await
    .unwrap();
    let mut logger = MockLoggerRefresher::new();
    logger.expect_refresh().times(1..).returning(|_, _| Ok(()));
    args.effects = Arc::new(ApplicationEffectExecutor::new(
        proxy,
        Arc::new(logger),
        None,
    ));
    let client = NyanpasuClient::try_new(args).await.unwrap();
    local.prime(&client).await;
    Fixture {
        client,
        runner,
        local,
        service: service_endpoint,
        _dir: dir,
    }
}

async fn mode(
    client: &NyanpasuClient,
    enabled: bool,
) -> Result<
    nyanpasu_runtime_values::runtime::MutationOutcome<()>,
    Box<nyanpasu_core::client::ClientError>,
> {
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.enable_service_mode = Some(enabled);
    client.patch_app_config(patch).await.map_err(Box::new)
}

async fn finish(f: Fixture) {
    f.client.request_shutdown();
    f.client.wait_shutdown().await;
    assert_eq!(f.runner.elevated.load(Ordering::SeqCst), 0);
    assert!(f.runner.probes.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn server_refuses_elevation_on_config_retry_compensation_and_copy_paths_config_on() {
    let f = fixture(false).await;
    let result = mode(&f.client, true).await;
    assert!(format!("{result:?}").contains("authoriz"), "{result:?}");
    assert!(!f.client.get_app_config().await.unwrap().enable_service_mode);
    finish(f).await;
}

#[tokio::test]
async fn server_refuses_elevation_on_config_retry_compensation_and_copy_paths_config_off() {
    let f = fixture(true).await;
    mode(&f.client, true).await.unwrap();
    let result = mode(&f.client, false).await;
    assert!(format!("{result:?}").contains("authoriz"), "{result:?}");
    assert!(
        matches!(&result, Ok(nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded { degradations, .. })
        if degradations.iter().any(|item| matches!(item.reason, nyanpasu_runtime_values::runtime::DegradationReason::ServiceStopFailed { .. })))
    );
    assert!(!f.client.get_app_config().await.unwrap().enable_service_mode);
    finish(f).await;
}

#[tokio::test]
async fn server_refuses_elevation_on_config_retry_compensation_and_copy_paths_retry() {
    let f = fixture(true).await;
    mode(&f.client, true).await.unwrap();
    mode(&f.client, false).await.unwrap();
    let before = f.runner.probes.load(Ordering::SeqCst);
    f.client.retry_runtime_now().await.unwrap();
    assert!(
        f.runner.probes.load(Ordering::SeqCst) > before,
        "retry must revisit the pending daemon release"
    );
    assert_eq!(f.runner.elevated.load(Ordering::SeqCst), 0);
    finish(f).await;
}

#[tokio::test]
async fn server_refuses_elevation_on_config_retry_compensation_and_copy_paths_compensation() {
    let f = fixture(true).await;
    mode(&f.client, true).await.unwrap();
    f.service
        .stop_daemon_with_core
        .store(true, Ordering::SeqCst);
    f.local
        .set_check_answer(test_support::TestCheckAnswer::Pass);
    // Fail persistence after the handoff, forcing restoration of the service baseline.
    let blocking = f._dir.path().join("application.yaml");
    if blocking.exists() {
        std::fs::remove_file(&blocking).unwrap();
    }
    std::fs::create_dir(&blocking).unwrap();
    let result = mode(&f.client, false).await;
    assert!(
        format!("{result:?}").contains("authoriz"),
        "compensation must reach refusal: {result:?}"
    );
    assert!(format!("{result:?}").contains("MoveHostBack"), "{result:?}");
    assert!(test_support::workflow(&f.client).status().uncertain);
    assert!(f.client.get_app_config().await.unwrap().enable_service_mode);
    finish(f).await;
}

#[derive(Default)]
struct Progress(Mutex<Vec<String>>);
impl nyanpasu_updater::BinaryInstallProgress for Progress {
    fn restarting(&self) {
        self.0.lock().unwrap().push("restarting".into());
    }
    fn finished(&self, error: Option<&str>) {
        self.0
            .lock()
            .unwrap()
            .push(error.unwrap_or("success").into());
    }
}

#[tokio::test]
async fn server_refuses_elevation_on_config_retry_compensation_and_copy_paths_copy_fallback() {
    let f = fixture(false).await;
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let source = staging.path().join("core");
    let destination = staging.path().join("destination");
    std::fs::write(&source, "staged binary").unwrap();
    std::fs::create_dir(&destination).unwrap();
    let error = test_support::workflow(&f.client)
        .replace_binary(nyanpasu_updater::PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source,
            destination,
            staging: staging.clone(),
            companion: None,
            progress: Arc::new(Progress::default()),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            nyanpasu_runtime_values::runtime_error::RuntimeError::InstallCoreBinary {
                source:
                    nyanpasu_contracts::core_install::InstallCoreBinaryError::AuthorizeCoreCopy { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        std::fs::read_to_string(staging.path().join("core")).unwrap(),
        "staged binary"
    );
    finish(f).await;
}

#[tokio::test]
async fn server_refuses_dns_elevation_without_launching_commands() {
    let f = fixture(false).await;
    let result = f.client.flush_system_dns_cache().await;
    assert_eq!(f.runner.elevated.load(Ordering::SeqCst), 0);
    #[cfg(any(windows, target_os = "macos"))]
    assert!(matches!(
        result,
        Err(
            nyanpasu_contracts::system_dns::SystemDnsError::AuthorizeDnsFlush {
                source: nyanpasu_contracts::elevation::ElevationConsentError::AuthorizeElevation {
                    operation: nyanpasu_contracts::elevation::ElevationOperation::DnsFlush
                }
            }
        )
    ));
    #[cfg(not(any(windows, target_os = "macos")))]
    assert!(matches!(
        result,
        Err(nyanpasu_contracts::system_dns::SystemDnsError::Unsupported)
    ));
    finish(f).await;
}

#[tokio::test]
async fn server_composes_without_presentation_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let paths = nyanpasu_paths::PathResolver::with_base_dirs(
        dir.path().join("config").to_str().unwrap().into(),
        dir.path().join("data").to_str().unwrap().into(),
    );
    let server = Server::try_new(ServerSetupArgs {
        paths,
        bundle_metadata: nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
        executable: std::env::current_exe().unwrap(),
        logger: Arc::new(MockLoggerRefresher::new()),
        jobs_capture: nyanpasu_core::client::jobs::capture(),
    })
    .await
    .unwrap();
    assert!(
        !server
            .client()
            .get_app_update_state()
            .await
            .unwrap()
            .supported
    );
    assert!(matches!(
        server
            .client()
            .dispatch_hotkey_action(
                nyanpasu_runtime_values::hotkey::HotkeyAction::OpenOrCloseDashboard
            )
            .await,
        Err(nyanpasu_core::client::ClientError::Window(
            nyanpasu_core::client::hotkey::ports::WindowError::ToggleDashboardUnsupported
        ))
    ));
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.hotkeys = Some(vec![]);
    assert!(matches!(
        server.client().patch_app_config(patch).await,
        Err(nyanpasu_core::client::ClientError::Config(
            nyanpasu_core::state::config_error::ConfigError::ValidateHotkeyCapability
        ))
    ));
    assert!(
        !server
            .client()
            .get_app_config()
            .await
            .unwrap()
            .enable_service_mode
    );
    server.shutdown().await;
}

#[test]
fn server_logging_uses_redb_with_current_unavailable_fallback() {
    use nyanpasu_observers::logs::{
        CoreLogDirection, CoreLogError, CoreLogQuery, CoreLogRecord, CoreLogSource, PreparedCoreLog,
    };
    let dir = tempfile::tempdir().unwrap();
    let paths = nyanpasu_paths::PathResolver::with_base_dirs(
        dir.path().to_str().unwrap().into(),
        dir.path().to_str().unwrap().into(),
    );
    paths.create_base_dirs().unwrap();
    let setup = || {
        logging(
            &paths,
            nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER).unwrap(),
        )
    };
    let mut store = setup().core;
    store.configure(CoreLogSettings::default()).unwrap();
    store
        .append(&[PreparedCoreLog::new(&CoreLogRecord {
            source: CoreLogSource {
                capture: "test".into(),
                instance_id: "test".into(),
                core_kind: None,
            },
            received_at: 1,
            time: None,
            log_type: "info".into(),
            payload: "persistent".into(),
        })
        .unwrap()])
        .unwrap();
    assert!(paths.app_logs_dir().join("core/current.redb").is_file());
    let page = store
        .query(CoreLogQuery {
            direction: CoreLogDirection::Latest,
            cursor: None,
            level: None,
            keyword: String::new(),
            limit: 10,
        })
        .unwrap();
    assert_eq!(page.rows[0].record.payload, "persistent");
    drop(store);
    let blocked = tempfile::tempdir().unwrap();
    let paths = nyanpasu_paths::PathResolver::with_base_dirs(
        blocked.path().to_str().unwrap().into(),
        blocked.path().to_str().unwrap().into(),
    );
    paths.create_base_dirs().unwrap();
    std::fs::write(paths.app_logs_dir().join("core"), "not a directory").unwrap();
    let mut store = logging(
        &paths,
        nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER).unwrap(),
    )
    .core;
    assert!(matches!(store.status(), Err(CoreLogError::Unavailable(_))));
}
