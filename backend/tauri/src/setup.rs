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
    },
    utils::path::PathResolver,
};
use anyhow::Context;
use camino::Utf8PathBuf;
use tauri_specta::Event;

const RESTART_BUDGET: u8 = 3;

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> Result<(), anyhow::Error> {
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
    let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
    let legacy_verge_store: Arc<dyn LegacyVergeStore> =
        Arc::new(ConfigLegacyVergeStore::new(legacy_lock.clone()));
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        paths,
        runtime_paths: runtime_paths.clone(),
        bridges: LegacyBridgeSet {
            verge: Arc::new(LegacyVergeBridge::with_store(legacy_verge_store.clone())),
            window: Arc::new(LegacyWindowBridge::new(legacy_lock.clone())),
            clash: Arc::new(LegacyClashBridge::new(legacy_lock)),
        },
        ui_sink: Arc::new(TauriUiEventSink::<R>::new(app_handle.clone())),
        core_v2,
        service,
        system_dns: Arc::new(OsSystemDnsCache),
    })
    .context("Failed to setup nyanpasu client")?;
    forward_actor_events(app_handle, client.clone());
    app.manage(LegacyVergeBridge::new(
        client.clone(),
        legacy_verge_path,
        legacy_verge_store,
    ));
    app.manage(client);

    // FIXME: this is a background setup, so be careful use this state in ipc.
    // crate::logging::setup(app).context("Failed to setup logging")?;
    Ok(())
}

fn forward_actor_events<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    client: NyanpasuClient,
) {
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
