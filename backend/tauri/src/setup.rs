//! Setup logic for the app
use std::sync::Arc;

use crate::{
    bridge::{
        clash::LegacyClashBridge,
        verge::{ConfigLegacyVergeStore, LegacyVergeBridge, LegacyVergeStore},
        window::LegacyWindowBridge,
    },
    client::{
        ClientSetupArgs, LegacyBridgeSet, NyanpasuClient, OsSystemDnsCache, RuntimePaths,
        TauriUiEventSink,
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
        },
    },
    utils::path::PathResolver,
};
use anyhow::Context;
use camino::Utf8PathBuf;
use tauri_specta::Event;

const RESTART_BUDGET: u8 = 3;

/// Bound to `tauri::Wry` rather than generic over the runtime: the window
/// helpers the hotkey adapter drives are themselves written against the
/// concrete handle, and this is the only runtime the app is ever built with.
pub fn setup<M: tauri::Manager<tauri::Wry>>(app: &M) -> Result<(), anyhow::Error> {
    let app_handle = app.app_handle().clone();
    #[cfg(target_os = "windows")]
    {
        let shutdown_handle = app_handle.clone();
        super::shutdown_hook::setup_shutdown_hook(move || {
            tracing::info!("Shutdown hook triggered, exiting app...");
            shutdown_handle.exit(0);
        })
        .context("Failed to setup the shutdown hook")?;
    }

    let paths = PathResolver::from_env().context("Failed to resolve app paths")?;
    let mut migrations = crate::core::migration::Runner::with_paths(paths.clone(), false)
        .context("Failed to setup config migrations")?;
    migrations
        .run_pending()
        .context("Failed to run config migrations before client setup")?;
    let legacy_verge_path = utf8_path(paths.nyanpasu_config_path())?;
    let runtime_paths = RuntimePaths::from_resolver(&paths)?;
    let (core_v2, service) = tauri::async_runtime::block_on(async {
        let control = crate::core::actor_v2::local_host::build(&paths).await?;
        let local: crate::core::actor_v2::endpoint::EndpointHandle =
            Arc::new(crate::core::actor_v2::endpoint::LocalEndpoint::new(control));
        let core = crate::core::actor_v2::CoreClient::spawn(local)
            .await
            .context("Failed to spawn core actor")?;
        let adapter = Arc::new(crate::core::actor_v2::service_host_adapter::OsServiceHostAdapter);
        let service =
            crate::core::actor_v2::service_actor::ServiceClient::spawn(adapter, RESTART_BUDGET)
                .await
                .context("Failed to spawn service actor")?;
        anyhow::Ok((core, service))
    })?;
    // The sink end of the hotkey channel goes into the actor; the receiving end
    // is pumped into the facade once the client exists. See `hotkey_action_pump`.
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    let effects = build_application_effects(&app_handle, &paths, hotkey_tx)?;
    let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
    let legacy_verge_store: Arc<dyn LegacyVergeStore> =
        Arc::new(ConfigLegacyVergeStore::new(legacy_lock.clone()));
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        logging: crate::client::logs::LoggingSetup {
            files: Arc::new(nyanpasu_logging::FsLogFiles::new(
                paths.app_logs_dir(),
                "clash-nyanpasu".into(),
            )),
            clock: Arc::new(nyanpasu_logging::MonotonicClock::default()),
            service: Arc::new(crate::client::logs::IpcServiceLogs::new(
                nyanpasu_ipc::client::Client::new(nyanpasu_ipc::SERVICE_PLACEHOLDER)?,
            )),
        },
        paths,
        runtime_paths: runtime_paths.clone(),
        bridges: LegacyBridgeSet {
            verge: Arc::new(LegacyVergeBridge::with_store(legacy_verge_store.clone())),
            window: Arc::new(LegacyWindowBridge::new(legacy_lock.clone())),
            clash: Arc::new(LegacyClashBridge::new(legacy_lock)),
        },
        ui_sink: Arc::new(TauriUiEventSink::<tauri::Wry>::new(app_handle.clone())),
        core_v2,
        service,
        system_dns: Arc::new(OsSystemDnsCache),
        binary_installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
        effects,
        window: Arc::new(TauriWindowControl::new(app_handle.clone())),
        accelerators: Arc::new(PlatformAcceleratorValidator),
    })
    .context("Failed to setup nyanpasu client")?;
    forward_actor_events(app_handle, client.clone());
    app.manage(LegacyVergeBridge::new(
        client.clone(),
        legacy_verge_path,
        legacy_verge_store,
    ));
    tauri::async_runtime::spawn(hotkey_action_pump(hotkey_rx, client.clone()));
    app.manage(client);

    Ok(())
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
fn build_application_effects(
    app_handle: &tauri::AppHandle,
    paths: &PathResolver,
    hotkey_tx: tokio::sync::mpsc::UnboundedSender<HotkeyAction>,
) -> anyhow::Result<Arc<ApplicationEffectExecutor>> {
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

    let system_proxy = tauri::async_runtime::block_on(SystemProxyClient::spawn(SystemProxyArgs {
        os: Arc::new(SysproxyOsProxy),
        auto_launch: Arc::new(auto_launch),
        pac: Arc::new(pac),
        schedule_guard_ticks: true,
    }))
    .context("Failed to spawn the system proxy actor")?;
    let hotkeys = tauri::async_runtime::block_on(HotkeyClient::spawn(HotkeyArgs {
        registrar: Arc::new(TauriShortcutRegistrar::new(app_handle.clone())),
        sink: Arc::new(ChannelActionSink::new(hotkey_tx)),
    }))
    .context("Failed to spawn the hotkey actor")?;

    Ok(Arc::new(ApplicationEffectExecutor::new(
        system_proxy,
        hotkeys,
        Arc::new(PlatformAcceleratorValidator),
    )))
}

fn forward_actor_events(app_handle: tauri::AppHandle, client: NyanpasuClient) {
    let mut core_events = client.subscribe_core_events();
    let core_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
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
    });

    let mut service_events = client.subscribe_service_events();
    tauri::async_runtime::spawn(async move {
        while service_events.changed().await.is_ok() {
            let status = service_events.borrow_and_update().clone();
            let _ = crate::core::actor_v2::ServiceStatusChangedEvent(status).emit(&app_handle);
        }
    });
}

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}
