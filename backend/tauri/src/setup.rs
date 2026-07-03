//! Setup logic for the app
use crate::utils::path::PathResolver;
use anyhow::Context;

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> Result<(), anyhow::Error> {
    let app_handle = app.app_handle().clone();
    #[cfg(target_os = "windows")]
    super::shutdown_hook::setup_shutdown_hook(move || {
        tracing::info!("Shutdown hook triggered, exiting app...");
        app_handle.exit(0);
    })
    .context("Failed to setup the shutdown hook")?;

    let paths = PathResolver::from_env().context("Failed to resolve app paths")?;
    let mut migrations = crate::core::migration::Runner::with_paths(paths.clone(), false)
        .context("Failed to setup config migrations")?;
    migrations
        .run_pending()
        .context("Failed to run config migrations before client setup")?;
    crate::client::setup(app, paths).context("Failed to setup nyanpasu client")?;

    // FIXME: this is a background setup, so be careful use this state in ipc.
    // crate::logging::setup(app).context("Failed to setup logging")?;
    Ok(())
}
