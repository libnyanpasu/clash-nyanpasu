use crate::{
    config::{
        nyanpasu::{ClashCore, WindowState},
        Config, IVerge,
    },
    core::{storage::Storage, tray::proxies, *},
    log_err, trace_err,
    utils::init,
};
use anyhow::Result;
use semver::Version;
use serde_yaml::Mapping;
use std::{
    net::TcpListener,
    sync::atomic::{AtomicU16, Ordering},
};
use tauri::{async_runtime::block_on, App, AppHandle, Emitter, Listener, Manager};
use tauri_plugin_shell::ShellExt;
static OPEN_WINDOWS_COUNTER: AtomicU16 = AtomicU16::new(0);

pub fn is_window_opened() -> bool {
    OPEN_WINDOWS_COUNTER.load(Ordering::Acquire) == 0 // 0 means no window open or windows is initialized
}

pub fn reset_window_open_counter() {
    OPEN_WINDOWS_COUNTER.store(0, Ordering::Release);
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
        .standardWindowButton(NSWindowButton::NSWindowCloseButton)
        .ok_or(anyhow::anyhow!("failed to get close button"))?;
    let miniaturize = window
        .standardWindowButton(NSWindowButton::NSWindowMiniaturizeButton)
        .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
    let zoom = window
        .standardWindowButton(NSWindowButton::NSWindowZoomButton)
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
            log::warn!(target: "app", "use default port: {}", port);
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
    app.listen("react_app_mounted", move |_| {
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

/// create main window
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        tracing::debug!("main window is already opened, try to show it");
        if OPEN_WINDOWS_COUNTER.load(Ordering::Acquire) == 0 {
            trace_err!(window.unminimize(), "set win unminimize");
            trace_err!(window.show(), "set win visible");
            trace_err!(window.set_focus(), "set win focus");
        }
        return;
    }

    let always_on_top = {
        *Config::verge()
            .latest()
            .always_on_top
            .as_ref()
            .unwrap_or(&false)
    };

    tracing::debug!("create main window...");
    let mut builder = tauri::WebviewWindowBuilder::new(
        app_handle,
        "main".to_string(),
        tauri::WebviewUrl::App("/".into()),
    )
    .title("Clash Nyanpasu")
    .fullscreen(false)
    .always_on_top(always_on_top)
    .min_inner_size(600.0, 520.0);

    let win_state = &Config::verge().latest().window_size_state.clone();
    match win_state {
        Some(_) => {
            builder = builder.inner_size(800., 800.).position(0., 0.);
        }
        _ => {
            #[cfg(target_os = "windows")]
            {
                builder = builder.inner_size(800.0, 636.0).center();
            }

            #[cfg(target_os = "macos")]
            {
                builder = builder.inner_size(800.0, 642.0).center();
            }

            #[cfg(target_os = "linux")]
            {
                builder = builder.inner_size(800.0, 642.0).center();
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
    let win_res = builder
        .decorations(true)
        .hidden_title(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .build();
    #[cfg(target_os = "linux")]
    let win_res = builder.decorations(true).transparent(false).build();

    match win_res {
        Ok(win) => {
            use tauri::{PhysicalPosition, PhysicalSize};

            if win_state.is_some() {
                let state = win_state.as_ref().unwrap();
                win.set_position(PhysicalPosition {
                    x: state.x,
                    y: state.y,
                })
                .unwrap();
                win.set_size(PhysicalSize {
                    width: state.width,
                    height: state.height,
                })
                .unwrap();
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
                if let Some(webview_window) = win.get_webview_window("main") {
                    webview_window.open_devtools();
                }
            }

            #[cfg(target_os = "macos")]
            {
                tracing::trace!("setup traffic lights pos");
                let mtm = objc2_foundation::MainThreadMarker::new().unwrap();
                crate::window::macos::setup_traffic_lights_pos(win.clone(), (26.0, 26.0), mtm);
            }

            OPEN_WINDOWS_COUNTER.fetch_add(1, Ordering::Release);
        }
        Err(err) => {
            log::error!(target: "app", "failed to create window, {err:?}");
            if let Some(win) = app_handle.get_webview_window("main") {
                // Cleanup window if failed to create, it's a workaround for tauri bug
                log_err!(
                    win.destroy(),
                    "occur error when close window while failed to create"
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6;
        use windows_core::Interface;

        app_handle
            .get_webview_window("main")
            .unwrap()
            .with_webview(|webview| unsafe {
                let settings = webview
                    .controller()
                    .CoreWebView2()
                    .unwrap()
                    .Settings()
                    .unwrap();
                let settings: ICoreWebView2Settings6 =
                    settings.cast::<ICoreWebView2Settings6>().unwrap();
                settings.SetIsSwipeNavigationEnabled(false).unwrap();
            })
            .unwrap();
    }
}

/// close main window
pub fn close_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        trace_err!(window.close(), "close window");
        reset_window_open_counter()
    }
}

/// is window open
pub fn is_window_open(app_handle: &AppHandle) -> bool {
    app_handle.get_webview_window("main").is_some()
}

pub fn save_window_state(app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
    let win = app_handle
        .get_webview_window("main")
        .ok_or(anyhow::anyhow!("failed to get window"))?;
    let current_monitor = win.current_monitor()?;
    let verge = Config::verge();
    let mut verge = verge.latest();
    match current_monitor {
        Some(_) => {
            let previous_state = verge.window_size_state.clone().unwrap_or_default();
            let mut state = WindowState {
                maximized: win.is_maximized()?,
                fullscreen: win.is_fullscreen()?,
                ..previous_state
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
            verge.window_size_state = Some(state);
        }
        None => {
            verge.window_size_state = None;
        }
    }

    if save_to_file {
        verge.save_file()?;
    }

    Ok(())
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
    log::trace!(target: "app", "get core version: {:?}", out);
    let out = out.trim().split(' ').collect::<Vec<&str>>();
    for item in out {
        log::debug!(target: "app", "check item: {}", item);
        if item.starts_with('v')
            || item.starts_with('n')
            || item.starts_with("alpha")
            || Version::parse(item).is_ok()
        {
            return Ok(item.to_string());
        }
    }
    Err(anyhow::anyhow!("failed to get core version"))
}
