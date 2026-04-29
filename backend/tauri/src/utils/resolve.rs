use crate::{
    config::{
        Config, IVerge,
        nyanpasu::{ClashCore, TrayMenuCloseBehavior, WindowState},
    },
    core::{storage::Storage, tray::proxies, *},
    log_err,
    utils::init,
    window::{AppWindow, ReactAppMountedEvent, WindowConfig, WindowParamsBuilder},
};
use anyhow::Result;
use semver::Version;
use serde_yaml::Mapping;
use std::{
    net::TcpListener,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{App, AppHandle, Emitter, Listener, Manager, async_runtime::block_on};
use tauri_plugin_shell::ShellExt;
use tauri_specta::Event;

static OPEN_WINDOWS_COUNTER: AtomicU16 = AtomicU16::new(0);
static TRAY_MENU_PERSISTENT: AtomicBool = AtomicBool::new(false);
/// Set to true only after the window has received Focused(true) at least once.
/// Prevents spurious Focused(false) events during window creation from triggering
/// hide/close before the user has ever seen the window.
static TRAY_MENU_READY: AtomicBool = AtomicBool::new(false);
/// Ignore focus-loss events until this unix timestamp in milliseconds.
///
/// Windows can emit Focused(true) immediately followed by Focused(false) while
/// the shell is still finishing the tray right-click interaction. Without a
/// short guard window, the webview tray menu flashes and is hidden/closed
/// before it can be used.
static TRAY_MENU_IGNORE_BLUR_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

const TRAY_MENU_SHOW_BLUR_GRACE_MS: u64 = 750;
const TRAY_MENU_FOCUS_BLUR_GRACE_MS: u64 = 250;

pub fn is_window_opened() -> bool {
    OPEN_WINDOWS_COUNTER.load(Ordering::Acquire) == 0 // 0 means no window open or windows is initialized
}

pub fn reset_window_open_counter() {
    OPEN_WINDOWS_COUNTER.store(0, Ordering::Release);
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn ignore_tray_menu_blur_for(duration_ms: u64) {
    TRAY_MENU_IGNORE_BLUR_UNTIL_MS.store(
        unix_time_millis().saturating_add(duration_ms),
        Ordering::Release,
    );
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

pub fn find_unused_port() -> Result<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let port = listener.local_addr()?.port();
            Ok(port)
        }
        Err(_) => {
            let port = Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port());
            log::warn!(target: "app", "use default port: {port}");
            Ok(port)
        }
    }
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    #[cfg(target_os = "macos")]
    let app_handle = app.app_handle().clone();
    ReactAppMountedEvent::listen(app, move |_| {
        tracing::debug!("Frontend React App is mounted, reset open window counter");
        reset_window_open_counter();
        #[cfg(target_os = "macos")]
        log_err!(app_handle.run_on_main_thread(move || {
            crate::utils::dock::macos::show_dock_icon();
        }));
    });

    handle::Handle::global().init(app.app_handle().clone());
    crate::consts::setup_app_handle(app.app_handle().clone());

    log_err!(init::init_resources());
    log_err!(init::init_service());

    // 处理随机端口
    let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);

    let mut port = Config::verge()
        .latest()
        .verge_mixed_port
        .unwrap_or(Config::clash().data().get_mixed_port());

    if enable_random_port {
        port = find_unused_port().unwrap_or(
            Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port()),
        );
    }

    Config::verge().data().patch_config(IVerge {
        verge_mixed_port: Some(port),
        ..IVerge::default()
    });
    let _ = Config::verge().data().save_file();
    let mut mapping = Mapping::new();
    mapping.insert("mixed-port".into(), port.into());
    Config::clash().data().patch_config(mapping);
    let _ = Config::clash().latest().prepare_external_controller_port();
    let _ = Config::clash().data().save_config();

    // 启动核心
    log::trace!("init config");
    log_err!(Config::init_config());

    log::trace!("init storage");
    log_err!(crate::core::storage::setup(app));

    log::trace!("launch core");
    log_err!(CoreManager::global().init());

    log::trace!("init clash connection connector");
    log_err!(crate::core::clash::setup(app));

    log::trace!("init widget manager");
    log_err!(tauri::async_runtime::block_on(async {
        crate::widget::setup(app, {
            let manager = app.state::<crate::core::clash::ws::ClashConnectionsConnector>();
            manager.subscribe()
        })
        .await
    }));

    #[cfg(any(windows, target_os = "linux"))]
    log::trace!("init system tray");
    #[cfg(any(windows, target_os = "linux"))]
    tray::icon::resize_images(crate::utils::help::get_max_scale_factor()); // generate latest cache icon by current scale factor
    let app_handle = app.app_handle().clone();
    app.listen("update_systray", move |_| {
        // Fix the GTK should run on main thread issue
        let app_handle_clone = app_handle.clone();
        log_err!(app_handle.run_on_main_thread(move || {
            log_err!(
                tray::Tray::update_systray(&app_handle_clone),
                "failed to update systray"
            );
        }));
    });
    log_err!(app.emit("update_systray", ()));

    let silent_start = { Config::verge().data().enable_silent_start };
    if !silent_start.unwrap_or(false) {
        create_window(app.app_handle());
    }

    log_err!(sysopt::Sysopt::global().init_launch());
    log_err!(sysopt::Sysopt::global().init_sysproxy());

    log_err!(handle::Handle::update_systray_part());
    log_err!(hotkey::Hotkey::global().init(app.app_handle().clone()));

    // setup jobs
    log::trace!("setup jobs");
    {
        let storage = app.state::<Storage>();
        let storage = (*storage).clone();
        log_err!(crate::core::tasks::setup(app, storage));
    }

    // test job
    proxies::setup_proxies();
    crate::core::storage::register_web_storage_listener(app.app_handle());
}

/// reset system proxy
pub fn resolve_reset() {
    log_err!(sysopt::Sysopt::global().reset_sysproxy());
    log_err!(block_on(CoreManager::global().stop_core()));
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
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        Config::verge().latest().window_size_state.clone()
    }

    fn set_window_state(&self, state: Option<WindowState>) {
        Config::verge().data().patch_config(IVerge {
            window_size_state: state,
            ..IVerge::default()
        });
    }
}

/// Editor window
struct EditorWindow {
    label: String,
}

impl EditorWindow {
    fn new(uid: &str) -> Self {
        Self {
            label: format!("{}-{}", crate::consts::EDITOR_WINDOW_LABEL, uid),
        }
    }
}

impl AppWindow for EditorWindow {
    fn label(&self) -> &str {
        &self.label
    }

    fn title(&self) -> &str {
        &crate::consts::APP_EDITOR_NAME
    }

    fn url(&self) -> &str {
        "/editor"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(false) // Allow multiple editor windows with different uids
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        // EditorWindow does not remember window state
        None
    }

    fn set_window_state(&self, _state: Option<WindowState>) {
        // EditorWindow does not remember window state
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

pub fn save_main_window_state(app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
    MainWindow.save_state(app_handle, save_to_file)
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

/// Save window state for the configured window type
pub fn save_window_state(app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
    save_main_window_state(app_handle, save_to_file)
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

    fn get_window_state(&self) -> Option<WindowState> {
        None
    }

    fn set_window_state(&self, _state: Option<WindowState>) {}
}

/// Register a window event handler that hides or closes the tray menu window on
/// focus loss, unless TRAY_MENU_PERSISTENT is set to true.
///
/// TRAY_MENU_READY guards against spurious Focused(false) events that fire
/// during window creation / OS tray interaction before the user sees the window.
fn setup_tray_menu_focus_handler(win: &tauri::WebviewWindow<tauri::Wry>) {
    let win_clone = win.clone();
    win.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(true) => {
            TRAY_MENU_READY.store(true, Ordering::Release);
            ignore_tray_menu_blur_for(TRAY_MENU_FOCUS_BLUR_GRACE_MS);
        }
        tauri::WindowEvent::Focused(false) => {
            let ignore_blur_until = TRAY_MENU_IGNORE_BLUR_UNTIL_MS.load(Ordering::Acquire);
            if unix_time_millis() < ignore_blur_until {
                return;
            }

            if !TRAY_MENU_PERSISTENT.load(Ordering::Acquire)
                && TRAY_MENU_READY.load(Ordering::Acquire)
            {
                TRAY_MENU_READY.store(false, Ordering::Release);
                let close_behavior = Config::verge()
                    .latest()
                    .tray_menu_close_behavior
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
    TRAY_MENU_PERSISTENT.store(true, Ordering::Release);
    ignore_tray_menu_blur_for(TRAY_MENU_SHOW_BLUR_GRACE_MS);

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

    TRAY_MENU_PERSISTENT.store(false, Ordering::Release);
    TRAY_MENU_READY.store(false, Ordering::Release);
    ignore_tray_menu_blur_for(TRAY_MENU_SHOW_BLUR_GRACE_MS);

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

/// Create editor window with uid
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_editor_window(app_handle: &AppHandle, uid: &str) -> Result<()> {
    let editor_window = EditorWindow::new(uid);
    let params = WindowParamsBuilder::new().param("uid", uid).build();
    editor_window.create_with_params(app_handle, params)?;
    Ok(())
}

/// Close editor window by uid
pub fn close_editor_window(app_handle: &AppHandle, uid: &str) {
    let editor_window = EditorWindow::new(uid);
    editor_window.close_by_label(app_handle, &editor_window.label());
}

/// Check if editor window with uid is open
pub fn is_editor_window_open(app_handle: &AppHandle, uid: &str) -> bool {
    let editor_window = EditorWindow::new(uid);
    app_handle
        .get_webview_window(editor_window.label())
        .is_some()
}

/// resolve core version
// TODO: use enum instead
pub async fn resolve_core_version(app_handle: &AppHandle, core_type: &ClashCore) -> Result<String> {
    let shell = app_handle.shell();
    let core = core_type.clone().to_string();
    log::debug!(target: "app", "check config in `{core}`");
    let cmd = match core_type {
        ClashCore::ClashPremium | ClashCore::Mihomo | ClashCore::MihomoAlpha => {
            shell.sidecar(core)?.args(["-v"])
        }
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => shell.sidecar(core)?.args(["-V"]),
    };
    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("failed to get core version"));
    }
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
    Err(anyhow::anyhow!("failed to get core version"))
}
