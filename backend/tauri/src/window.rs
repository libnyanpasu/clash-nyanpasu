//! Tauri window management mod
//!
//! This module provides a flexible window management system that supports:
//! - URL parameters for windows
//! - Multiple instances of the same window type (e.g., main, main-1, main-2)
//! - Inter-window communication
//! - Configurable window properties (singleton, visibility, size, etc.)

use crate::{
    config::{Config, nyanpasu::WindowState},
    log_err, trace_err,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU16, Ordering},
    },
};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// Global counter for tracking open windows
static OPEN_WINDOWS_COUNTER: AtomicU16 = AtomicU16::new(0);

/// Global window manager instance
static WINDOW_MANAGER: OnceLock<Mutex<WindowManager>> = OnceLock::new();
/// Window configuration options
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Whether only one instance of this window type is allowed
    pub singleton: bool,
    /// Whether the window should be visible when created
    pub visible_on_create: bool,
    /// Default window size (width, height)
    pub default_size: (f64, f64),
    /// Minimum window size (width, height)
    pub min_size: Option<(f64, f64)>,
    /// Maximum window size (width, height)
    pub max_size: Option<(f64, f64)>,
    /// Whether to center the window on creation
    pub center: bool,
    /// Whether the window is resizable
    pub resizable: bool,
    /// Whether the window should always be on top (None = use global config)
    pub always_on_top: Option<bool>,
    /// Whether to use decorations (None = use platform default)
    pub decorations: Option<bool>,
    /// Whether the window is transparent (None = use platform default)
    pub transparent: Option<bool>,
    /// Whether to skip taskbar
    pub skip_taskbar: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            singleton: true,
            visible_on_create: true,
            default_size: (800.0, 636.0),
            min_size: Some((400.0, 600.0)),
            max_size: None,
            center: true,
            resizable: true,
            always_on_top: None,
            decorations: None,
            transparent: None,
            skip_taskbar: false,
        }
    }
}

impl WindowConfig {
    /// Create a new WindowConfig with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether only one instance is allowed
    pub fn singleton(mut self, singleton: bool) -> Self {
        self.singleton = singleton;
        self
    }

    /// Set whether window is visible on creation
    pub fn visible_on_create(mut self, visible: bool) -> Self {
        self.visible_on_create = visible;
        self
    }

    /// Set default window size
    pub fn default_size(mut self, width: f64, height: f64) -> Self {
        self.default_size = (width, height);
        self
    }

    /// Set minimum window size
    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// Set maximum window size
    pub fn max_size(mut self, width: f64, height: f64) -> Self {
        self.max_size = Some((width, height));
        self
    }

    /// Set whether to center the window
    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }

    /// Set whether the window is resizable
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set always on top
    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = Some(always_on_top);
        self
    }

    /// Set whether to skip taskbar
    pub fn skip_taskbar(mut self, skip: bool) -> Self {
        self.skip_taskbar = skip;
        self
    }
}

/// Window URL parameters
pub type WindowParams = HashMap<String, String>;

/// Builder for constructing URL parameters
#[derive(Debug, Clone, Default)]
pub struct WindowParamsBuilder {
    params: WindowParams,
}

impl WindowParamsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a string parameter
    pub fn param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Add a parameter if condition is true
    pub fn param_if(
        self,
        condition: bool,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        if condition {
            self.param(key, value)
        } else {
            self
        }
    }

    /// Add an optional parameter
    pub fn param_opt(self, key: impl Into<String>, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.param(key, v),
            None => self,
        }
    }

    /// Build the parameters
    pub fn build(self) -> Option<WindowParams> {
        if self.params.is_empty() {
            None
        } else {
            Some(self.params)
        }
    }
}

/// Build URL with optional parameters
pub fn build_url_with_params(base_url: &str, params: Option<&WindowParams>) -> String {
    match params {
        Some(params) if !params.is_empty() => {
            let query: Vec<String> = params
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect();
            format!("{}?{}", base_url, query.join("&"))
        }
        _ => base_url.to_string(),
    }
}
/// Window manager for tracking window instances
#[derive(Debug, Default)]
pub struct WindowManager {
    /// Maps base label to list of instance labels
    instances: HashMap<String, Vec<String>>,
}

impl WindowManager {
    /// Get global window manager instance
    pub fn global() -> &'static Mutex<Self> {
        WINDOW_MANAGER.get_or_init(|| Mutex::new(Self::default()))
    }

    /// Generate a unique label for a window
    ///
    /// For singleton windows, returns None if an instance already exists.
    /// For non-singleton windows, generates labels like: base, base-1, base-2, etc.
    pub fn generate_label(&mut self, base_label: &str, singleton: bool) -> Option<String> {
        let instances = self.instances.entry(base_label.to_string()).or_default();

        if singleton && !instances.is_empty() {
            return None; // Singleton window already exists
        }

        if instances.is_empty() {
            instances.push(base_label.to_string());
            return Some(base_label.to_string());
        }

        // Find the next available number
        let mut next_num = 1;
        loop {
            let label = format!("{}-{}", base_label, next_num);
            if !instances.contains(&label) {
                instances.push(label.clone());
                return Some(label);
            }
            next_num += 1;
        }
    }

    /// Remove a window instance
    pub fn remove_instance(&mut self, label: &str) {
        for instances in self.instances.values_mut() {
            instances.retain(|l| l != label);
        }
    }

    /// Get all instances for a base label
    pub fn get_instances(&self, base_label: &str) -> Vec<String> {
        self.instances.get(base_label).cloned().unwrap_or_default()
    }

    /// Check if a specific label exists
    pub fn has_instance(&self, label: &str) -> bool {
        self.instances
            .values()
            .any(|instances| instances.contains(&label.to_string()))
    }

    /// Get the count of instances for a base label
    pub fn instance_count(&self, base_label: &str) -> usize {
        self.instances.get(base_label).map(|v| v.len()).unwrap_or(0)
    }
}
/// Message for inter-window communication
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct WindowMessageEvent {
    /// Source window label
    pub from: String,
    /// Target window label (use "*" for broadcast)
    pub to: String,
    /// Message type/event name
    pub event: String,
    /// Message payload
    pub payload: serde_json::Value,
}

impl WindowMessageEvent {
    /// Create a new window message
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        event: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            event: event.into(),
            payload,
        }
    }

    /// Create a broadcast message to all windows
    pub fn broadcast(
        from: impl Into<String>,
        event: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self::new(from, "*", event, payload)
    }
}

/// Send a message to a specific window
pub fn send_message_to_window(app_handle: &AppHandle, message: WindowMessageEvent) -> Result<()> {
    // Verify window exists
    let _ = app_handle
        .get_webview_window(&message.to)
        .ok_or_else(|| anyhow::anyhow!("Window '{}' not found", message.to))?;

    let target = message.to.clone();
    message.emit_to(app_handle, target)?;
    Ok(())
}

/// Send a message to all instances of a window type
pub fn broadcast_to_window_type(
    app_handle: &AppHandle,
    base_label: &str,
    from: &str,
    event: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let instances = {
        let manager = WindowManager::global().lock().unwrap();
        manager.get_instances(base_label)
    };

    for label in instances {
        if app_handle.get_webview_window(&label).is_some() {
            let message = WindowMessageEvent::new(from, &label, event, payload.clone());
            trace_err!(
                message.emit_to(app_handle, &label),
                "failed to emit message"
            );
        }
    }
    Ok(())
}

/// Broadcast a message to all open windows
pub fn broadcast_to_all_windows(
    app_handle: &AppHandle,
    from: &str,
    event: &str,
    payload: serde_json::Value,
) -> Result<()> {
    WindowMessageEvent::broadcast(from, event, payload).emit(app_handle)?;
    Ok(())
}

/// Result of window creation
#[derive(Debug, Clone)]
pub struct WindowCreateResult {
    /// The actual label of the created window
    pub label: String,
    /// Whether this was a newly created window or an existing one was shown
    pub is_new: bool,
}

impl WindowCreateResult {
    fn new(label: String) -> Self {
        Self {
            label,
            is_new: true,
        }
    }

    fn existing(label: String) -> Self {
        Self {
            label,
            is_new: false,
        }
    }
}

/// Trait for window management
pub trait AppWindow {
    /// Get window base label (e.g., "main", "editor")
    fn label(&self) -> &str;

    /// Get window title
    fn title(&self) -> &str;

    /// Get window URL path
    fn url(&self) -> &str;

    /// Get window configuration
    fn config(&self) -> WindowConfig {
        WindowConfig::default()
    }

    /// Get window state from config
    fn get_window_state(&self) -> Option<WindowState>;

    /// Set window state to config
    fn set_window_state(&self, state: Option<WindowState>);

    fn reset_window_open_counter(&self) {
        OPEN_WINDOWS_COUNTER.fetch_sub(1, Ordering::Release);
    }

    /// Create window with optional URL parameters
    ///
    /// Returns the label of the created (or existing) window
    fn create_with_params(
        &self,
        app_handle: &AppHandle,
        params: Option<WindowParams>,
    ) -> Result<WindowCreateResult> {
        let config = self.config();
        let base_label = self.label();

        // Clean up stale window records before generating label
        // This handles cases where the window was destroyed but the record wasn't cleaned up
        {
            let mut manager = WindowManager::global().lock().unwrap();
            let stale_labels: Vec<String> = manager
                .get_instances(base_label)
                .into_iter()
                .filter(|label| app_handle.get_webview_window(label).is_none())
                .collect();
            for label in stale_labels {
                tracing::debug!("cleaning up stale window record: {}", label);
                manager.remove_instance(&label);
            }
        }

        // Generate unique label
        let label = {
            let mut manager = WindowManager::global().lock().unwrap();
            // After cleanup above, generate_label should work correctly
            // For singleton windows, if it returns None, the window truly exists
            manager
                .generate_label(base_label, config.singleton)
                .unwrap_or_else(|| {
                    // Singleton window already exists - try to show it
                    if let Some(window) = app_handle.get_webview_window(base_label) {
                        tracing::debug!("{} window is already opened, try to show it", base_label);
                        if OPEN_WINDOWS_COUNTER.load(Ordering::Acquire) == 0 {
                            trace_err!(window.unminimize(), "set win unminimize");
                            trace_err!(window.show(), "set win visible");
                            trace_err!(window.set_focus(), "set win focus");
                        }
                    }
                    // Return early indicator - we'll handle this below
                    String::new()
                })
        };

        // Handle singleton window that already exists
        if label.is_empty() {
            return Ok(WindowCreateResult::existing(base_label.to_string()));
        }

        let always_on_top = config.always_on_top.unwrap_or_else(|| {
            *Config::verge()
                .latest()
                .always_on_top
                .as_ref()
                .unwrap_or(&false)
        });

        // Build URL with params
        let url = build_url_with_params(self.url(), params.as_ref());

        tracing::debug!("create {} window (label: {})...", base_label, label);

        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            label.clone(),
            tauri::WebviewUrl::App(url.into()),
        )
        .title(self.title())
        .fullscreen(false)
        .always_on_top(always_on_top)
        .resizable(config.resizable)
        .skip_taskbar(config.skip_taskbar)
        .disable_drag_drop_handler();

        // Apply min/max size
        if let Some((w, h)) = config.min_size {
            builder = builder.min_inner_size(w, h);
        }
        if let Some((w, h)) = config.max_size {
            builder = builder.max_inner_size(w, h);
        }

        let win_state = &self.get_window_state();
        match win_state {
            Some(_) => {
                builder = builder.inner_size(800., 800.).position(0., 0.);
            }
            _ => {
                let (default_width, default_height) = config.default_size;

                #[cfg(target_os = "windows")]
                {
                    builder = builder.inner_size(default_width, default_height);
                }

                #[cfg(target_os = "macos")]
                {
                    // macOS has slightly different height due to title bar
                    builder = builder.inner_size(default_width, default_height + 6.0);
                }

                #[cfg(target_os = "linux")]
                {
                    builder = builder.inner_size(default_width, default_height + 6.0);
                }

                if config.center {
                    builder = builder.center();
                }
            }
        };

        #[cfg(windows)]
        let win_res = builder
            .decorations(false)
            .transparent(true)
            .visible(false)
            .additional_browser_args("--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling")
            .build();

        #[cfg(target_os = "macos")]
        let win_res = {
            let decorations = config.decorations.unwrap_or(true);
            builder
                .decorations(decorations)
                .hidden_title(true)
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .build()
        };

        #[cfg(target_os = "linux")]
        let win_res = {
            let decorations = config.decorations.unwrap_or(true);
            let transparent = config.transparent.unwrap_or(false);
            builder
                .decorations(decorations)
                .transparent(transparent)
                .build()
        };

        match win_res {
            Ok(win) => {
                use tauri::{PhysicalPosition, PhysicalSize};

                if win_state.is_some() {
                    let state = win_state.as_ref().unwrap();
                    let _ = win.set_position(PhysicalPosition {
                        x: state.x,
                        y: state.y,
                    });
                    let _ = win.set_size(PhysicalSize {
                        width: state.width,
                        height: state.height,
                    });
                }

                if let Some(state) = win_state {
                    if state.maximized {
                        trace_err!(win.maximize(), "set win maximize");
                    }
                    if state.fullscreen {
                        trace_err!(win.set_fullscreen(true), "set win fullscreen");
                    }
                }
                #[cfg(windows)]
                trace_err!(win.set_shadow(true), "set win shadow");
                log::trace!("try to calculate the monitor size");
                let center = (|| -> Result<bool> {
                    let center;
                    if let Some(state) = win_state {
                        let monitor = win.current_monitor()?.ok_or(anyhow::anyhow!(""))?;
                        let PhysicalPosition { x, y } = *monitor.position();
                        let PhysicalSize { width, height } = *monitor.size();
                        let left = x;
                        let right = x + width as i32;
                        let top = y;
                        let bottom = y + height as i32;

                        let x = state.x;
                        let y = state.y;
                        let width = state.width as i32;
                        let height = state.height as i32;
                        center = ![
                            (x, y),
                            (x + width, y),
                            (x, y + height),
                            (x + width, y + height),
                        ]
                        .into_iter()
                        .any(|(x, y)| x >= left && x < right && y >= top && y < bottom);
                    } else {
                        center = true;
                    }
                    Ok(center)
                })();

                if center.unwrap_or(true) {
                    trace_err!(win.center(), "set win center");
                }

                #[cfg(debug_assertions)]
                {
                    if let Some(webview_window) = win.get_webview_window(&label) {
                        webview_window.open_devtools();
                    }
                }

                #[cfg(target_os = "macos")]
                {
                    tracing::trace!("setup traffic lights pos");
                    let mtm = objc2_foundation::MainThreadMarker::new().unwrap();
                    crate::window::macos::setup_traffic_lights_pos(win.clone(), (18.0, 22.0), mtm);
                }

                // Register window close event to clean up WindowManager
                let label_clone = label.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::Destroyed = event {
                        tracing::debug!("window {} destroyed, removing from manager", label_clone);
                        let mut manager = WindowManager::global().lock().unwrap();
                        manager.remove_instance(&label_clone);
                        OPEN_WINDOWS_COUNTER.fetch_sub(1, Ordering::Release);
                    }
                });

                OPEN_WINDOWS_COUNTER.fetch_add(1, Ordering::Release);
                Ok(WindowCreateResult::new(label))
            }
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                // Remove from manager on failure
                {
                    let mut manager = WindowManager::global().lock().unwrap();
                    manager.remove_instance(&label);
                }
                if let Some(win) = app_handle.get_webview_window(&label) {
                    // Cleanup window if failed to create, it's a workaround for tauri bug
                    log_err!(
                        win.destroy(),
                        "occur error when close window while failed to create"
                    );
                }
                Err(err.into())
            }
        }
    }

    /// Create window with default implementation (no params)
    fn create(&self, app_handle: &AppHandle) -> Result<()> {
        let result = self.create_with_params(app_handle, None)?;

        // Configure webview settings asynchronously to avoid blocking
        #[cfg(target_os = "windows")]
        if result.is_new {
            let label = result.label.clone();
            let app_handle = app_handle.clone();
            std::thread::spawn(move || {
                use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6;
                use windows_core::Interface;

                // Wait a bit for webview to be ready
                std::thread::sleep(std::time::Duration::from_millis(100));

                if let Some(window) = app_handle.get_webview_window(&label) {
                    let _ = window.with_webview(|webview| unsafe {
                        if let Ok(core) = webview.controller().CoreWebView2() {
                            if let Ok(settings) = core.Settings() {
                                if let Ok(settings6) = settings.cast::<ICoreWebView2Settings6>() {
                                    let _ = settings6.SetIsSwipeNavigationEnabled(false);
                                }
                            }
                        }
                    });
                }
            });
        }

        Ok(())
    }

    /// Close window by label
    ///
    /// Note: The WindowManager cleanup is handled automatically by the
    /// on_window_event callback registered during window creation.
    fn close_by_label(&self, app_handle: &AppHandle, label: &str) {
        if let Some(window) = app_handle.get_webview_window(label) {
            trace_err!(window.close(), "close window");
            // WindowManager cleanup is handled by on_window_event(Destroyed)
        }
    }

    /// Close window with default implementation (closes the base label window)
    fn close(&self, app_handle: &AppHandle) {
        self.close_by_label(app_handle, self.label());
    }

    /// Close all instances of this window type
    fn close_all(&self, app_handle: &AppHandle) {
        let instances = {
            let manager = WindowManager::global().lock().unwrap();
            manager.get_instances(self.label())
        };
        for label in instances {
            self.close_by_label(app_handle, &label);
        }
    }

    /// Check if the base label window is open
    fn is_open(&self, app_handle: &AppHandle) -> bool {
        app_handle.get_webview_window(self.label()).is_some()
    }

    /// Check if any instance of this window type is open
    fn has_any_instance(&self, app_handle: &AppHandle) -> bool {
        let manager = WindowManager::global().lock().unwrap();
        let instances = manager.get_instances(self.label());
        instances
            .iter()
            .any(|label| app_handle.get_webview_window(label).is_some())
    }

    /// Get all open window labels for this type
    fn get_open_instances(&self, app_handle: &AppHandle) -> Vec<String> {
        let manager = WindowManager::global().lock().unwrap();
        manager
            .get_instances(self.label())
            .into_iter()
            .filter(|label| app_handle.get_webview_window(label).is_some())
            .collect()
    }

    /// Send a message to another window
    fn send_message(
        &self,
        app_handle: &AppHandle,
        to: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let message = WindowMessageEvent::new(self.label(), to, event, payload);
        send_message_to_window(app_handle, message)
    }

    /// Broadcast a message to all instances of another window type
    fn broadcast_to_type(
        &self,
        app_handle: &AppHandle,
        target_type: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        broadcast_to_window_type(app_handle, target_type, self.label(), event, payload)
    }

    /// Save window state with default implementation
    fn save_state(&self, app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
        let win = app_handle
            .get_webview_window(self.label())
            .ok_or(anyhow::anyhow!("failed to get window"))?;
        let current_monitor = win.current_monitor()?;

        let state = match current_monitor {
            Some(_) => {
                let mut state = WindowState {
                    maximized: win.is_maximized()?,
                    fullscreen: win.is_fullscreen()?,
                    ..WindowState::default()
                };
                let is_minimized = win.is_minimized()?;

                let size = win.inner_size()?;
                if size.width > 0 && size.height > 0 && !state.maximized && !is_minimized {
                    state.width = size.width;
                    state.height = size.height;
                }
                let position = win.outer_position()?;
                if !state.maximized && !is_minimized {
                    state.x = position.x;
                    state.y = position.y;
                }
                Some(state)
            }
            None => None,
        };

        self.set_window_state(state);

        if save_to_file {
            Config::verge().data().save_file()?;
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    #![allow(non_snake_case)]
    use std::cell::RefCell;

    use objc2::{
        DeclaredClass, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
    };
    use objc2_app_kit::{NSApplicationPresentationOptions, NSWindow, NSWindowDelegate};
    use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};
    use tauri::{Emitter, Listener, Manager, Runtime, WebviewWindow, Window, WindowEvent};

    #[derive(Debug, Clone, Copy)]
    pub struct Position {
        pub x: f64,
        pub y: f64,
    }

    impl From<(f64, f64)> for Position {
        fn from(value: (f64, f64)) -> Self {
            Self {
                x: value.0,
                y: value.1,
            }
        }
    }

    impl From<Position> for (f64, f64) {
        fn from(value: Position) -> Self {
            (value.x, value.y)
        }
    }

    fn set_traffic_lights_pos(
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
        pos: Position,
    ) -> anyhow::Result<()> {
        use objc2_app_kit::NSWindowButton;
        use objc2_foundation::NSRect;
        let close = window
            .standardWindowButton(NSWindowButton::CloseButton)
            .ok_or(anyhow::anyhow!("failed to get close button"))?;
        let miniaturize = window
            .standardWindowButton(NSWindowButton::MiniaturizeButton)
            .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
        let zoom = window
            .standardWindowButton(NSWindowButton::ZoomButton)
            .ok_or(anyhow::anyhow!("failed to get zoom button"))?;

        let title_bar_container_view = unsafe {
            close
                .superview()
                .and_then(|view| view.superview())
                .ok_or(anyhow::anyhow!("failed to get title bar container view"))?
        };

        let close_rect = close.frame();
        let button_height = close_rect.size.height;

        let title_bar_frame_height = button_height + pos.y;
        let mut title_bar_rect = title_bar_container_view.frame();
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
        unsafe {
            title_bar_container_view.setFrame(title_bar_rect);
        }

        let space_between = miniaturize.frame().origin.x - close.frame().origin.x;
        let window_buttons = vec![close, miniaturize, zoom];

        for (i, button) in window_buttons.into_iter().enumerate() {
            let mut rect: NSRect = button.frame();
            rect.origin.x = pos.x + (i as f64 * space_between);
            unsafe {
                button.setFrameOrigin(rect.origin);
            }
        }
        Ok(())
    }

    #[derive(Debug, Clone)]
    struct WindowState {
        window: WebviewWindow<tauri::Wry>,
        traffic_lights_pos: Position,
    }

    impl WindowState {
        fn new(window: WebviewWindow<tauri::Wry>, traffic_lights_pos: Position) -> Self {
            Self {
                window,
                traffic_lights_pos,
            }
        }

        fn with_ns_window<T>(&self, func: impl FnOnce(Retained<NSWindow>) -> T) -> T {
            let ns_window = self.window.ns_window().expect("window not found");
            let ns_window = unsafe { Retained::retain_autoreleased(ns_window as *mut NSWindow) }
                .expect("failed to retain window");
            func(ns_window)
        }

        fn apply_traffic_lights_pos(&self) {
            self.with_ns_window(|win| {
                set_traffic_lights_pos(win, self.traffic_lights_pos)
                    .expect("failed to set traffic lights pos");
            });
        }
    }

    #[derive(Debug)]
    struct TrafficLightsWindowDelegateIvars {
        app_box: WindowState,
        super_class: Retained<ProtocolObject<dyn NSWindowDelegate>>,
    }

    const WINDOW_DID_ENTER_FULL_SCREEN: &str = "internal:://window-did-enter-full-screen";
    const WINDOW_WILL_ENTER_FULL_SCREEN: &str = "internal:://window-will-enter-full-screen";
    const WINDOW_WILL_EXIT_FULL_SCREEN: &str = "internal:://window-will-exit-full-screen";
    const WINDOW_DID_EXIT_FULL_SCREEN: &str = "internal:://window-did-exit-full-screen";

    define_class! {
        #[unsafe(super(NSObject))]
        #[name = "TrafficLightsPosWindowDelegate"]
        #[thread_kind = MainThreadOnly]
        #[ivars = TrafficLightsWindowDelegateIvars]
        struct WindowDelegate;

        unsafe impl NSObjectProtocol for WindowDelegate {}

        unsafe impl NSWindowDelegate for WindowDelegate {
            #[unsafe(method(windowShouldClose:))]
            unsafe fn windowShouldClose(&self, sender: &NSWindow) -> bool {
                tracing::trace!("passthrough `windowShouldClose` to TAO layer");
                unsafe { self.ivars().super_class.windowShouldClose(sender) }
            }

            #[unsafe(method(windowWillClose:))]
            unsafe fn windowWillClose(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowWillClose` to TAO layer");
                unsafe { self.ivars().super_class.windowWillClose(notification) }
            }

            #[unsafe(method(windowDidResize:))]
            unsafe fn windowDidResize(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidResize` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResize(notification) }
            }

            #[unsafe(method(windowDidMove:))]
            unsafe fn windowDidMove(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidMove` to TAO layer");
                unsafe { self.ivars().super_class.windowDidMove(notification) }
            }

            #[unsafe(method(windowDidChangeBackingProperties:))]
            unsafe fn windowDidChangeBackingProperties(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidChangeBackingProperties` to TAO layer");
                unsafe { self.ivars().super_class.windowDidChangeBackingProperties(notification) }
            }

            #[unsafe(method(windowDidBecomeKey:))]
            unsafe fn windowDidBecomeKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidBecomeKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidBecomeKey(notification) }
            }

            #[unsafe(method(windowDidResignKey:))]
            unsafe fn windowDidResignKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidResignKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResignKey(notification) }
            }

            #[unsafe(method(window:willUseFullScreenPresentationOptions:))]
            unsafe fn window_willUseFullScreenPresentationOptions(&self, window: &NSWindow, options: NSApplicationPresentationOptions) -> NSApplicationPresentationOptions {
                tracing::trace!("passthrough `window_willUseFullScreenPresentationOptions` to TAO layer");
                unsafe { self.ivars().super_class.window_willUseFullScreenPresentationOptions(window, options) }
            }

            #[unsafe(method(windowDidEnterFullScreen:))]
            unsafe fn windowDidEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-enter-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowDidEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillEnterFullScreen:))]
            unsafe fn windowWillEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-enter-full-screen event: {}", e);
                }
                unsafe { self.ivars().super_class.windowWillEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillExitFullScreen:))]
            unsafe fn windowWillExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-exit-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowWillExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowWillExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidExitFullScreen:))]
            unsafe fn windowDidExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-exit-full-screen event: {}", e);
                }
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidFailToEnterFullScreen:))]
            unsafe fn windowDidFailToEnterFullScreen(&self,window: &NSWindow) {
                tracing::trace!("passthrough `windowDidFailToEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidFailToEnterFullScreen(window) }
            }

        }
    }

    impl WindowDelegate {
        pub fn new(window_state: WindowState, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm);
            let super_class = window_state
                .with_ns_window(|win| unsafe { win.delegate().expect("failed to get delegate") });
            let ivars = TrafficLightsWindowDelegateIvars {
                app_box: window_state,
                super_class,
            };
            let this = this.set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    pub struct TrafficLightsWindowDelegateGuard {
        _delegate: Retained<WindowDelegate>,
    }

    thread_local! {
        /// This is used to keep the delegate alive until the window is destroyed
        static TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD: RefCell<Option<TrafficLightsWindowDelegateGuard>> = const { RefCell::new(None) };
    }

    pub fn setup_traffic_lights_pos(window: WebviewWindow, pos: (f64, f64), mtm: MainThreadMarker) {
        let window_state = WindowState::new(window.clone(), pos.into());
        let ns_window = window_state.with_ns_window(|win| win);
        let window_state_clone = window_state.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::ThemeChanged(_) => {
                window_state_clone.apply_traffic_lights_pos();
            }
            WindowEvent::Destroyed => {
                let _ = TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD.take();
            }
            _ => {}
        });
        // first apply the traffic lights pos
        window_state.apply_traffic_lights_pos();
        let delegate = WindowDelegate::new(window_state, mtm);
        let object: &ProtocolObject<dyn NSWindowDelegate> = ProtocolObject::from_ref(&*delegate);
        ns_window.setDelegate(Some(object));
        TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD.replace(Some(TrafficLightsWindowDelegateGuard {
            _delegate: delegate,
        }));
    }
}
