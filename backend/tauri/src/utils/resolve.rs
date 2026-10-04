use crate::{
    client::{NyanpasuClient, application_workflow::startup::StartupOutcome},
    core::tray::proxies,
    log_err,
    window::{AppWindow, WindowConfig, WindowParamsBuilder, WindowReadyEvent},
};
use anyhow::Result;
use nyanpasu_config::{
    application::{ClashCore, TrayMenuCloseBehavior},
    state::window::WindowState,
};
use semver::Version;
use serde::Serialize;
use snafu::{ResultExt, Snafu, ensure};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tauri::{App, AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use tauri_specta::Event;

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

#[cfg(target_os = "macos")]
fn set_window_controls_pos(
    window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    x: f64,
    y: f64,
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

    let title_bar_frame_height = button_height + y;
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
        rect.origin.x = x + (i as f64 * space_between);
        unsafe {
            button.setFrameOrigin(rect.origin);
        }
    }
    Ok(())
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    #[cfg(any(windows, target_os = "macos"))]
    let ready_app_handle = app.app_handle().clone();
    WindowReadyEvent::listen(app, move |event| {
        let label = &event.payload.label;
        tracing::debug!("Window '{}' is ready", label);
        #[cfg(windows)]
        if label == crate::consts::MAIN_WINDOW_LABEL {
            let handle = ready_app_handle.clone();
            log_err!(ready_app_handle.run_on_main_thread(move || {
                if let Some(window) = handle.get_webview_window(crate::consts::MAIN_WINDOW_LABEL) {
                    crate::window::log_main_window_geometry(&window, "frontend_ready_after_show");
                }
            }));
        }
        #[cfg(target_os = "macos")]
        if label == crate::consts::MAIN_WINDOW_LABEL {
            log_err!(ready_app_handle.run_on_main_thread(|| {
                crate::utils::dock::macos::show_dock_icon();
            }));
        }
    });

    #[cfg(any(windows, target_os = "linux"))]
    log::trace!("init system tray");
    #[cfg(any(windows, target_os = "linux"))]
    crate::core::tray::icon::resize_images(crate::utils::help::get_max_scale_factor()); // generate latest cache icon by current scale factor

    {
        let client = app.state::<crate::client::NyanpasuClient>();
        // TODO(startup): resolve_setup needs restructuring; startup_reconcile
        // should not block setup. See
        // docs/plan/2026-09-28-workflow-lifecycle-simplification.md §9.
        let report = tauri::async_runtime::block_on(client.startup_reconcile());
        if let Some(observation) = &report.observation {
            log::info!(
                target: "app",
                "startup reconcile {} observed: desired {:?}, service {:?}, runtime {:?}",
                report.operation_id,
                observation.desired,
                observation.service,
                observation.runtime
            );
        }
        match &report.outcome {
            StartupOutcome::Ready => {
                log::info!(target: "app", "startup reconcile {}: ready", report.operation_id)
            }
            outcome => log::warn!(
                target: "app",
                "startup reconcile {}: {outcome:?}",
                report.operation_id
            ),
        }
        // Even an unsettled startup lets them run: what they change queues
        // behind the startup command.
        log_err!(client.start_background_sources());
    }

    log::trace!("init clash connection connector");
    log_err!(crate::core::clash::setup(app));

    log_err!(tauri::async_runtime::block_on(
        app.state::<crate::client::NyanpasuClient>()
            .start_clash_streams()
    ));

    let silent_start = app
        .state::<NyanpasuClient>()
        .app_config_snapshot()
        .enable_silent_start;
    if !silent_start {
        create_window(app.app_handle());
        spawn_window_ready_timeout(app.app_handle().clone());
    }

    // test job
    proxies::setup_proxies(app.app_handle());
    crate::core::storage::register_web_storage_listener(app.app_handle());
}

/// Spawn a background task that shows the main window after a timeout if the
/// frontend never called show() — guards against a crashed or hung webview
/// leaving the window permanently invisible.
fn spawn_window_ready_timeout(app_handle: AppHandle) {
    const TIMEOUT_SECS: u64 = 10;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT_SECS)).await;
        let inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            if let Some(win) = inner.get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
                && !win.is_visible().unwrap_or(true)
            {
                tracing::warn!(
                    "Main window still hidden after {}s timeout, showing as fallback",
                    TIMEOUT_SECS
                );
                let _ = win.show();
                let _ = win.set_focus();
                #[cfg(target_os = "macos")]
                crate::utils::dock::macos::show_dock_icon();
            }
        });
    });
}

/// Main window implementation (new UI)
struct MainWindow;

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
            .visible_on_create(false)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self, app_handle: &AppHandle) -> Option<WindowState> {
        app_handle
            .try_state::<NyanpasuClient>()?
            .main_window_geometry()
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
struct EditorWindow {
    label: String,
    window_type: EditorWindowType,
}

impl EditorWindow {
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

        crate::window::build_url_with_params(
            &format!("{}/{}", self.url(), editor_type),
            query_params.as_ref(),
        )
    }

    fn config(&self) -> WindowConfig {
        let singleton = matches!(self.window_type, EditorWindowType::CssEditor);
        WindowConfig::new()
            .singleton(singleton)
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 500.0)
            .center(true)
    }

    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        // EditorWindow does not remember window state
        None
    }
}

/// create main window
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_main_window(app_handle: &AppHandle) {
    log_err!(MainWindow.create(app_handle));
}

/// close main window
pub fn close_main_window(app_handle: &AppHandle) {
    MainWindow.close(app_handle);
}

/// is main window open
pub fn is_main_window_open(app_handle: &AppHandle) -> bool {
    MainWindow.is_open(app_handle)
}

pub async fn save_main_window_state_async(
    app_handle: &AppHandle,
    _save_to_file: bool,
) -> Result<()> {
    if let Some(geometry) = MainWindow.capture_state(app_handle)? {
        app_handle
            .state::<crate::client::NyanpasuClient>()
            .save_main_window_geometry(geometry)
            .await?;
    }
    Ok(())
}

/// Create window based on window_type config
/// This is the primary function to use when opening window from tray, etc.
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_window(app_handle: &AppHandle) {
    create_main_window(app_handle)
}

/// Close the currently active window based on window_type config
pub fn close_window(app_handle: &AppHandle) {
    close_main_window(app_handle)
}

/// Check if the configured window is open
pub fn is_window_open(app_handle: &AppHandle) -> bool {
    is_main_window_open(app_handle)
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
struct TrayMenuWindow;

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
            .visible_on_create(false)
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
}

/// Register a window event handler that hides or closes the tray menu window on
/// focus loss, as [`TrayMenuFocus`] decides.
fn setup_tray_menu_focus_handler(win: &tauri::WebviewWindow<tauri::Wry>) {
    let win_clone = win.clone();
    win.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(true) => {
            win_clone.state::<TrayMenuWindowController>().focused();
        }
        tauri::WindowEvent::Focused(false) => {
            if win_clone.state::<TrayMenuWindowController>().blurred() {
                let close_behavior = win_clone
                    .try_state::<NyanpasuClient>()
                    .map(|client| client.app_config_snapshot().tray_menu_close_behavior)
                    .unwrap_or_default();
                match close_behavior {
                    TrayMenuCloseBehavior::Close => {
                        let _ = win_clone.close();
                    }
                    TrayMenuCloseBehavior::Hide => {
                        let _ = win_clone.hide();
                    }
                }
            }
        }
        _ => {}
    });
}

/// Create a persistent tray menu window for debugging.
pub fn create_debug_tray_menu_window(app_handle: &AppHandle) -> Result<()> {
    app_handle
        .state::<TrayMenuWindowController>()
        .shown_persistent();

    let params = WindowParamsBuilder::new()
        .param("persistent", "true")
        .build();
    let result = TrayMenuWindow.create_with_params(app_handle, params)?;

    let win = app_handle
        .get_webview_window(crate::consts::TRAY_MENU_WINDOW_LABEL)
        .ok_or_else(|| anyhow::anyhow!("failed to get tray menu window"))?;

    if result.is_new {
        setup_tray_menu_focus_handler(&win);
    }

    let _ = win.show();
    let _ = win.set_focus();

    Ok(())
}

/// Show the webview tray menu window near the given cursor position.
pub fn show_tray_menu_window(
    app_handle: &AppHandle,
    cursor: tauri::PhysicalPosition<f64>,
) -> Result<()> {
    use tauri::{Manager, PhysicalPosition};

    app_handle.state::<TrayMenuWindowController>().shown();

    let win = match app_handle.get_webview_window(crate::consts::TRAY_MENU_WINDOW_LABEL) {
        Some(existing) => existing,
        None => {
            let result = TrayMenuWindow.create_with_params(app_handle, None)?;
            let win = app_handle
                .get_webview_window(crate::consts::TRAY_MENU_WINDOW_LABEL)
                .ok_or_else(|| anyhow::anyhow!("failed to get tray menu window after creation"))?;
            if result.is_new {
                setup_tray_menu_focus_handler(&win);
            }
            win
        }
    };

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
    let _ = win.show();
    let _ = win.set_focus();

    Ok(())
}

/// Create editor window with window_type and optional uid
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_editor_window(
    app_handle: &AppHandle,
    window_type: EditorWindowType,
    uid: Option<&str>,
) -> Result<()> {
    let window = match &window_type {
        EditorWindowType::Profile => {
            let uid = uid.ok_or_else(|| anyhow::anyhow!("uid required for Profile editor"))?;
            EditorWindow::profile(uid)
        }
        EditorWindowType::CssEditor => EditorWindow::css_editor(),
    };
    let mut builder = WindowParamsBuilder::new().param("type", window_type.type_str());
    if let Some(u) = uid {
        builder = builder.param("uid", u);
    }
    window.create_with_params(app_handle, builder.build())?;
    Ok(())
}

/// Close editor window by window_type and optional uid
#[allow(dead_code)]
pub fn close_editor_window(
    app_handle: &AppHandle,
    window_type: &EditorWindowType,
    uid: Option<&str>,
) {
    let window = match window_type {
        EditorWindowType::Profile => {
            let Some(uid) = uid else { return };
            EditorWindow::profile(uid)
        }
        EditorWindowType::CssEditor => EditorWindow::css_editor(),
    };
    window.close_by_label(app_handle, window.label());
}

/// Check if editor window with window_type (and optional uid) is open
#[allow(dead_code)]
pub fn is_editor_window_open(
    app_handle: &AppHandle,
    window_type: &EditorWindowType,
    uid: Option<&str>,
) -> bool {
    let window = match window_type {
        EditorWindowType::Profile => {
            let Some(uid) = uid else { return false };
            EditorWindow::profile(uid)
        }
        EditorWindowType::CssEditor => EditorWindow::css_editor(),
    };
    app_handle.get_webview_window(window.label()).is_some()
}

/// A failure of asking a core binary for its version.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreVersionError {
    #[snafu(display("could not run the {core} core to read its version: {source}"))]
    RunCoreVersion {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: tauri_plugin_shell::Error,
    },
    #[snafu(display("the {core} core failed when asked for its version"))]
    CoreVersionExit {
        #[specta(type = String)]
        core: ClashCore,
    },
    #[snafu(display("the {core} core did not report a version"))]
    CoreVersionNotReported {
        #[specta(type = String)]
        core: ClashCore,
    },
}

/// resolve core version
// TODO: use enum instead
pub async fn resolve_core_version(
    app_handle: &AppHandle,
    core_type: &ClashCore,
) -> Result<String, CoreVersionError> {
    let shell = app_handle.shell();
    let core = core_type.binary_name();
    let core_type = *core_type;
    log::debug!(target: "app", "check config in `{core}`");
    let cmd = match core_type {
        ClashCore::ClashPremium | ClashCore::Mihomo | ClashCore::MihomoAlpha | ClashCore::Meow => {
            shell
                .sidecar(core)
                .context(RunCoreVersionSnafu { core: core_type })?
                .args(["-v"])
        }
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => shell
            .sidecar(core)
            .context(RunCoreVersionSnafu { core: core_type })?
            .args(["-V"]),
    };
    let out = cmd
        .output()
        .await
        .context(RunCoreVersionSnafu { core: core_type })?;
    ensure!(
        out.status.success(),
        CoreVersionExitSnafu { core: core_type }
    );
    let out = String::from_utf8_lossy(&out.stdout);
    log::trace!(target: "app", "get core version: {out:?}");
    let out = out.trim().split(' ').collect::<Vec<&str>>();
    for item in out {
        log::debug!(target: "app", "check item: {item}");
        if item.starts_with('v')
            || item.starts_with('n')
            || item.starts_with("alpha")
            || Version::parse(item).is_ok()
        {
            match core_type {
                ClashCore::ClashRs => return Ok(format!("v{}", item)),
                _ => return Ok(item.to_string()),
            }
        }
    }
    CoreVersionNotReportedSnafu { core: core_type }.fail()
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
