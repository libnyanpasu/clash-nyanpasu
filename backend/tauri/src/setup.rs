//! Setup logic for the app
use nyanpasu_core::{
    diagnostics::direct_egress::HttpDirectEgressProbe,
    effects::executor::ApplicationEffectExecutor,
    hotkey::HotkeyAction,
    logs::logging::{ReloadSignal, TracingLoggerRefresher},
    tasks::track_until_shutdown,
};
use nyanpasu_paths::PathResolver;
use std::sync::Arc;

use crate::client::{
    ClientSetupArgs, ClientSetupOutput, MainThreadExecutor, NyanpasuClient, RuntimePaths,
    TauriMainThread, TauriUiEventSink,
    effects::{
        presentation::{TauriEffectInvalidationSink, TauriPresentationEffects},
        tray_view,
    },
    hotkey::{
        HotkeyArgs, HotkeyClient,
        adapters::{
            ChannelActionSink, PlatformAcceleratorValidator, TauriShortcutRegistrar,
            TauriWindowControl,
        },
        dispatch_hotkey_action,
        ports::WindowControl,
    },
    system_proxy_adapters::{AutoLaunchBackend, AutoLaunchConfig},
    ui_effects::{
        adapters::{RustI18nLocaleSink, TauriTrayRefresher, TauriWidgetController},
        ports::LocaleSink,
    },
};
use anyhow::Context;
use nyanpasu_core::{
    system_dns::OsSystemDnsCache,
    system_proxy::{
        SystemProxyArgs, SystemProxyClient,
        adapters::{HttpPacBackend, SysproxyOsProxy},
        ports::OsProxyPort,
    },
};
use nyanpasu_traffic::{RedbTrafficStore, TrafficStore};
use tauri_specta::Event;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const RESTART_BUDGET: u8 = 3;

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
    app.manage(shutdown.clone());
    app.manage(tasks.clone());
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
    let resources_dir = crate::utils::init::bundled_resources_dir(app)
        .inspect_err(|error| tracing::error!(%error, "failed to locate the bundled resources"))
        .ok();
    let mut migrations = nyanpasu_core::migration::Runner::with_paths(
        paths.clone(),
        false,
        crate::consts::BUILD_INFO.pkg_version,
    )
    .context("Failed to setup config migrations")?;
    migrations
        .run_pending()
        .context("Failed to run config migrations before client setup")?;
    crate::log_err!(crate::utils::init::init_resources(
        paths.app_data_dir().as_std_path(),
        resources_dir.as_deref()
    ));
    // For the desktop commands that need a path.
    app.manage(paths.clone());
    let runtime_paths = RuntimePaths::from_resolver(&paths);
    // The service client uses real socket/pipe IPC without an explicit request timeout.
    // Preserve CoreClient/ServiceClient caller budgets and endpoint/OS bounds:
    // they also cover mailbox residence and potentially outstanding external work.
    let service_ipc = nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)
        .context("Failed to build the service IPC client")?;
    let service_binary = nyanpasu_core::service::control::service_binary(
        paths
            .app_install_dir()
            .context("Failed to locate the service binary")?
            .as_std_path(),
    );
    let span = tracing::info_span!("spawn_core_actors").entered();
    let (core_v2, service) = tauri::async_runtime::block_on(async {
        let control = nyanpasu_core::control::local_host::build(&paths).await?;
        let local: nyanpasu_core::control::endpoint::EndpointHandle = Arc::new(
            nyanpasu_core::control::endpoint::LocalEndpoint::new(control),
        );
        let core = nyanpasu_core::control::CoreClient::spawn(local)
            .await
            .context("Failed to spawn core actor")?;
        let adapter = Arc::new(nyanpasu_core::service::os::OsServiceHostAdapter::new(
            service_ipc.clone(),
            service_binary,
            paths.clone(),
        ));
        let service = nyanpasu_core::service::actor::ServiceClient::spawn(adapter, RESTART_BUDGET)
            .await
            .context("Failed to spawn service actor")?;
        anyhow::Ok((core, service))
    })?;
    drop(span);
    // The sink end of the hotkey channel goes into the actor; the receiving end
    // is pumped into the facade once the client exists. See `hotkey_action_pump`.
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    // One instance behind both the system proxy actor and the client's own
    // read of the OS settings.
    let os_proxy: Arc<dyn OsProxyPort> = Arc::new(SysproxyOsProxy);
    let span = tracing::info_span!("spawn_effect_owners").entered();
    let jobs = tauri::async_runtime::block_on(crate::client::jobs::start(
        paths.jobs_path().into_std_path_buf(),
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
    drop(span);
    let traffic_store = open_traffic_store(&paths);
    let http_routes = Arc::new(crate::unified_rpc::RpcHttpRoutes::default());
    app.manage(http_routes.clone());
    let debug_http = tauri::async_runtime::block_on(
        crate::server::debug_http::HttpServerClient::spawn_tracked(
            Some(debug_http_frontend(&app_handle)?),
            http_routes,
            shutdown.clone(),
            &tasks,
        ),
    )?;
    app.manage(debug_http);
    let window: Arc<dyn WindowControl> = Arc::new(TauriWindowControl::new(app_handle.clone()));
    app.manage(window.clone());
    // Opened after the in-process migrations above, which open the same file.
    let storage = nyanpasu_core::storage::Storage::try_new(paths.storage_path().as_std_path())
        .context("Failed to open the storage")?;
    app.manage(storage.clone());
    // The core runs with the app data dir as its home, where its geo databases live.
    let geo_index = Arc::new(nyanpasu_core::geo::FsCountryIndexSource::new(
        paths.app_data_dir().as_std_path().to_owned(),
        paths.cache_dir().join("geodata").into_std_path_buf(),
    ));
    let span = tracing::info_span!("build_client").entered();
    let ClientSetupOutput {
        client,
        mut application_settings,
        self_proxy_port,
    } = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        installed_channel: bundle_metadata.release_channel,
        is_portable: bundle_metadata.is_portable,
        environment: Arc::new(nyanpasu_core::diagnostics::os::OsEnvironmentCollector::new(
            crate::consts::BUILD_INFO.clone(),
            paths.clone(),
        )),
        device_info: Arc::new(nyanpasu_core::device::OsDeviceInfoSource::new()),
        jobs,
        logging: nyanpasu_core::logs::app::LoggingSetup {
            core: match nyanpasu_core::logs::RedbCoreLogStore::open(
                paths.app_logs_dir().join("core").into_std_path_buf(),
            ) {
                Ok(store) => Box::new(store),
                Err(error) => {
                    tracing::warn!(%error, "Core log storage unavailable");
                    Box::new(nyanpasu_core::logs::UnavailableCoreLogStore(format!(
                        "{error:#}"
                    )))
                }
            },
            files: Arc::new(nyanpasu_logging::FsLogFiles::new(
                paths.app_logs_dir().into_std_path_buf(),
                "clash-nyanpasu".into(),
            )),
            clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
            service: Arc::new(nyanpasu_core::logs::app::IpcServiceLogs::new(service_ipc)),
            frontend: Arc::new(nyanpasu_core::logs::frontend::TracingFrontendLogSink),
        },
        core_specs: {
            let paths = paths.clone();
            Arc::new(move |core| nyanpasu_core::control::local_host::core_spec(core, &paths))
        },
        paths: paths.clone(),
        storage,
        runtime_paths: runtime_paths.clone(),
        invalidation: Some(Arc::new(TauriEffectInvalidationSink::new(Arc::new(
            TauriUiEventSink::<tauri::Wry>::new(app_handle.clone()),
        )))),
        core_v2,
        service,
        system_dns: Arc::new(OsSystemDnsCache),
        direct_egress: Arc::new(HttpDirectEgressProbe::dnspod()),
        geo_index,
        os_proxy: os_proxy.clone(),
        binary_installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
        core_versions: Arc::new(crate::utils::core_version::TauriCoreVersionReader::new(
            app_handle.clone(),
        )),
        effects,
        accelerators: Arc::new(PlatformAcceleratorValidator),
        traffic_store,
        shutdown: shutdown.clone(),
        tasks: tasks.clone(),
    })
    .context("Failed to setup nyanpasu client")?;
    drop(span);
    let installed_channel = bundle_metadata.release_channel;
    let app_config = client.app_config_snapshot();
    let app_update_settings = crate::client::app_update::AppUpdateSettings {
        channel: app_config.release_channel.unwrap_or(installed_channel),
        sources: app_config.update_sources.clone(),
        auto_check: app_config.enable_auto_check_update,
        auto_download: app_config.enable_auto_download_update,
    };
    let app_update_supported = !bundle_metadata.is_portable
        && (cfg!(any(target_os = "windows", target_os = "macos"))
            || (cfg!(target_os = "linux") && *crate::consts::IS_APPIMAGE));
    let app_updater =
        tauri::async_runtime::block_on(crate::client::app_update::AppUpdateClient::spawn(
            crate::client::app_update::AppUpdateArgs {
                backend: Arc::new(
                    crate::client::app_update::adapters::TauriAppUpdateBackend::new(
                        app_handle.clone(),
                        self_proxy_port,
                    ),
                ),
                events: Arc::new(
                    crate::client::app_update::adapters::TauriAppUpdateEventSink::new(
                        app_handle.clone(),
                    ),
                ),
                settings: app_update_settings,
                supported: app_update_supported,
                endpoints: crate::bundle::update_endpoints(
                    app_config.release_channel.unwrap_or(installed_channel),
                ),
                shutdown: shutdown.child_token(),
            },
            &tasks,
        ))?;
    let settings_updater = app_updater.clone();
    let settings_shutdown = shutdown.child_token();
    tauri::async_runtime::block_on(async {
        tasks.spawn(async move {
            loop {
                tokio::select! {
                    () = settings_shutdown.cancelled() => break,
                    changed = application_settings.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let config = application_settings.borrow_and_update().clone();
                        settings_updater.configure(crate::client::app_update::AppUpdateSettings {
                            channel: config.release_channel.unwrap_or(installed_channel),
                            sources: config.update_sources,
                            auto_check: config.enable_auto_check_update,
                            auto_download: config.enable_auto_download_update,
                        });
                    }
                }
            }
        });
    });
    app.manage(app_updater);
    // The tray menu and the first window render with the process locale, so
    // the configured language replaces the system default before either exists.
    RustI18nLocaleSink.set_locale(client.app_config_snapshot().language);
    // Seeded before anything can build the tray, so the first menu is rendered
    // from the committed configuration rather than from defaults.
    app.manage(crate::core::tray::TrayState::<tauri::Wry>::new(
        tauri::async_runtime::block_on(tray_view(&client))?,
    ));
    app.manage(crate::window::WindowManager::new(app_handle.clone()));
    app.manage(crate::window::kinds::TrayMenuWindowController::default());
    forward_actor_events(app_handle.clone(), client.clone(), &shutdown, &tasks);
    tauri::async_runtime::spawn(track_until_shutdown(
        &tasks,
        &shutdown,
        hotkey_action_pump(hotkey_rx, client.clone(), window),
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
        paths,
    ))
    .context("Failed to setup the network statistic widget")?;
    widget_controller
        .install(Arc::new(widget_manager))
        .context("Failed to install the network statistic widget")?;
    drop(span);
    app.manage(client);

    Ok(())
}

/// Registers the unified RPC command table after `resolve_setup` has managed
/// storage and the other Tauri state dependencies.
pub fn setup_unified_rpc<M: tauri::Manager<tauri::Wry>>(app: &M) -> anyhow::Result<()> {
    let dependencies = crate::unified_rpc::RpcDependencies {
        client: (*app.state::<NyanpasuClient>()).clone(),
        debug_http: (*app.state::<crate::server::debug_http::HttpServerClient>()).clone(),
        storage: (*app.state::<nyanpasu_core::storage::Storage>()).clone(),
        paths: (*app.state::<PathResolver>()).clone(),
        events: (*app.state::<crate::unified_rpc::EventBus>()).clone(),
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
    let dir = paths.app_data_dir().join("traffic").into_std_path_buf();
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
    window: Arc<dyn WindowControl>,
) {
    while let Some(action) = actions.recv().await {
        if let Err(error) = dispatch_hotkey_action(&client, window.as_ref(), action).await {
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
    let pac = HttpPacBackend::new(paths.cache_dir().join("pac.js"))
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
        Arc::new(TracingLoggerRefresher::new(logger_reload)),
        Some(Arc::new(TauriPresentationEffects::new(
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            Arc::new(RustI18nLocaleSink),
            widget.clone(),
            Arc::new(TauriTrayRefresher::<tauri::Wry>::new(app_handle.clone())),
        ))),
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
                    let _ = crate::core::status_events::CoreStatusChangedEvent(status.into())
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
            let _ = crate::core::status_events::ServiceStatusChangedEvent(status).emit(&app_handle);
        }
    }));
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
