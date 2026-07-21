//! Setup logic for the app
use std::sync::Arc;

use crate::{
    bridge::{
        clash::LegacyClashBridge,
        verge::{ConfigLegacyVergeStore, LegacyVergeBridge, LegacyVergeStore},
        window::LegacyWindowBridge,
    },
    client::{
        ClientSetupArgs, LegacyBridgeSet, LegacyCoreBridge, LegacyRunningConfigPatchBridge,
        NyanpasuClient, OsSystemDnsCache, RuntimePaths, TauriUiEventSink,
    },
    utils::path::PathResolver,
};
use anyhow::Context;
use camino::Utf8PathBuf;

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
        ui_sink: Arc::new(TauriUiEventSink::<R>::new(app_handle)),
        core: Arc::new(LegacyCoreBridge::new(runtime_paths)),
        clash_patch: Some(Arc::new(LegacyRunningConfigPatchBridge)),
        system_dns: Arc::new(OsSystemDnsCache),
    })
    .context("Failed to setup nyanpasu client")?;
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

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}
