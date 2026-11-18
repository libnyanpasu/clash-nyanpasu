//! The desktop-only `system.window` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::window`.

use nyanpasu_rpc::error::{IpcError, Result};
use tauri::{AppHandle, State, Webview};

use crate::window::{
    WindowManager, kinds,
    kinds::{EditorWindow, EditorWindowType, MainWindow},
};

#[nyanpasu_rpc::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_debug_tray_menu_window(app_handle: AppHandle) -> Result<()> {
    kinds::create_debug_tray_menu_window(&app_handle)?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_editor_window(
    windows: State<'_, WindowManager>,
    window_type: EditorWindowType,
    uid: Option<String>,
) -> Result<()> {
    let (window, params) = EditorWindow::for_type(window_type, uid.as_deref())?;
    windows.open(&window, params)?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn create_main_window(windows: State<'_, WindowManager>) -> Result<()> {
    windows.open(&MainWindow, None)?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub fn quit_application(app_handle: AppHandle) {
    crate::utils::help::quit_application(&app_handle);
}

/// The calling window has rendered. Its label comes from the invoking webview,
/// so a window can only report itself.
#[nyanpasu_rpc::rpc(capabilities(caller_webview))]
#[tauri::command]
#[specta::specta]
pub async fn report_window_ready(
    windows: State<'_, WindowManager>,
    webview: Webview,
) -> Result<()> {
    windows.report_ready(webview.label());
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub fn restart_application(app_handle: tauri::AppHandle) -> Result {
    crate::utils::help::restart_application(&app_handle);
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(window))]
#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(
    windows: State<'_, WindowManager>,
    label: String,
) -> Result<()> {
    windows.save_window_geometry(&label).await?;
    Ok(())
}

/// Panics on purpose, so the crash reporter can be tested. The panic hook
/// exits differently when the main thread panics, hence the choice.
#[nyanpasu_rpc::rpc(capabilities(lifecycle))]
#[tauri::command]
#[specta::specta]
pub async fn simulate_crash(app_handle: AppHandle, main_thread: bool) -> Result<()> {
    if main_thread {
        app_handle
            .run_on_main_thread(|| panic!("simulated crash on the main thread"))
            .map_err(IpcError::unknown)?;
    } else {
        std::thread::spawn(|| panic!("simulated crash on a background thread"));
    }
    Ok(())
}
