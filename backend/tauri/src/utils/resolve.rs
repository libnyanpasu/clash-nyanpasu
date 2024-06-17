use crate::{
    config::{
        nyanpasu::{ClashCore, WindowState},
        Config, IVerge,
    },
    core::{
        tasks::{jobs::ProfilesJobGuard, JobsManager},
        tray::proxies,
        *,
    },
    log_err, trace_err,
    utils::init,
};
use anyhow::Result;
use semver::Version;
use serde_yaml::Mapping;
use std::net::TcpListener;
use tauri::{api::process::Command, App, AppHandle, Manager};

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

    handle::Handle::global().init(app.app_handle());

    log_err!(init::init_resources());
    #[cfg(target_os = "windows")]
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
    let _ = Config::clash().data().save_config();

    // 启动核心
    log::trace!("init config");
    log_err!(Config::init_config());

    log::trace!("launch core");
    log_err!(CoreManager::global().init());

    log::trace!("init system tray");
    log_err!(tray::Tray::update_systray(&app.app_handle()));

    let silent_start = { Config::verge().data().enable_silent_start };
    if !silent_start.unwrap_or(false) {
        create_window(&app.app_handle());
    }

    log_err!(sysopt::Sysopt::global().init_launch());
    log_err!(sysopt::Sysopt::global().init_sysproxy());

    log_err!(handle::Handle::update_systray_part());
    log_err!(hotkey::Hotkey::global().init(app.app_handle()));

    // setup jobs
    log_err!(JobsManager::global_register());
    // init task manager
    log_err!(ProfilesJobGuard::global().lock().init());

    // test job
    proxies::setup_proxies();
}

/// reset system proxy
pub fn resolve_reset() {
    log_err!(sysopt::Sysopt::global().reset_sysproxy());
    log_err!(CoreManager::global().stop_core());
}

/// create main window
pub fn create_window(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_window("main") {
        trace_err!(window.unminimize(), "set win unminimize");
        trace_err!(window.show(), "set win visible");
        trace_err!(window.set_focus(), "set win focus");
        return;
    }

    let mut builder = tauri::window::WindowBuilder::new(
        app_handle,
        "main".to_string(),
        tauri::WindowUrl::App("/dashboard".into()),
    )
    .title("Clash Nyanpasu")
    .fullscreen(false)
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

    #[cfg(target_os = "windows")]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        use window_shadows::set_shadow;

        match builder
            .decorations(false)
            .transparent(true)
            .visible(false)
            .build()
        {
            Ok(win) => {
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
                trace_err!(set_shadow(&win, true), "set win shadow");
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

                // log::trace!("try to create window");
                // let app_handle = app_handle.clone();

                // 加点延迟避免界面闪一下
                //     tauri::async_runtime::spawn(async move {
                //         // sleep(Duration::from_millis(888)).await;

                //         if let Some(window) = app_handle.get_window("main") {
                //             trace_err!(set_shadow(&window, true), "set win shadow");
                //             trace_err!(window.show(), "set win visible");
                //             trace_err!(window.unminimize(), "set win unminimize");
                //             trace_err!(window.set_focus(), "set win focus");
                //         } else {
                //             log::error!(target: "app", "failed to create window, get_window is None")
                //         }
                //     });
                #[cfg(debug_assertions)]
                {
                    win.open_devtools();
                }
            }
            Err(err) => log::error!(target: "app", "failed to create window, {err}"),
        }
    }

    #[cfg(target_os = "macos")]
    {
        match builder
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .build()
        {
            Ok(win) => {
                #[cfg(debug_assertions)]
                {
                    win.open_devtools();
                }
            }
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    crate::log_err!(builder.decorations(true).transparent(false).build());

    #[cfg(target_os = "windows")]
    {
        use webview2_com_bridge::{
            webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings6,
            windows::core::Interface,
        };
        app_handle
            .get_window("main")
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
    if let Some(window) = app_handle.get_window("main") {
        trace_err!(window.close(), "close window");
    }
}

/// is window open
pub fn is_window_open(app_handle: &AppHandle) -> bool {
    app_handle.get_window("main").is_some()
}

pub fn save_window_state(app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
    let win = app_handle
        .get_window("main")
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
pub fn resolve_core_version(core_type: &ClashCore) -> Result<String> {
    let core = core_type.clone().to_string();
    log::debug!(target: "app", "check config in `{core}`");
    let cmd = match core_type {
        ClashCore::ClashPremium | ClashCore::Mihomo | ClashCore::MihomoAlpha => {
            Command::new_sidecar(core)?.args(["-v"])
        }
        ClashCore::ClashRs => Command::new_sidecar(core)?.args(["-V"]),
    };
    let out = cmd.output()?;
    log::debug!(target: "app", "get core version: {:?}", out);
    if !out.status.success() {
        return Err(anyhow::anyhow!("failed to get core version"));
    }
    let out = out.stdout.trim().split(' ').collect::<Vec<&str>>();
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
