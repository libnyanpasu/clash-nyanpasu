//! The kinds of window this app opens: what each looks like, where it loads
//! from, and how it reacts to its own events. [`WindowManager`] does the rest.

use super::{
    AppWindow, WindowConfig, WindowManager, WindowParams, WindowParamsBuilder,
    build_url_with_params,
};
use crate::client::NyanpasuClient;
use anyhow::Result;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, TrayMenuCloseBehavior},
    state::window::WindowState,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

const TRAY_MENU_SHOW_BLUR_GRACE: Duration = Duration::from_millis(750);
const TRAY_MENU_FOCUS_BLUR_GRACE: Duration = Duration::from_millis(250);

/// Decides when the webview tray menu is dismissed on focus loss. The caller
/// passes the time in, so the rules hold without a window.
#[derive(Debug, Default)]
struct TrayMenuFocus {
    /// Set for the debug menu window, which stays open on focus loss.
    persistent: bool,
    /// Set to true only after the window has received Focused(true) at least once.
    /// Prevents spurious Focused(false) events during window creation from triggering
    /// hide/close before the user has ever seen the window.
    ready: bool,
    /// Ignore focus-loss events until this instant.
    ///
    /// Windows can emit Focused(true) immediately followed by Focused(false) while
    /// the shell is still finishing the tray right-click interaction. Without a
    /// short guard window, the webview tray menu flashes and is hidden/closed
    /// before it can be used.
    ignore_blur_until: Option<Instant>,
}

impl TrayMenuFocus {
    /// The menu was shown at the cursor.
    fn shown(&mut self, now: Instant) {
        self.persistent = false;
        self.ready = false;
        self.ignore_blur_until = Some(now + TRAY_MENU_SHOW_BLUR_GRACE);
    }

    /// The debug menu window was opened.
    fn shown_persistent(&mut self, now: Instant) {
        self.persistent = true;
        self.ignore_blur_until = Some(now + TRAY_MENU_SHOW_BLUR_GRACE);
    }

    fn focused(&mut self, now: Instant) {
        self.ready = true;
        self.ignore_blur_until = Some(now + TRAY_MENU_FOCUS_BLUR_GRACE);
    }

    /// Whether this focus loss dismisses the menu.
    fn blurred(&mut self, now: Instant) -> bool {
        if self.ignore_blur_until.is_some_and(|until| now < until) {
            return false;
        }
        if !self.persistent && self.ready {
            self.ready = false;
            return true;
        }
        false
    }
}

/// The tray menu window's focus state, managed as Tauri state by the
/// composition root.
#[derive(Debug, Default)]
pub struct TrayMenuWindowController {
    focus: parking_lot::Mutex<TrayMenuFocus>,
}

impl TrayMenuWindowController {
    fn shown(&self) {
        self.focus.lock().shown(Instant::now());
    }

    fn shown_persistent(&self) {
        self.focus.lock().shown_persistent(Instant::now());
    }

    fn focused(&self) {
        self.focus.lock().focused(Instant::now());
    }

    fn blurred(&self) -> bool {
        self.focus.lock().blurred(Instant::now())
    }
}

/// Main window implementation (new UI)
#[derive(Clone)]
pub struct MainWindow;

impl AppWindow for MainWindow {
    fn label(&self) -> &str {
        crate::consts::MAIN_WINDOW_LABEL
    }

    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }

    fn url(&self) -> &str {
        "/main"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self, app_handle: &AppHandle) -> Option<WindowState> {
        app_handle
            .try_state::<NyanpasuClient>()?
            .main_window_geometry()
    }

    fn on_dismissed(&self, window: &WebviewWindow) {
        log::debug!(target: "app", "window dismissed");
        let _ = save_window_state(window.app_handle());
        #[cfg(target_os = "macos")]
        crate::utils::dock::macos::hide_dock_icon();
    }
}

/// Type of content the editor window displays.
/// Used to derive the window label (for singleton logic) and URL path params.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum EditorWindowType {
    Profile,
    CssEditor,
}

impl EditorWindowType {
    /// Stable string used to build the window label suffix.
    fn label_suffix(&self) -> &str {
        match self {
            Self::Profile => "profile",
            Self::CssEditor => "css",
        }
    }

    fn type_str(&self) -> &str {
        match self {
            Self::Profile => "profile",
            Self::CssEditor => "css",
        }
    }
}

/// Editor window
#[derive(Clone)]
pub struct EditorWindow {
    label: String,
    window_type: EditorWindowType,
}

impl EditorWindow {
    /// The editor for `window_type`, with the URL parameters that open it.
    pub fn for_type(
        window_type: EditorWindowType,
        uid: Option<&str>,
    ) -> Result<(Self, Option<WindowParams>)> {
        let window = match &window_type {
            EditorWindowType::Profile => {
                let uid = uid.ok_or_else(|| anyhow::anyhow!("uid required for Profile editor"))?;
                Self::profile(uid)
            }
            EditorWindowType::CssEditor => Self::css_editor(),
        };
        let mut builder = WindowParamsBuilder::new().param("type", window_type.type_str());
        if let Some(u) = uid {
            builder = builder.param("uid", u);
        }
        Ok((window, builder.build()))
    }

    /// Profile editor — non-singleton, one window per uid.
    fn profile(uid: &str) -> Self {
        Self {
            label: format!(
                "{}-{}-{}",
                crate::consts::EDITOR_WINDOW_LABEL,
                EditorWindowType::Profile.label_suffix(),
                uid
            ),
            window_type: EditorWindowType::Profile,
        }
    }

    /// CSS editor — singleton, no uid.
    fn css_editor() -> Self {
        Self {
            label: format!(
                "{}-{}",
                crate::consts::EDITOR_WINDOW_LABEL,
                EditorWindowType::CssEditor.label_suffix()
            ),
            window_type: EditorWindowType::CssEditor,
        }
    }
}

impl AppWindow for EditorWindow {
    fn label(&self) -> &str {
        &self.label
    }

    fn title(&self) -> &str {
        crate::consts::APP_EDITOR_NAME
    }

    fn url(&self) -> &str {
        "/editor"
    }

    fn url_with_params(&self, params: Option<&HashMap<String, String>>) -> String {
        let editor_type = params
            .and_then(|params| params.get("type"))
            .map(String::as_str)
            .unwrap_or_else(|| self.window_type.type_str());

        let query_params = params.map(|params| {
            params
                .iter()
                .filter(|(key, _)| key.as_str() != "type")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });

        build_url_with_params(
            &format!("{}/{}", self.url(), editor_type),
            query_params.as_ref(),
        )
    }

    fn config(&self) -> WindowConfig {
        let singleton = matches!(self.window_type, EditorWindowType::CssEditor);
        WindowConfig::new()
            .singleton(singleton)
            .default_size(800.0, 636.0)
            .min_size(400.0, 500.0)
            .center(true)
    }

    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        // EditorWindow does not remember window state
        None
    }
}

pub async fn save_main_window_state_async(
    app_handle: &AppHandle,
    _save_to_file: bool,
) -> Result<()> {
    if let Some(geometry) = MainWindow.capture_state(app_handle)? {
        app_handle
            .state::<NyanpasuClient>()
            .save_main_window_geometry(geometry)
            .await?;
    }
    Ok(())
}

/// Queues a save of the main window's geometry, so the caller never waits
/// for the write: it runs on the main thread.
pub fn save_window_state(app_handle: &AppHandle) -> Result<()> {
    if let Some(geometry) = MainWindow.capture_state(app_handle)? {
        app_handle
            .state::<NyanpasuClient>()
            .queue_main_window_geometry_save(geometry)?;
    }
    Ok(())
}

/// Webview tray menu window
#[derive(Clone)]
struct TrayMenuWindow {
    /// Where a window built for this request is placed, before it can be shown.
    cursor: Option<PhysicalPosition<f64>>,
}

impl AppWindow for TrayMenuWindow {
    fn label(&self) -> &str {
        crate::consts::TRAY_MENU_WINDOW_LABEL
    }

    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }

    fn url(&self) -> &str {
        "/tray-menu"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .default_size(240.0, 448.0)
            .min_size(240.0, 448.0)
            .center(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .decorations(false)
    }

    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        None
    }

    fn hides_on_close(&self, config: &NyanpasuAppConfig) -> bool {
        config.tray_menu_close_behavior == TrayMenuCloseBehavior::Hide
    }

    fn on_created(&self, window: &WebviewWindow) {
        if let Some(cursor) = self.cursor {
            place_tray_menu_window(window, cursor);
        }
    }

    /// Hides or closes the menu on focus loss, as [`TrayMenuFocus`] decides.
    fn on_window_event(&self, window: &WebviewWindow, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(true) => {
                window.state::<TrayMenuWindowController>().focused();
            }
            WindowEvent::Focused(false) if window.state::<TrayMenuWindowController>().blurred() => {
                window.state::<WindowManager>().close(window.label());
            }
            _ => {}
        }
    }
}

/// Open a persistent tray menu window for debugging.
pub fn create_debug_tray_menu_window(app_handle: &AppHandle) -> Result<()> {
    app_handle
        .state::<TrayMenuWindowController>()
        .shown_persistent();

    let params = WindowParamsBuilder::new()
        .param("persistent", "true")
        .build();
    app_handle
        .state::<WindowManager>()
        .open(&TrayMenuWindow { cursor: None }, params)?;

    Ok(())
}

/// Show the webview tray menu window near the given cursor position.
pub fn show_tray_menu_window(app_handle: &AppHandle, cursor: PhysicalPosition<f64>) -> Result<()> {
    app_handle.state::<TrayMenuWindowController>().shown();

    // A window that is shown again is moved before it can be shown, so it
    // never appears at its old position first. A new one is placed as part of
    // its creation.
    if let Some(existing) = app_handle.get_webview_window(crate::consts::TRAY_MENU_WINDOW_LABEL) {
        place_tray_menu_window(&existing, cursor);
    }
    app_handle.state::<WindowManager>().open(
        &TrayMenuWindow {
            cursor: Some(cursor),
        },
        None,
    )?;

    Ok(())
}

/// Moves the tray menu window next to the cursor, inside the work area of the
/// monitor the cursor is on.
fn place_tray_menu_window(win: &WebviewWindow, cursor: PhysicalPosition<f64>) {
    let (menu_w, menu_h) = win
        .outer_size()
        .map(|size| {
            let width = if size.width == 0 {
                240.0
            } else {
                size.width as f64
            };
            let height = if size.height == 0 {
                448.0
            } else {
                size.height as f64
            };
            (width, height)
        })
        .unwrap_or((240.0, 448.0));
    let margin = 8.0_f64;

    let (screen_x, screen_y, screen_w, screen_h) = win
        .available_monitors()
        .ok()
        .and_then(|monitors| {
            monitors
                .iter()
                .find(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    let left = position.x as f64;
                    let top = position.y as f64;
                    let right = left + size.width as f64;
                    let bottom = top + size.height as f64;

                    cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
                })
                .or_else(|| monitors.first())
                .map(|monitor| {
                    let work_area = monitor.work_area();
                    let size = if work_area.size.width == 0 || work_area.size.height == 0 {
                        *monitor.size()
                    } else {
                        work_area.size
                    };
                    let position = if work_area.size.width == 0 || work_area.size.height == 0 {
                        *monitor.position()
                    } else {
                        work_area.position
                    };

                    (
                        position.x as f64,
                        position.y as f64,
                        size.width as f64,
                        size.height as f64,
                    )
                })
        })
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));

    let left = screen_x + margin;
    let top = screen_y + margin;
    let right = screen_x + screen_w - margin;
    let bottom = screen_y + screen_h - margin;

    let max_x = (right - menu_w).max(left);
    let max_y = (bottom - menu_h).max(top);
    let mut x = if cursor.x + menu_w > right {
        cursor.x - menu_w
    } else {
        cursor.x
    };
    let mut y = if cursor.y + menu_h > bottom {
        cursor.y - menu_h
    } else {
        cursor.y
    };

    x = x.max(left).min(max_x);
    y = y.max(top).min(max_y);

    let _ = win.set_position(PhysicalPosition {
        x: x as i32,
        y: y as i32,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn a_blur_before_the_first_focus_keeps_the_menu() {
        let t0 = Instant::now();
        let mut focus = TrayMenuFocus::default();
        focus.shown(t0);
        assert!(!focus.blurred(t0 + ms(10)), "within the show grace");
        assert!(!focus.blurred(t0 + ms(1000)), "never focused");
    }

    #[test]
    fn a_blur_right_after_focus_is_ignored_until_the_grace_ends() {
        let t0 = Instant::now();
        let mut focus = TrayMenuFocus::default();
        focus.shown(t0);
        focus.focused(t0 + ms(5));
        assert!(!focus.blurred(t0 + ms(6)), "the shell's focus flicker");
        assert!(!focus.blurred(t0 + ms(254)));
        assert!(focus.blurred(t0 + ms(255)));
    }

    #[test]
    fn a_focus_shortly_after_showing_shortens_the_grace() {
        let t0 = Instant::now();
        let mut focus = TrayMenuFocus::default();
        focus.shown(t0);
        focus.focused(t0 + ms(100));
        assert!(
            focus.blurred(t0 + ms(400)),
            "the focus grace replaces the show grace"
        );
    }

    #[test]
    fn a_dismissal_waits_for_the_next_focus() {
        let t0 = Instant::now();
        let mut focus = TrayMenuFocus::default();
        focus.shown(t0);
        focus.focused(t0 + ms(10));
        assert!(focus.blurred(t0 + ms(1000)));
        assert!(!focus.blurred(t0 + ms(1001)));

        focus.focused(t0 + ms(2000));
        assert!(focus.blurred(t0 + ms(3000)));
    }

    #[test]
    fn the_debug_menu_stays_open_until_shown_normally() {
        let t0 = Instant::now();
        let mut focus = TrayMenuFocus::default();
        focus.shown_persistent(t0);
        focus.focused(t0 + ms(10));
        assert!(!focus.blurred(t0 + ms(1000)));

        focus.shown(t0 + ms(2000));
        assert!(
            !focus.blurred(t0 + ms(3000)),
            "showing again waits for a new focus"
        );
        focus.focused(t0 + ms(3000));
        assert!(focus.blurred(t0 + ms(4000)));
    }
}
