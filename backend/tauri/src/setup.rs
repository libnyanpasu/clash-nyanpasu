//! Setup logic for the app
use std::sync::Arc;

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ClientSetupArgs, LegacyBridgeSet, LegacyCoreBridge, NyanpasuClient, OsSystemDnsCache,
        TauriUiEventSink,
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
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        paths,
        bridges: LegacyBridgeSet {
            verge: Arc::new(LegacyVergeBridge::default()),
            window: Arc::new(LegacyWindowBridge),
            clash: Arc::new(LegacyClashBridge),
        },
        ui_sink: Arc::new(TauriUiEventSink::<R>::new(app_handle)),
        core: Arc::new(LegacyCoreBridge),
        system_dns: Arc::new(OsSystemDnsCache),
    })
    .context("Failed to setup nyanpasu client")?;
    app.manage(LegacyVergeBridge::new(client.clone(), legacy_verge_path));
    app.manage(client);

    // FIXME: this is a background setup, so be careful use this state in ipc.
    // crate::logging::setup(app).context("Failed to setup logging")?;
    Ok(())
}

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}
