//! Setup logic for the app
use nyanpasu_paths::PathResolver;
use std::sync::Arc;

use crate::{
    client::{
        ClientSetupArgs, HttpDirectEgressProbe, MainThreadExecutor, NyanpasuClient,
        OsSystemDnsCache, RuntimePaths, StateChanged, TauriMainThread, TauriUiEventSink,
        UiEventSink,
        effects::executor::ApplicationEffectExecutor,
        hotkey::{
            HotkeyArgs, HotkeyClient,
            adapters::{
                ChannelActionSink, PlatformAcceleratorValidator, TauriShortcutRegistrar,
                TauriWindowControl,
            },
            ports::HotkeyAction,
        },
        system_proxy::{
            SystemProxyArgs, SystemProxyClient,
            adapters::{AutoLaunchBackend, AutoLaunchConfig, HttpPacBackend, SysproxyOsProxy},
            ports::OsProxyPort,
        },
        track_until_shutdown,
        ui_effects::{
            adapters::{
                RustI18nLocaleSink, TauriTrayRefresher, TauriWidgetController,
                TracingLoggerRefresher,
            },
            ports::LocaleSink,
        },
    },
    core::{
        storage::{StorageNotification, StorageResyncEvent, StorageValueChangedEvent},
        traffic::{TrafficFileStorage, TrafficStorage},
    },
    utils::init::logging::ReloadSignal,
};
use nyanpasu_tsdb::{Profile, storage::redb::RedbStorage};
use snafu::ResultExt as _;
use tauri_specta::Event;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const RESTART_BUDGET: u8 = 3;

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

/// The composition root retains these owners even when construction stops early.
#[derive(Clone, Default)]
pub(crate) struct StartupOwners {
    shutdown: CancellationToken,
    tasks: TaskTracker,
    core: Option<crate::core::actor_v2::CoreClient>,
}

impl StartupOwners {
    pub(crate) fn request_shutdown(&self) {
        self.shutdown.cancel();
        self.tasks.close();
    }

    pub(crate) async fn wait_shutdown(&self) {
        self.tasks.wait().await;
        if let Some(core) = &self.core {
            core.finish_shutdown().await;
        }
    }
}

/// Keeps the error out of Tauri's panic-producing setup boundary. The failure
/// handler shows the dialog and hands ownership to the event-loop exit path.
pub(crate) fn run_setup(
    mut owners: StartupOwners,
    setup: impl FnOnce(&mut StartupOwners) -> Result<(), crate::startup_failure::StartupFailure>,
    host: &impl StartupFailureHost,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(failure) = setup(&mut owners) {
        tracing::error!(error = %failure.detail, path = ?failure.path, kind = ?failure.kind, "application startup failed");
        host.show(failure);
        host.retain(owners);
        host.exit(1);
    }
    Ok(())
}

/// Presentation and event-loop ownership transfer at the setup boundary.
pub(crate) trait StartupFailureHost {
    fn show(&self, failure: crate::startup_failure::StartupFailure);
    fn retain(&self, owners: StartupOwners);
    fn exit(&self, code: i32);
}

pub(crate) struct TauriStartupFailureHost {
    pub handle: tauri::AppHandle,
    pub paths: PathResolver,
}

impl StartupFailureHost for TauriStartupFailureHost {
    fn show(&self, failure: crate::startup_failure::StartupFailure) {
        use tauri::Manager as _;
        if let Some(logging) = self
            .handle
            .try_state::<crate::utils::init::logging::LoggingShutdown>()
        {
            logging.record_startup_failure(failure.detail.clone());
        }
        crate::startup_failure::show(&crate::startup_failure::Report::new(failure, &self.paths));
    }

    fn retain(&self, owners: StartupOwners) {
        use tauri::Manager as _;
        self.handle.manage(owners);
    }

    fn exit(&self, code: i32) {
        self.handle.exit(code);
    }
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to prepare migrations"))]
    PrepareMigrations {
        source: crate::core::migration::runner::Error,
    },
    #[snafu(display("failed to run migrations"))]
    RunMigrations {
        source: crate::core::migration::runner::Error,
    },
    #[snafu(display("failed to build service ipc"))]
    BuildServiceIpc {
        source: nyanpasu_ipc::client::ClientError,
    },
    #[snafu(display("failed to locate install directory"))]
    LocateInstallDirectory {
        source: nyanpasu_paths::InstallDirError,
    },
    #[snafu(display("failed to build core host"))]
    BuildCoreHost {
        source: crate::core::actor_v2::local_host::Error,
    },
    #[snafu(display("failed to spawn core actor"))]
    SpawnCoreActor { source: ractor::SpawnErr },
    #[snafu(display("failed to spawn service actor"))]
    SpawnServiceActor { source: ractor::SpawnErr },
    #[snafu(display("failed to start jobs"))]
    StartJobs { source: crate::client::jobs::Error },
    #[snafu(display("failed to build auto launch"))]
    BuildAutoLaunch {
        source: crate::client::system_proxy::ports::AutoLaunchError,
    },
    #[snafu(display("failed to build pac"))]
    BuildPac {
        source: crate::client::system_proxy::ports::PacError,
    },
    #[snafu(display("failed to spawn system proxy"))]
    SpawnSystemProxy {
        source: crate::client::system_proxy::Error,
    },
    #[snafu(display("failed to spawn hotkeys"))]
    SpawnHotkeys {
        source: crate::client::hotkey::Error,
    },
    #[snafu(display("failed to open storage"))]
    OpenStorage {
        source: crate::core::storage::StorageOperationError,
    },
    #[snafu(display("failed to build client"))]
    BuildClient { source: crate::client::SetupError },
    #[snafu(display("failed to install widget"))]
    InstallWidget {
        source: crate::client::ui_effects::adapters::InstallError,
    },
}

/// Bound to `tauri::Wry` rather than generic over the runtime: the window
/// helpers the hotkey adapter drives are themselves written against the
/// concrete handle, and this is the only runtime the app is ever built with.
#[tracing::instrument(skip_all)]
pub fn setup<M: tauri::Manager<tauri::Wry>>(
    app: &M,
    bundle_metadata: crate::bundle::BundleMetadata,
    logger_reload: std::sync::mpsc::Sender<ReloadSignal>,
    jobs_capture: nyanpasu_jobs::LogCapture,
    paths: PathResolver,
    owners: &mut StartupOwners,
) -> Result<(), Error> {
    let app_handle = app.app_handle().clone();
    let rpc_events = crate::unified_rpc::EventBus::new();
    crate::unified_rpc::bridge_tauri_events(&app_handle, rpc_events.clone());
    app.manage(rpc_events);
    // Shared with the panic hook, which takes the main thread's work over
    // once the event loop is gone.
    let main_thread_handoff = Arc::new(crate::client::MainThreadHandoff::default());
    app.manage(main_thread_handoff.clone());
    let main_thread: Arc<dyn MainThreadExecutor> = Arc::new(TauriMainThread::new(
        app_handle.clone(),
        main_thread_handoff,
    ));
    // The root of the shutdown. Created here rather than in the client: the
    // system proxy, hotkey and widget owners are built outside it.
    let shutdown = owners.shutdown.clone();
    let tasks = owners.tasks.clone();
    #[cfg(target_os = "windows")]
    {
        let shutdown_handle = app_handle.clone();
        if let Err(error) = super::shutdown_hook::setup_shutdown_hook(move || {
            tracing::info!("Shutdown hook triggered, exiting app...");
            shutdown_handle.exit(0);
        }) {
            tracing::warn!(error = %snafu::Report::from_error(&error), "shutdown hook unavailable; continuing without it");
        }
    }

    // Only Tauri knows where the bundle is. Resources are copied best-effort,
    // so a bundle that cannot be located does not stop the app.
    let resources_dir = crate::utils::init::bundled_resources_dir(app)
        .inspect_err(|error| tracing::error!(%error, "failed to locate the bundled resources"))
        .ok();
    let mut migrations = crate::core::migration::Runner::with_paths(paths.clone(), false)
        .context(PrepareMigrationsSnafu)?;
    migrations.run_pending().context(RunMigrationsSnafu)?;
    crate::log_err!(crate::utils::init::init_resources(
        paths.app_data_dir().as_std_path(),
        resources_dir.as_deref()
    ));
    // For the desktop commands that need a path.
    app.manage(paths.clone());
    let runtime_paths = RuntimePaths::from_resolver(&paths);
    // TODO(ipc-timeout): nyanpasu_ipc::Client sets no request timeout. Remove the
    // outer call deadlines in core/actor_v2 once the upstream client sets one.
    let service_ipc = nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)
        .context(BuildServiceIpcSnafu)?;
    let service_binary = crate::core::service::control::service_binary(
        paths
            .app_install_dir()
            .context(LocateInstallDirectorySnafu)?
            .as_std_path(),
    );
    let span = tracing::info_span!("spawn_core_actors").entered();
    let (core_v2, service, jobs) = tauri::async_runtime::block_on(async {
        let control = crate::core::actor_v2::local_host::build(&paths, &service_binary)
            .await
            .context(BuildCoreHostSnafu)?;
        let local: crate::core::actor_v2::endpoint::EndpointHandle =
            Arc::new(crate::core::actor_v2::endpoint::LocalEndpoint::new(control));
        let adapter = Arc::new(
            crate::core::actor_v2::service_host_adapter::OsServiceHostAdapter::new(
                service_ipc.clone(),
                service_binary,
                paths.clone(),
            ),
        );
        start_runtime_owners(owners, local, adapter, &paths, jobs_capture).await
    })?;
    drop(span);
    // The sink end of the hotkey channel goes into the actor; the receiving end
    // is pumped into the facade once the client exists. See `hotkey_action_pump`.
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    // One instance behind both the system proxy actor and the client's own
    // read of the OS settings.
    let os_proxy: Arc<dyn OsProxyPort> = Arc::new(SysproxyOsProxy);
    let span = tracing::info_span!("spawn_effect_owners").entered();
    let (effects, widget_controller) = build_application_effects(
        &app_handle,
        main_thread.clone(),
        os_proxy.clone(),
        &paths,
        hotkey_tx,
        logger_reload,
        &shutdown,
        &tasks,
    )?;
    drop(span);
    let traffic_store = open_traffic_store(&paths);
    let http_routes = Arc::new(crate::unified_rpc::RpcHttpRoutes::default());
    app.manage(http_routes.clone());
    // Opened after the in-process migrations above, which open the same file.
    let storage = crate::core::storage::Storage::try_new(paths.storage_path().as_std_path())
        .context(OpenStorageSnafu)?;
    app.manage(storage.clone());
    let storage_changes = storage.subscribe();
    let storage_handle = app_handle.clone();
    tauri::async_runtime::spawn(track_until_shutdown(
        &tasks,
        &shutdown,
        crate::core::storage::forward_storage_changes(storage_changes, move |notification| {
            let emitted = match notification {
                StorageNotification::Changed(change) => StorageValueChangedEvent {
                    key: change.key,
                    value: change
                        .value
                        .map(|value| String::from_utf8_lossy(&value).into_owned()),
                    revision: change.revision,
                }
                .emit(&storage_handle),
                StorageNotification::Resync => StorageResyncEvent {}.emit(&storage_handle),
            };
            if let Err(error) = emitted {
                tracing::warn!(%error, "failed to emit a storage notification");
            }
        }),
    ));
    // The core runs with the app data dir as its home, where its geo databases live.
    let geo_index = Arc::new(crate::core::geo::FsCountryIndexSource::new(
        paths.app_data_dir().as_std_path().to_owned(),
        paths.cache_dir().join("geodata").into_std_path_buf(),
    ));
    let span = tracing::info_span!("build_client").entered();
    let ui_sink: Arc<dyn UiEventSink> =
        Arc::new(TauriUiEventSink::<tauri::Wry>::new(app_handle.clone()));
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        bundle_metadata,
        http_frontend: Some(debug_http_frontend(&app_handle)),
        http_routes,
        jobs,
        logging: crate::client::logs::LoggingSetup {
            core: match crate::core::logs::RedbCoreLogStore::open(
                paths.app_logs_dir().join("core").into_std_path_buf(),
            ) {
                Ok(store) => Box::new(store),
                Err(error) => {
                    tracing::warn!(%error, "Core log storage unavailable");
                    Box::new(crate::core::logs::UnavailableCoreLogStore(format!(
                        "{error:#}"
                    )))
                }
            },
            files: Arc::new(nyanpasu_logging::FsLogFiles::new(
                paths.app_logs_dir().into_std_path_buf(),
                "clash-nyanpasu".into(),
            )),
            clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
            service: Arc::new(crate::client::logs::IpcServiceLogs::new(service_ipc)),
            frontend: Arc::new(crate::client::frontend_events::TracingFrontendLogSink),
        },
        core_specs: {
            let paths = paths.clone();
            Arc::new(move |core| crate::core::actor_v2::local_host::core_spec(core, &paths))
        },
        paths: paths.clone(),
        storage,
        runtime_paths: runtime_paths.clone(),
        ui_sink: ui_sink.clone(),
        app_update_backend_factory: Some(Arc::new({
            let app_handle = app_handle.clone();
            move |proxy_port| {
                Arc::new(
                    crate::client::app_update::adapters::TauriAppUpdateBackend::new(
                        app_handle.clone(),
                        proxy_port,
                    ),
                )
            }
        })),
        app_update_event_sink: Some(Arc::new(
            crate::client::app_update::adapters::TauriAppUpdateEventSink::new(app_handle.clone()),
        )),
        core_v2,
        service,
        system_dns: Arc::new(OsSystemDnsCache),
        direct_egress: Arc::new(HttpDirectEgressProbe::dnspod()),
        geo_index,
        os_proxy: os_proxy.clone(),
        binary_installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
        core_versions: Arc::new(crate::utils::core_version::SidecarCoreVersionReader),
        effects,
        window: Arc::new(TauriWindowControl::new(app_handle.clone())),
        accelerators: Arc::new(PlatformAcceleratorValidator),
        traffic_store,
        shutdown: shutdown.clone(),
        tasks: tasks.clone(),
    })
    .context(BuildClientSnafu)?;
    drop(span);
    // The tray menu and the first window render with the process locale, so
    // the configured language replaces the system default before either exists.
    RustI18nLocaleSink.set_locale(client.app_config_snapshot().language);
    // Seeded before anything can build the tray, so the first menu is rendered
    // from the committed configuration rather than from defaults.
    app.manage(crate::core::tray::TrayState::<tauri::Wry>::new(
        client.tray_view(),
    ));
    app.manage(crate::window::WindowManager::new(app_handle.clone()));
    app.manage(crate::window::kinds::TrayMenuWindowController::default());
    forward_actor_events(
        app_handle.clone(),
        client.clone(),
        ui_sink,
        &shutdown,
        &tasks,
    );
    tauri::async_runtime::spawn(track_until_shutdown(
        &tasks,
        &shutdown,
        hotkey_action_pump(hotkey_rx, client.clone()),
    ));
    // The widget needs the client's connection stream and the client needs the
    // widget controller, so the controller is built empty and filled here, in
    // the one place that has both. Its desired configuration arrives with the
    // startup effect reconcile like every other effect.
    let span = tracing::info_span!("setup_widget").entered();
    let widget_manager = tauri::async_runtime::block_on(crate::widget::setup(
        client.subscribe_clash_connections(),
        shutdown.child_token(),
        &tasks,
    ));
    widget_controller
        .install(Arc::new(widget_manager))
        .context(InstallWidgetSnafu)?;
    drop(span);
    app.manage(client);

    Ok(())
}

async fn start_runtime_owners(
    owners: &mut StartupOwners,
    local: crate::core::actor_v2::endpoint::EndpointHandle,
    adapter: Arc<dyn crate::core::actor_v2::service_actor::ServiceHostAdapter>,
    paths: &PathResolver,
    jobs_capture: nyanpasu_jobs::LogCapture,
) -> Result<
    (
        crate::core::actor_v2::CoreClient,
        crate::core::actor_v2::service_actor::ServiceClient,
        nyanpasu_jobs::JobsClient,
    ),
    Error,
> {
    let core = crate::core::actor_v2::CoreClient::spawn(local)
        .await
        .context(SpawnCoreActorSnafu)?;
    owners.core = Some(core.clone());
    let service =
        crate::core::actor_v2::service_actor::ServiceClient::spawn(adapter, RESTART_BUDGET)
            .await
            .context(SpawnServiceActorSnafu)?;
    service.drain_on_shutdown(&owners.tasks, owners.shutdown.child_token());
    let jobs = crate::client::jobs::start(
        paths.jobs_path().into_std_path_buf(),
        jobs_capture,
        owners.shutdown.child_token(),
        &owners.tasks,
    )
    .await
    .context(StartJobsSnafu)?;
    Ok((core, service, jobs))
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum UnifiedRpcError {
    #[snafu(display("failed to build unified RPC"))]
    BuildRpc {
        source: crate::unified_rpc::SetupError,
    },
    #[snafu(display("failed to install HTTP routes"))]
    InstallHttpRoutes {
        source: crate::unified_rpc::SetupError,
    },
    #[snafu(display("unified RPC state was already registered"))]
    RegisterState,
}

/// Registers the unified RPC command table after `resolve_setup` has managed
/// storage and the other Tauri state dependencies.
pub fn setup_unified_rpc<M: tauri::Manager<tauri::Wry>>(app: &M) -> Result<(), UnifiedRpcError> {
    let dependencies = crate::unified_rpc::RpcDependencies {
        client: (*app.state::<NyanpasuClient>()).clone(),
        storage: (*app.state::<crate::core::storage::Storage>()).clone(),
        paths: (*app.state::<PathResolver>()).clone(),
        events: (*app.state::<crate::unified_rpc::EventBus>()).clone(),
    };
    let rpc = crate::unified_rpc::UnifiedRpc::new(dependencies).context(BuildRpcSnafu)?;
    app.state::<Arc<crate::unified_rpc::RpcHttpRoutes>>()
        .install(&rpc)
        .context(InstallHttpRoutesSnafu)?;
    snafu::ensure!(app.manage(rpc), RegisterStateSnafu);
    Ok(())
}

#[derive(Debug, snafu::Snafu)]
enum TrafficStoreError {
    #[snafu(display("failed to create traffic directory {}", path.display()))]
    CreateTrafficDirectory {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[cfg(unix)]
    #[snafu(display("failed to secure traffic directory {}", path.display()))]
    SecureTrafficDirectory {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[cfg(windows)]
    #[snafu(display("failed to harden traffic directory {}", path.display()))]
    HardenTrafficDirectory {
        path: std::path::PathBuf,
        source: nyanpasu_utils::io::atomic_fs::AtomicFsError,
    },
}

/// Recording is optional, so a store that cannot be opened disables it rather
/// than failing the launch. The history holds browsing targets, hence the
/// owner-only directory.
fn open_traffic_store(paths: &PathResolver) -> Option<Arc<dyn TrafficStorage<RedbStorage>>> {
    let dir = paths.app_data_dir().join("traffic").into_std_path_buf();
    let open = || -> Result<TrafficFileStorage, TrafficStoreError> {
        std::fs::create_dir_all(&dir).context(CreateTrafficDirectorySnafu { path: &dir })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .context(SecureTrafficDirectorySnafu { path: &dir })?;
        }
        #[cfg(windows)]
        nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)
            .context(HardenTrafficDirectorySnafu { path: &dir })?;
        let profile = Profile::desktop();
        Ok(TrafficFileStorage::new(
            dir.join("traffic.tsdb"),
            profile.backend_cache,
        ))
    };
    match open() {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::error!(%error, "failed to open the traffic store; recording is disabled");
            None
        }
    }
}

/// Carries pressed shortcuts from the OS callback into the facade.
///
/// Serial on purpose: holding a shortcut down must not put several conflicting
/// config mutations in flight at once.
async fn hotkey_action_pump(
    mut actions: tokio::sync::mpsc::UnboundedReceiver<HotkeyAction>,
    client: NyanpasuClient,
) {
    while let Some(action) = actions.recv().await {
        if let Err(error) = client.dispatch_hotkey_action(action).await {
            tracing::warn!(%error, %action, "hotkey action failed");
        }
    }
}

/// Builds the effect executor and the actors behind it.
///
/// Assembled here rather than inside the client so that `ClientSetupArgs` keeps
/// exposing one effect dependency: the composition root owns the concrete
/// adapters, and a test still injects a single port.
#[allow(clippy::too_many_arguments)]
fn build_application_effects(
    app_handle: &tauri::AppHandle,
    main_thread: Arc<dyn MainThreadExecutor>,
    os_proxy: Arc<dyn OsProxyPort>,
    paths: &PathResolver,
    hotkey_tx: tokio::sync::mpsc::UnboundedSender<HotkeyAction>,
    logger_reload: std::sync::mpsc::Sender<ReloadSignal>,
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
) -> Result<(Arc<ApplicationEffectExecutor>, Arc<TauriWidgetController>), Error> {
    // The AppImage path is read here, at the only place that legitimately has
    // the Tauri environment, and handed to the adapter as a plain value.
    #[cfg(target_os = "linux")]
    let appimage = {
        use tauri::Manager as _;
        app_handle
            .env()
            .appimage
            .and_then(|path| path.into_string().ok())
    };
    #[cfg(not(target_os = "linux"))]
    let appimage = {
        let _ = app_handle;
        None
    };

    let auto_launch = AutoLaunchConfig::resolve(appimage)
        .and_then(AutoLaunchBackend::new)
        .context(BuildAutoLaunchSnafu)?;
    let pac = HttpPacBackend::new(paths.cache_dir().join("pac.js")).context(BuildPacSnafu)?;

    let system_proxy = tauri::async_runtime::block_on(SystemProxyClient::spawn(
        SystemProxyArgs {
            os: os_proxy,
            auto_launch: Arc::new(auto_launch),
            pac: Arc::new(pac),
            schedule_guard_ticks: true,
            shutdown: shutdown.child_token(),
        },
        tasks,
    ))
    .context(SpawnSystemProxySnafu)?;
    let hotkeys = tauri::async_runtime::block_on(HotkeyClient::spawn(
        HotkeyArgs {
            registrar: Arc::new(TauriShortcutRegistrar::new(app_handle.clone(), main_thread)),
            sink: Arc::new(ChannelActionSink::new(hotkey_tx)),
            shutdown: shutdown.child_token(),
        },
        tasks,
    ))
    .context(SpawnHotkeysSnafu)?;

    let widget = Arc::new(TauriWidgetController::new(paths.clone()));
    let executor = Arc::new(ApplicationEffectExecutor::new(
        system_proxy,
        hotkeys,
        Arc::new(PlatformAcceleratorValidator),
        Arc::new(RustI18nLocaleSink),
        Arc::new(TracingLoggerRefresher::new(logger_reload)),
        widget.clone(),
        Arc::new(TauriTrayRefresher::<tauri::Wry>::new(app_handle.clone())),
    ));
    Ok((executor, widget))
}

fn forward_actor_events(
    app_handle: tauri::AppHandle,
    client: NyanpasuClient,
    ui_sink: Arc<dyn UiEventSink>,
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
) {
    let (mut mutations, mut effects, mut sources) = client.subscribe_configuration_changes();
    let configuration_client = client.clone();
    let configuration_handle = app_handle.clone();
    tauri::async_runtime::spawn(track_until_shutdown(tasks, shutdown, async move {
        loop {
            let changed = tokio::select! {
                result = mutations.changed() => result,
                result = effects.changed() => result,
                result = sources.changed() => result,
            };
            if changed.is_err() {
                break;
            }
            let _ =
                crate::ipc::ConfigurationStatusChanged(configuration_client.configuration_status())
                    .emit(&configuration_handle);
        }
    }));
    let mut core_events = client.subscribe_core_events();
    let core_handle = app_handle.clone();
    tauri::async_runtime::spawn(track_until_shutdown(tasks, shutdown, async move {
        loop {
            match core_events.recv().await {
                Ok(status) => {
                    let _ = crate::core::actor_v2::CoreStatusChangedEvent(status.into())
                        .emit(&core_handle);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));

    let mut proxy_changes = client.subscribe_proxy_changes();
    tauri::async_runtime::spawn(track_until_shutdown(tasks, shutdown, async move {
        while proxy_changes.changed().await.is_ok() {
            ui_sink.state_changed(StateChanged::Proxies);
        }
    }));

    let mut service_events = client.subscribe_service_events();
    tauri::async_runtime::spawn(track_until_shutdown(tasks, shutdown, async move {
        while service_events.changed().await.is_ok() {
            let status = service_events.borrow_and_update().clone();
            let _ = crate::core::actor_v2::ServiceStatusChangedEvent(status).emit(&app_handle);
        }
    }));
}

fn debug_http_frontend(app: &tauri::AppHandle) -> crate::server::debug_http::Frontend {
    use crate::server::debug_http::{Frontend, FrontendAssets};
    if !cfg!(feature = "custom-protocol") {
        if let Some(url) = &app.config().build.dev_url {
            return Frontend::Dev(url.clone());
        }
    }
    struct TauriFrontendAssets(tauri::AssetResolver<tauri::Wry>);
    impl FrontendAssets for TauriFrontendAssets {
        fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
            if !self
                .0
                .iter()
                .any(|(key, _)| key.as_ref().trim_start_matches('/') == path)
            {
                return None;
            }
            self.0
                .get(path.to_owned())
                .map(|asset| (asset.mime_type, asset.bytes))
        }
    }
    Frontend::Embedded(Arc::new(TauriFrontendAssets(app.asset_resolver())))
}
