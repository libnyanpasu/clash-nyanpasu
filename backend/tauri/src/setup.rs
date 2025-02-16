//! Setup logic for the app

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> Result<(), tauri::Error> {
    let app_handle = app.app_handle().clone();
    #[cfg(target_os = "windows")]
    super::shutdown_hook::setup_shutdown_hook(move || {
        tracing::info!("Shutdown hook triggered, exiting app...");
        app_handle.exit(0);
    })?;

    Ok(())
}
