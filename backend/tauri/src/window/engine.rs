//! Code bound to the webview engine (WebView2 on Windows). Windows reach the
//! engine only through here, so a different engine is a rewrite of this file.

use tauri::{Manager, Runtime, WebviewWindowBuilder};

/// Gives a window builder the engine's browser arguments.
pub fn configure_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(windows)]
    let builder = builder.additional_browser_args("--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling");

    builder
}

/// Configures the webview of a window just built. The webview is created
/// asynchronously, so this waits a moment for it.
#[cfg(target_os = "windows")]
pub fn on_created(app_handle: &tauri::AppHandle, label: String) {
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6;
        use windows_core::Interface;

        // Wait a bit for webview to be ready
        std::thread::sleep(std::time::Duration::from_millis(100));

        if let Some(window) = app_handle.get_webview_window(&label) {
            let _ = window.with_webview(|webview| unsafe {
                if let Ok(core) = webview.controller().CoreWebView2()
                    && let Ok(settings) = core.Settings()
                    && let Ok(settings6) = settings.cast::<ICoreWebView2Settings6>()
                {
                    let _ = settings6.SetIsSwipeNavigationEnabled(false);
                }
            });
        }
    });
}
