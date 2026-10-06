//! Tauri window management mod
//!
//! This module provides a flexible window management system that supports:
//! - URL parameters for windows
//! - Multiple instances of the same window type (e.g., main, main-1, main-2)
//! - Inter-window communication
//! - Configurable window properties (singleton, size, etc.)
//!
//! [`WindowManager`] owns every window's lifecycle: it builds windows hidden and
//! shows them once their frontend reports ready.

mod engine;
pub mod kinds;
#[cfg(target_os = "macos")]
mod macos;
mod manager;
mod table;

pub use manager::WindowManager;

use crate::trace_err;
use anyhow::Result;
use nyanpasu_config::{
    application::{WindowCloseOverride, WindowCloseSettings},
    state::window::WindowState,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_specta::Event;

/// Window configuration options
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WindowConfig {
    /// Whether only one instance of this window type is allowed
    pub singleton: bool,
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
    /// Whether to skip taskbar
    pub skip_taskbar: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            singleton: true,
            default_size: (800.0, 636.0),
            min_size: Some((400.0, 600.0)),
            max_size: None,
            center: true,
            resizable: true,
            always_on_top: None,
            decorations: None,
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
    #[allow(dead_code)]
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

    /// Set whether to use decorations (title bar and traffic lights on macOS)
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = Some(decorations);
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

#[allow(dead_code)]
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
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value).
    // Keep as `any` until a named recursive JsonValue type is supported by specta.
    #[specta(type = specta_typescript::Any)]
    pub payload: serde_json::Value,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn broadcast_to_window_type(
    app_handle: &AppHandle,
    base_label: &str,
    from: &str,
    event: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let instances = app_handle
        .try_state::<WindowManager>()
        .ok_or_else(|| anyhow::anyhow!("the window manager is not managed yet"))?
        .instances(base_label);

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
#[allow(dead_code)]
pub fn broadcast_to_all_windows(
    app_handle: &AppHandle,
    from: &str,
    event: &str,
    payload: serde_json::Value,
) -> Result<()> {
    WindowMessageEvent::broadcast(from, event, payload).emit(app_handle)?;
    Ok(())
}

/// What a window kind does when its window is applied. Object safe, so the
/// manager can keep it with the window.
pub trait WindowHooks: Send + Sync + 'static {
    fn on_dismissed(&self, window: &WebviewWindow);

    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride;
}

impl<K: AppWindow> WindowHooks for K {
    fn on_dismissed(&self, window: &WebviewWindow) {
        AppWindow::on_dismissed(self, window);
    }

    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        AppWindow::close_override(self, settings)
    }
}

/// Trait for window management
#[allow(dead_code)]
pub trait AppWindow: Clone + Send + Sync + 'static {
    /// Get window base label (e.g., "main", "editor")
    fn label(&self) -> &str;

    /// Get window title
    fn title(&self) -> &str;

    /// Get window URL path
    fn url(&self) -> &str;

    /// Build the final window URL with optional parameters.
    fn url_with_params(&self, params: Option<&WindowParams>) -> String {
        build_url_with_params(self.url(), params)
    }

    /// Get window configuration
    fn config(&self) -> WindowConfig {
        WindowConfig::default()
    }

    /// The geometry to restore the window with, if it remembers one.
    fn get_window_state(&self, app_handle: &AppHandle) -> Option<WindowState>;

    /// Which of the settings governs how this kind of window closes.
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride;

    /// Finishes setting up a window just built, before it can be shown.
    fn on_created(&self, _window: &WebviewWindow) {}

    /// Reacts to this window's events, before the manager's own handling.
    fn on_window_event(&self, _window: &WebviewWindow, _event: &WindowEvent) {}

    /// The window is about to go away or be hidden after being shown, whether
    /// the user closed it or the app did.
    fn on_dismissed(&self, _window: &WebviewWindow) {}

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

    /// Reads geometry without changing any source or projection.
    fn capture_state(&self, app_handle: &AppHandle) -> Result<Option<WindowState>> {
        let win = app_handle
            .get_webview_window(self.label())
            .ok_or(anyhow::anyhow!("failed to get window"))?;
        if win.is_minimized()? {
            return Ok(None);
        }

        let state = match win.current_monitor()? {
            Some(_) => {
                let maximized = win.is_maximized()?;
                let fullscreen = win.is_fullscreen()?;
                let size = win.inner_size()?;

                // During system shutdown, Windows sends resize events with 0x0 dimensions.
                // Skip saving in this case to preserve the last valid window state.
                if (size.width == 0 || size.height == 0) && !maximized && !fullscreen {
                    tracing::debug!(
                        "skipping window state save: invalid size {}x{} in normal state",
                        size.width,
                        size.height
                    );
                    return Ok(None);
                }

                let mut state = WindowState {
                    maximized,
                    fullscreen,
                    ..WindowState::default()
                };

                if size.width > 0 && size.height > 0 && !state.maximized {
                    state.width = size.width;
                    state.height = size.height;
                }
                let position = win.outer_position()?;
                if !state.maximized {
                    state.x = position.x;
                    state.y = position.y;
                }
                Some(state)
            }
            None => None,
        };

        Ok(state)
    }
}
