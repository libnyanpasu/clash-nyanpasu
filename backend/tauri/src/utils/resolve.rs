use crate::{
    config::{
        Config, IVerge,
        nyanpasu::{ClashCore, WindowState},
    },
    core::{storage::Storage, tray::proxies, *},
    log_err,
    utils::init,
    window::AppWindow,
};
use anyhow::Result;
use semver::Version;
use serde_yaml::Mapping;
use std::{
    net::TcpListener,
    sync::atomic::{AtomicU16, Ordering},
};
use tauri::{App, AppHandle, Emitter, Listener, Manager, async_runtime::block_on};
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
        create_main_window(app.app_handle());
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

/// Main window implementation
struct MainWindow;

impl AppWindow for MainWindow {
    fn label(&self) -> &str {
        "main"
    }

    fn title(&self) -> &str {
        "Clash Nyanpasu"
    }

    fn url(&self) -> &str {
        "/"
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
