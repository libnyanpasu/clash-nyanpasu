//! Code bound to the webview engine (WebView2 on Windows). Windows reach the
//! engine only through here, so a different engine is a rewrite of this file.

use tauri::{Manager, Runtime, WebviewWindowBuilder};

#[cfg(windows)]
const WEBVIEW2_BROWSER_ARGS: &str = "--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling";

/// Gives a window builder the engine's browser arguments.
pub fn configure_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(windows)]
    let builder = builder.additional_browser_args(WEBVIEW2_BROWSER_ARGS);

    builder
}

use nyanpasu_diagnose::report::WebviewInfo;

/// The engine, its runtime version and the overrides that change how it runs.
/// Reading the version needs no webview.
pub fn collect_webview_info() -> WebviewInfo {
    #[cfg(windows)]
    let (engine, browser_args, env_names) = (
        "WebView2",
        Some(WEBVIEW2_BROWSER_ARGS),
        &[
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
            "WEBVIEW2_USER_DATA_FOLDER",
        ][..],
    );
    #[cfg(target_os = "linux")]
    let (engine, browser_args, env_names) = (
        "WebKitGTK",
        None,
        &[
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
        ][..],
    );
    #[cfg(target_os = "macos")]
    let (engine, browser_args, env_names) = ("WKWebView", None, &[] as &[&str]);

    let (version, version_error) = match tauri::webview_version() {
        Ok(version) => (Some(version), None),
        Err(error) => (None, Some(error.to_string())),
    };

    WebviewInfo {
        engine: engine.into(),
        version,
        version_error,
        browser_args: browser_args.map(str::to_owned),
        env: env_names
            .iter()
            .filter_map(|name| {
                std::env::var_os(name)
                    .map(|value| ((*name).to_owned(), value.to_string_lossy().into_owned()))
            })
            .collect(),
        tauri_version: tauri::VERSION.into(),
        wry_version: env!("WRY_VERSION").into(),
    }
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
