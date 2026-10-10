//! Owns the lifecycle of every window: builds it hidden, shows it once its
//! frontend reports ready, and forgets it when it is destroyed. Tauri window
//! operations are issued only from here.

use super::{
    AppWindow, WindowHooks, WindowParams, engine,
    table::{Opened, WindowTable},
};
use crate::{log_err, trace_err};
use anyhow::Result;
use nyanpasu_config::application::WindowCloseBehavior;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

/// How long a window may stay hidden waiting for its frontend before it is
/// shown anyway, so a crashed or hung webview cannot leave it invisible.
const READY_FALLBACK: Duration = Duration::from_secs(10);

type Table = WindowTable<Arc<dyn WindowHooks>>;

/// A GUI-crate adapter, managed as Tauri state by the composition root. The
/// lock guards the table only: hold it just to read or write facts, never
/// while calling Tauri or queueing an apply.
pub struct WindowManager {
    app_handle: AppHandle,
    table: Mutex<Table>,
}

impl WindowManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            table: Mutex::new(Table::default()),
        }
    }

    /// Opens a window of this kind: builds it hidden, or asks the existing one
    /// to be shown.
    pub fn open<K: AppWindow>(&self, kind: &K, params: Option<WindowParams>) -> Result<String> {
        let hooks: Arc<dyn WindowHooks> = Arc::new(kind.clone());
        let opened = self
            .table
            .lock()
            .open(kind.label(), kind.config().singleton, hooks);
        match opened {
            Opened::Existing(label) => {
                tracing::debug!("{} window is already opened, try to focus it", label);
                // Always applied, so a window that is already shown is focused.
                self.queue_apply(&label);
                Ok(label)
            }
            Opened::Build(label) => {
                self.create(kind, &label, params)?;
                Ok(label)
            }
        }
    }

    /// The frontend of `label` has rendered.
    pub fn report_ready(&self, label: &str) {
        self.ready(label);
    }

    /// Closes the window: it is no longer wanted. Applying that destroys or
    /// hides it. The native close is never called from here: it would come
    /// back as a close request that cannot be told from the user's, and
    /// could undo a later open.
    pub fn close(&self, label: &str) {
        let changed = self.table.lock().unwant(label);
        if changed {
            self.queue_apply(label);
        }
    }

    /// Whether the window is wanted: opened and not closed since.
    pub fn is_wanted(&self, label: &str) -> bool {
        self.table.lock().wanted(label)
    }

    /// The labels of the windows opened under `base_label`.
    pub fn instances(&self, base_label: &str) -> Vec<String> {
        self.table.lock().instances(base_label)
    }

    fn ready(&self, label: &str) {
        let first = self.table.lock().ready(label);
        match first {
            Some(true) => {
                tracing::debug!("Window '{}' is ready", label);
                self.queue_apply(label);
            }
            Some(false) => {}
            None => tracing::warn!("Window '{}' reported ready but is not known", label),
        }
    }

    /// Queues an apply of the window's facts. Always queued, never run inline:
    /// the caller may be a window listener, which wry calls with the window's
    /// listener list locked, or hold a lock of its own.
    fn queue_apply(&self, label: &str) {
        let handle = self.app_handle.clone();
        let label = label.to_string();
        tauri::async_runtime::spawn(async move {
            let on_main = handle.clone();
            log_err!(handle.run_on_main_thread(move || apply(&on_main, &label)));
        });
    }

    fn create<K: AppWindow>(
        &self,
        kind: &K,
        label: &str,
        params: Option<WindowParams>,
    ) -> Result<()> {
        let app_handle = &self.app_handle;
        let config = kind.config();
        let base_label = kind.label();

        let always_on_top = config.always_on_top.unwrap_or_else(|| {
            app_handle
                .try_state::<nyanpasu_core::client::NyanpasuClient>()
                .is_some_and(|client| client.app_config_snapshot().always_on_top)
        });

        // Build URL with params
        let url = kind.url_with_params(params.as_ref());

        tracing::debug!("create {} window (label: {})...", base_label, label);

        let mut builder =
            tauri::WebviewWindowBuilder::new(app_handle, label, tauri::WebviewUrl::App(url.into()))
                .title(kind.title())
                .fullscreen(false)
                .always_on_top(always_on_top)
                .resizable(config.resizable)
                .skip_taskbar(config.skip_taskbar)
                .disable_drag_drop_handler()
                // Shown by `reveal`, once the frontend has rendered.
                .visible(false);

        // Apply min/max size
        if let Some((w, h)) = config.min_size {
            builder = builder.min_inner_size(w, h);
        }
        if let Some((w, h)) = config.max_size {
            builder = builder.max_inner_size(w, h);
        }

        let win_state = &kind.get_window_state(app_handle);
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

        let builder = engine::configure_builder(builder);

        #[cfg(windows)]
        let win_res = builder.decorations(false).build();

        #[cfg(target_os = "macos")]
        let win_res = {
            let decorations = config.decorations.unwrap_or(true);
            if decorations {
                // Tao keeps the buttons there across resizes, fullscreen and
                // title changes.
                builder
                    .decorations(true)
                    .hidden_title(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .traffic_light_position(tauri::LogicalPosition::new(18.0, 22.0))
                    .build()
            } else {
                builder.decorations(false).build()
            }
        };

        #[cfg(target_os = "linux")]
        let win_res = {
            let decorations = config.decorations.unwrap_or(true);
            builder.decorations(decorations).build()
        };

        let win = match win_res {
            Ok(win) => win,
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                self.table.lock().remove(label);
                if let Some(win) = app_handle.get_webview_window(label) {
                    // Cleanup window if failed to create, it's a workaround for tauri bug
                    log_err!(
                        win.destroy(),
                        "occur error when close window while failed to create"
                    );
                }
                return Err(err.into());
            }
        };

        // Before anything else is set up, so a window destroyed meanwhile is
        // forgotten.
        self.watch(kind, label, &win);

        use tauri::{PhysicalPosition, PhysicalSize};

        if win_state.is_some() {
            let state = win_state.as_ref().unwrap();
            let _ = win.set_position(PhysicalPosition {
                x: state.x,
                y: state.y,
            });
            // Clamp restored size to min_size to prevent 0x0 windows
            let mut width = state.width;
            let mut height = state.height;
            if let Some((min_w, min_h)) = config.min_size {
                let scale_factor = win.scale_factor().unwrap_or(1.0);
                let min_w_physical = (min_w * scale_factor) as u32;
                let min_h_physical = (min_h * scale_factor) as u32;
                if width < min_w_physical {
                    width = min_w_physical;
                }
                if height < min_h_physical {
                    height = min_h_physical;
                }
            }
            let _ = win.set_size(PhysicalSize { width, height });
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
            if let Some(webview_window) = win.get_webview_window(label) {
                webview_window.open_devtools();
            }
        }

        kind.on_created(&win);

        // The window is built and set up, so it can be acted on from now on.
        let awaiting_ready = self.table.lock().built(label);
        match awaiting_ready {
            Some(true) => {
                self.start_fallback(label);
                self.queue_apply(label);
            }
            Some(false) => self.queue_apply(label),
            None => {}
        }
        Ok(())
    }

    /// Follows the window's events: the kind reacts first, a close request
    /// from the user is a close like any other, and destruction removes the
    /// window from the table.
    fn watch<K: AppWindow>(&self, kind: &K, label: &str, win: &WebviewWindow) {
        let kind = kind.clone();
        let window = win.clone();
        let handle = self.app_handle.clone();
        let event_label = label.to_string();
        win.on_window_event(move |event| {
            kind.on_window_event(&window, event);
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // The window closes through the table, and what that means
                    // for it is decided when the table is applied.
                    if let Some(windows) = handle.try_state::<WindowManager>() {
                        api.prevent_close();
                        windows.close(&event_label);
                    }
                }
                WindowEvent::Destroyed => {
                    tracing::debug!("window {} destroyed, removing from the table", event_label);
                    if let Some(windows) = handle.try_state::<WindowManager>() {
                        windows.table.lock().remove(&event_label);
                    }
                }
                _ => {}
            }
        });
    }

    /// Marks a window ready once [`READY_FALLBACK`] has passed without its
    /// webview reporting, so a crashed or hung webview cannot leave it unseen.
    fn start_fallback(&self, label: &str) {
        let handle = self.app_handle.clone();
        let fallback_label = label.to_string();
        let fallback = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(READY_FALLBACK).await;
            let Some(windows) = handle.try_state::<WindowManager>() else {
                return;
            };
            let first = windows.table.lock().ready(&fallback_label);
            if first == Some(true) {
                tracing::warn!(
                    "Window '{}' did not report ready within {}s, treating it as ready",
                    fallback_label,
                    READY_FALLBACK.as_secs()
                );
                windows.queue_apply(&fallback_label);
            }
        });
        let fallback = fallback.inner();
        let kept = self
            .table
            .lock()
            .set_watchdog(label, &fallback.abort_handle());
        // The window is already ready or destroyed, so nothing is left to wait for.
        if !kept {
            fallback.abort();
        }
    }
}

/// Makes the native window match the table, on the main thread, where a close
/// request is also handled: shown when it is ready and wanted, and gone, or
/// hidden, when it is not wanted. It reads the facts when it runs, so which
/// events queued it, and in which order, does not matter; one queued for a
/// window that is gone or replaced sees the facts of what is there. It never
/// builds a window, which would deadlock here on Windows. The table is not
/// held while Tauri is called, as showing a window can re-enter the event
/// handler.
fn apply(handle: &AppHandle, label: &str) {
    let Some(windows) = handle.try_state::<WindowManager>() else {
        return;
    };
    let Some(facts) = windows.table.lock().facts(label) else {
        return;
    };
    let Some(window) = handle.get_webview_window(label) else {
        return;
    };
    let presented = window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false);
    let actions = facts.actions(presented);

    if actions.first_ready {
        engine::on_first_ready(&window);
    }

    if actions.show {
        #[cfg(target_os = "macos")]
        if label == crate::consts::MAIN_WINDOW_LABEL {
            crate::utils::dock::macos::show_dock_icon();
        }

        trace_err!(window.unminimize(), "set win unminimize");
        trace_err!(window.show(), "set win visible");
        trace_err!(window.set_focus(), "set win focus");
    } else if actions.retire {
        if actions.dismiss {
            facts.hooks.on_dismissed(&window);
        }
        let settings = handle
            .try_state::<nyanpasu_core::client::NyanpasuClient>()
            .map(|client| client.app_config_snapshot().window_close)
            .unwrap_or_default();
        match facts
            .hooks
            .close_override(&settings)
            .resolve(settings.global)
        {
            WindowCloseBehavior::Destroy => trace_err!(window.destroy(), "destroy window"),
            WindowCloseBehavior::Hide if actions.dismiss => {
                trace_err!(window.hide(), "hide window");
            }
            WindowCloseBehavior::Hide => {}
        }
    }
}
