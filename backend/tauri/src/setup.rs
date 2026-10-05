//! Setup logic for the app
use std::sync::Arc;

use crate::{
    client::{
        ClientSetupArgs, HttpDirectEgressProbe, MainThreadExecutor, NyanpasuClient,
        OsSystemDnsCache, RuntimePaths, TauriMainThread, TauriUiEventSink,
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
    utils::{init::logging::ReloadSignal, path::PathResolver},
};
use anyhow::Context;
use camino::Utf8PathBuf;
use nyanpasu_traffic::{RedbTrafficStore, TrafficStore};
use tauri_specta::Event;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const RESTART_BUDGET: u8 = 3;

/// Bound to `tauri::Wry` rather than generic over the runtime: the window
/// helpers the hotkey adapter drives are themselves written against the
/// concrete handle, and this is the only runtime the app is ever built with.
pub fn setup<M: tauri::Manager<tauri::Wry>>(
    app: &M,
    bundle_metadata: crate::bundle::BundleMetadata,
    logger_reload: std::sync::mpsc::Sender<ReloadSignal>,
    jobs_capture: nyanpasu_jobs::LogCapture,
) -> Result<(), anyhow::Error> {
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
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    #[cfg(target_os = "windows")]
    {
        let shutdown_handle = app_handle.clone();
        super::shutdown_hook::setup_shutdown_hook(move || {
            tracing::info!("Shutdown hook triggered, exiting app...");
            shutdown_handle.exit(0);
        })
        .context("Failed to setup the shutdown hook")?;
    }

    // Only Tauri knows where the bundle is. Resources are copied best-effort,
    // so a bundle that cannot be located does not stop the app.
    let resources_dir = app
        .path()
        .resource_dir()
        .inspect_err(|error| tracing::error!(%error, "failed to locate the bundled resources"))
        .ok()
        .map(|dir| dir.join("resources"));
    let paths = PathResolver::from_env(resources_dir).context("Failed to resolve app paths")?;
    let mut migrations = crate::core::migration::Runner::with_paths(paths.clone(), false)
        .context("Failed to setup config migrations")?;
    migrations
        .run_pending()
        .context("Failed to run config migrations before client setup")?;
    crate::log_err!(crate::utils::init::init_resources(&paths));
    // For commands that need a path, such as the Windows UWP loopback tool.
    app.manage(paths.clone());
    let runtime_paths = RuntimePaths::from_resolver(&paths)?;
    // TODO(ipc-timeout): nyanpasu_ipc::Client sets no request timeout. Remove the
    // outer call deadlines in core/actor_v2 once the upstream client sets one.
    let service_ipc = nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)
        .context("Failed to build the service IPC client")?;
    let service_binary = paths
        .service_binary_path()
        .context("Failed to locate the service binary")?;
    let (core_v2, service) = tauri::async_runtime::block_on(async {
        let control = crate::core::actor_v2::local_host::build(&paths).await?;
        let local: crate::core::actor_v2::endpoint::EndpointHandle =
            Arc::new(crate::core::actor_v2::endpoint::LocalEndpoint::new(control));
        let core = crate::core::actor_v2::CoreClient::spawn(local)
            .await
            .context("Failed to spawn core actor")?;
        let adapter = Arc::new(
            crate::core::actor_v2::service_host_adapter::OsServiceHostAdapter::new(
                service_ipc.clone(),
                service_binary,
            ),
        );
        let service =
            crate::core::actor_v2::service_actor::ServiceClient::spawn(adapter, RESTART_BUDGET)
                .await
                .context("Failed to spawn service actor")?;
        anyhow::Ok((core, service))
    })?;
    // The sink end of the hotkey channel goes into the actor; the receiving end
    // is pumped into the facade once the client exists. See `hotkey_action_pump`.
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    // One instance behind both the system proxy actor and the client's own
    // read of the OS settings.
    let os_proxy: Arc<dyn OsProxyPort> = Arc::new(SysproxyOsProxy);
    let jobs = tauri::async_runtime::block_on(crate::client::jobs::start(
        paths.jobs_path(),
        jobs_capture,
        shutdown.child_token(),
        &tasks,
    ))
    .context("Failed to start jobs owner")?;
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
    let traffic_store = open_traffic_store(&paths);
    let http_routes = Arc::new(crate::unified_rpc::RpcHttpRoutes::default());
    app.manage(http_routes.clone());
    // Opened after the in-process migrations above, which open the same file.
    let storage = crate::core::storage::Storage::try_new(&paths.storage_path())
        .context("Failed to open the storage")?;
    app.manage(storage.clone());
    // The core runs with the app data dir as its home, where its geo databases live.
    let geo_index = Arc::new(crate::core::geo::FsCountryIndexSource::new(
        paths.app_data_dir().to_owned(),
        paths.cache_dir().join("geodata"),
    ));
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        bundle_metadata,
        http_frontend: Some(debug_http_frontend(&app_handle)?),
        http_routes,
        jobs,
        logging: crate::client::logs::LoggingSetup {
            core: match crate::core::logs::RedbCoreLogStore::open(paths.app_logs_dir().join("core"))
            {
                Ok(store) => Box::new(store),
                Err(error) => {
                    tracing::warn!(%error, "Core log storage unavailable");
                    Box::new(crate::core::logs::UnavailableCoreLogStore(format!(
                        "{error:#}"
                    )))
                }
            },
            files: Arc::new(nyanpasu_logging::FsLogFiles::new(
                paths.app_logs_dir(),
                "clash-nyanpasu".into(),
            )),
            clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
            service: Arc::new(crate::client::logs::IpcServiceLogs::new(service_ipc)),
            frontend: Arc::new(crate::client::frontend_events::TracingFrontendLogSink),
        },
        paths,
        storage,
        runtime_paths: runtime_paths.clone(),
        ui_sink: Arc::new(TauriUiEventSink::<tauri::Wry>::new(app_handle.clone())),
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
        effects,
        window: Arc::new(TauriWindowControl::new(app_handle.clone(), main_thread)),
        accelerators: Arc::new(PlatformAcceleratorValidator),
        traffic_store,
        shutdown: shutdown.clone(),
        tasks: tasks.clone(),
    })
    .context("Failed to setup nyanpasu client")?;
    // The tray menu and the first window render with the process locale, so
    // the configured language replaces the system default before either exists.
    RustI18nLocaleSink.set_locale(client.app_config_snapshot().language);
    // Seeded before anything can build the tray, so the first menu is rendered
    // from the committed configuration rather than from defaults.
    app.manage(crate::core::tray::TrayState::<tauri::Wry>::new(
        client.tray_view(),
    ));
    app.manage(crate::window::WindowRegistry::default());
    app.manage(crate::utils::resolve::TrayMenuWindowController::default());
    forward_actor_events(app_handle, client.clone(), &shutdown, &tasks);
    tauri::async_runtime::spawn(track_until_shutdown(
        &tasks,
        &shutdown,
        hotkey_action_pump(hotkey_rx, client.clone()),
    ));
    // The widget needs the client's connection stream and the client needs the
    // widget controller, so the controller is built empty and filled here, in
    // the one place that has both. Its desired configuration arrives with the
    // startup effect reconcile like every other effect.
    let widget_manager = tauri::async_runtime::block_on(crate::widget::setup(
        client.subscribe_clash_connections(),
        shutdown.child_token(),
        &tasks,
    ))
    .context("Failed to setup the network statistic widget")?;
    widget_controller
        .install(Arc::new(widget_manager))
        .context("Failed to install the network statistic widget")?;
    // Picked last, so the server binds it soon after.
    let server_port = port_scanner::request_open_port()
        .context("Failed to find a free port for the internal server")?;
    // Dropped on the cancel rather than drained: an icon request in flight
    // must not hold the shutdown up, and nothing it does needs finishing.
    let server = crate::server::run(server_port, client.clone());
    tauri::async_runtime::spawn(track_until_shutdown(&tasks, &shutdown, async move {
        server.await.expect("failed to start server");
    }));
    app.manage(crate::server::ServerPort(server_port));
    app.manage(client);

    Ok(())
}

/// Registers the unified RPC command table after `resolve_setup` has managed
/// storage and the other Tauri state dependencies.
pub fn setup_unified_rpc<M: tauri::Manager<tauri::Wry>>(app: &M) -> anyhow::Result<()> {
    let dependencies = crate::unified_rpc::RpcDependencies {
        client: (*app.state::<NyanpasuClient>()).clone(),
        storage: (*app.state::<crate::core::storage::Storage>()).clone(),
        events: (*app.state::<crate::unified_rpc::EventBus>()).clone(),
        app_handle: Some(app.app_handle().clone()),
    };
    let rpc = crate::unified_rpc::UnifiedRpc::new(dependencies)?;
    app.state::<Arc<crate::unified_rpc::RpcHttpRoutes>>()
        .install(&rpc)?;
    anyhow::ensure!(app.manage(rpc), "unified RPC state was already registered");
    Ok(())
}

/// Recording is optional, so a store that cannot be opened disables it rather
/// than failing the launch. The history holds browsing targets, hence the
/// owner-only directory.
fn open_traffic_store(paths: &PathResolver) -> Option<Arc<dyn TrafficStore>> {
    let dir = paths.app_data_dir().join("traffic");
    let open = || -> anyhow::Result<RedbTrafficStore> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)?;
        Ok(RedbTrafficStore::open(&dir.join("traffic.redb"))?)
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
) -> anyhow::Result<(Arc<ApplicationEffectExecutor>, Arc<TauriWidgetController>)> {
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
        .context("Failed to resolve the auto-launch registration")?;
    let pac = HttpPacBackend::new(utf8_path(paths.cache_dir().join("pac.js"))?)
        .context("Failed to build the PAC backend")?;

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
    .context("Failed to spawn the system proxy actor")?;
    let hotkeys = tauri::async_runtime::block_on(HotkeyClient::spawn(
        HotkeyArgs {
            registrar: Arc::new(TauriShortcutRegistrar::new(app_handle.clone(), main_thread)),
            sink: Arc::new(ChannelActionSink::new(hotkey_tx)),
            shutdown: shutdown.child_token(),
        },
        tasks,
    ))
    .context("Failed to spawn the hotkey actor")?;

    let widget = Arc::new(TauriWidgetController::default());
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

    let mut service_events = client.subscribe_service_events();
    tauri::async_runtime::spawn(track_until_shutdown(tasks, shutdown, async move {
        while service_events.changed().await.is_ok() {
            let status = service_events.borrow_and_update().clone();
            let _ = crate::core::actor_v2::ServiceStatusChangedEvent(status).emit(&app_handle);
        }
    }));
}

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

fn debug_http_frontend(
    app: &tauri::AppHandle,
) -> anyhow::Result<crate::server::debug_http::Frontend> {
    use crate::server::debug_http::{Frontend, FrontendAssets};
    if !cfg!(feature = "custom-protocol") {
        if let Some(url) = &app.config().build.dev_url {
            return Ok(Frontend::Dev(url.clone()));
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
    Ok(Frontend::Embedded(Arc::new(TauriFrontendAssets(
        app.asset_resolver(),
    ))))
}
