//! Setup logic for the app
use anyhow::Context;

use crate::logging;

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> Result<(), anyhow::Error> {
    let app_handle = app.app_handle().clone();
    #[cfg(target_os = "windows")]
    super::shutdown_hook::setup_shutdown_hook(move || {
        tracing::info!("Shutdown hook triggered, exiting app...");
        app_handle.exit(0);
    })
    .context("Failed to setup the shutdown hook")?;

    logging::setup(app).context("Failed to setup logging")?;
    Ok(())
}
