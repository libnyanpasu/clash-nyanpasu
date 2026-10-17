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

/// Tells the webview whether anyone can see it. Out of sight, the page is
/// hidden, so it throttles its animations and timers, and its memory is kept
/// low; scripts keep running. WebView2 cannot tell by itself: its controller
/// stays visible while its window is hidden or minimized, and it never
/// restores the normal memory level on its own.
#[cfg(target_os = "windows")]
pub fn set_in_sight(window: &tauri::WebviewWindow, in_sight: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows_core::{BOOL, Interface};

    let level = if in_sight {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    };
    let _ = window.with_webview(move |webview| unsafe {
        let controller = webview.controller();
        let mut visible = BOOL::default();
        // A resize of a window in sight asks again; nothing changes then.
        if controller.IsVisible(&mut visible).is_ok() && visible.as_bool() == in_sight {
            return;
        }
        let _ = controller.SetIsVisible(in_sight);
        if let Ok(core) = controller.CoreWebView2()
            && let Ok(core19) = core.cast::<ICoreWebView2_19>()
        {
            let _ = core19.SetMemoryUsageTargetLevel(level);
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub fn set_in_sight(_window: &tauri::WebviewWindow, _in_sight: bool) {}
