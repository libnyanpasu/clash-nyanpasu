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

/// Configures the webview of a window the first time its frontend reports
/// ready, when the webview certainly exists.
#[cfg(target_os = "windows")]
pub fn on_first_ready(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6;
    use windows_core::Interface;

    let _ = window.with_webview(|webview| unsafe {
        if let Ok(core) = webview.controller().CoreWebView2()
            && let Ok(settings) = core.Settings()
            && let Ok(settings6) = settings.cast::<ICoreWebView2Settings6>()
        {
            let _ = settings6.SetIsSwipeNavigationEnabled(false);
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub fn on_first_ready(_window: &tauri::WebviewWindow) {}
