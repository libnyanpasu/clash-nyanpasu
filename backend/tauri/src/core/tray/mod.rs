use crate::{
    config::{nyanpasu::ClashCore, Config},
    feat, ipc, log_err, trace_err,
    utils::{help, resolve},
};
use anyhow::Result;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};
use tracing_attributes::instrument;

pub mod icon;
pub mod proxies;
pub use self::icon::on_scale_factor_changed;
use self::proxies::SystemTrayMenuProxiesExt;
mod utils;

const TRAY_ID: &str = "main";

struct TrayState<R: Runtime> {
    menu: Mutex<Menu<R>>,
}

pub struct Tray {}

impl Tray {
    #[instrument(skip(app_handle))]
    pub fn tray_menu<R: Runtime>(app_handle: &AppHandle<R>) -> Result<Menu<R>> {
        let version = env!("NYANPASU_VERSION");
        let core = {
            *Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::default())
        };
        let mut menu = MenuBuilder::new(app_handle)
            .text("open_window", t!("tray.dashboard"))
            .setup_proxies(app_handle)? // Setup the proxies menu
            .separator()
            .check("rule_mode", t!("tray.rule_mode"))
            .check("global_mode", t!("tray.global_mode"))
            .check("direct_mode", t!("tray.direct_mode"));
        if core == ClashCore::ClashPremium {
            menu = menu.check("script_mode", t!("tray.script_mode"));
        }
        menu = menu
            .separator()
            .check("system_proxy", t!("tray.system_proxy"))
            .check("tun_mode", t!("tray.tun_mode"))
            .separator()
            .text("copy_env_sh", t!("tray.copy_env.sh"))
            .text("copy_env_cmd", t!("tray.copy_env.cmd"))
            .text("copy_env_ps", t!("tray.copy_env.ps"))
            .item(
                &SubmenuBuilder::new(app_handle, t!("tray.open_dir.menu"))
                    .text("open_app_config_dir", t!("tray.open_dir.app_config_dir"))
                    .text("open_app_data_dir", t!("tray.open_dir.app_data_dir"))
                    .text("open_core_dir", t!("tray.open_dir.core_dir"))
                    .text("open_logs_dir", t!("tray.open_dir.log_dir"))
                    .build()?,
            )
            .item(
                &SubmenuBuilder::new(app_handle, t!("tray.more.menu"))
                    .text("restart_clash", t!("tray.more.restart_clash"))
                    .text("restart_app", t!("tray.more.restart_app"))
                    .item(
                        &MenuItemBuilder::new(format!("Version {}", version))
                            .id("app_version")
                            .enabled(false)
                            .build(app_handle)?,
                    )
                    .build()?,
            )
            .separator()
            .item(
                &MenuItemBuilder::new(t!("tray.quit"))
                    .id("quit")
                    .accelerator("CmdOrControl+Q")
                    .build(app_handle)?,
            );
        Ok(menu.build()?)
    }

    #[instrument(skip(app_handle))]
    pub fn update_systray(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        let mut tray = app_handle.tray_by_id(TRAY_ID);
        if cfg!(target_os = "linux") && tray.is_some() {
            app_handle.remove_tray_by_id(TRAY_ID);
            tray = None;
        }
        let menu = Tray::tray_menu(app_handle)?;
        let tray = match tray {
            None => {
                let mut builder = TrayIconBuilder::with_id(TRAY_ID);
                #[cfg(any(windows, target_os = "linux"))]
                {
                    builder = builder.icon(tauri::image::Image::from_bytes(&icon::get_icon(
                        &icon::TrayIcon::Normal,
                    ))?);
                }
                #[cfg(target_os = "macos")]
                {
                    builder = builder
                        .icon(tauri::image::Image::from_bytes(include_bytes!(
                            "../../../icons/icon.icns"
                        ))?)
                        .icon_as_template(true);
                }
                builder
                    .menu(&menu)
                    .on_menu_event(|app, event| {
                        Tray::on_menu_item_event(app, event);
                    })
                    .on_tray_icon_event(|tray_icon, event| {
                        Tray::on_system_tray_event(tray_icon, event);
                    })
                    .build(app_handle)?
            }
            Some(tray) => {
                tray.set_menu(Some(menu.clone()))?;
                tray
            }
        };
        tray.set_visible(true)?;
        {
            match app_handle.try_state::<TrayState<tauri::Wry>>() {
                Some(state) => {
                    *state.menu.lock() = menu;
                }
                None => {
                    app_handle.manage(TrayState {
                        menu: Mutex::new(menu),
                    });
                }
            }
        }
        Tray::update_part(app_handle)?;
        Ok(())
    }

    #[instrument(skip(app_handle))]
    pub fn update_part<R: Runtime>(app_handle: &AppHandle<R>) -> Result<()> {
        let mode = crate::utils::config::get_current_clash_mode();
        let core = {
            *Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::default())
        };
        let tray = app_handle.tray_by_id(TRAY_ID).unwrap();
        let state = app_handle.state::<TrayState<R>>();
        let menu = state.menu.lock();

        let _ = menu
            .get("rule_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "rule").ok());
        let _ = menu
            .get("global_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "global").ok());
        let _ = menu
            .get("direct_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "direct").ok());
        if core == ClashCore::ClashPremium {
            let _ = menu
                .get("script_mode")
                .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "script").ok());
        }

        let (system_proxy, tun_mode) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                *verge.enable_system_proxy.as_ref().unwrap_or(&false),
                *verge.enable_tun_mode.as_ref().unwrap_or(&false),
            )
        };

        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            use icon::TrayIcon;

            let mode = if tun_mode {
                TrayIcon::Tun
            } else if system_proxy {
                TrayIcon::SystemProxy
            } else {
                TrayIcon::Normal
            };
            let icon = icon::get_icon(&mode);
            let _ = tray.set_icon(Some(tauri::image::Image::from_bytes(&icon)?));
        }

        let _ = menu
            .get("system_proxy")
            .and_then(|item| item.as_check_menuitem()?.set_checked(system_proxy).ok());
        let _ = menu
            .get("tun_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(tun_mode).ok());

        let switch_map = {
            let mut map = std::collections::HashMap::new();
            map.insert(true, t!("tray.proxy_action.on"));
            map.insert(false, t!("tray.proxy_action.off"));
            map
        };

        #[cfg(not(target_os = "linux"))]
        {
            let _ = tray.set_tooltip(Some(&format!(
                "{}: {}\n{}: {}",
                t!("tray.system_proxy"),
                switch_map[&system_proxy],
                t!("tray.tun_mode"),
                switch_map[&tun_mode]
            )));
        }
        #[cfg(target_os = "linux")]
        {
            let _ = tray.set_title(Some(&format!(
                "{}: {}\n{}: {}",
                t!("tray.system_proxy"),
                switch_map[&system_proxy],
                t!("tray.tun_mode"),
                switch_map[&tun_mode]
            )));
        }

        Ok(())
    }

    #[instrument(skip(app_handle, event))]
    pub fn on_menu_item_event(app_handle: &AppHandle, event: MenuEvent) {
        let id = event.id().0.as_str();
        match id {
            mode @ ("rule_mode" | "global_mode" | "direct_mode" | "script_mode") => {
                let mode = &mode[0..mode.len() - 5];
                feat::change_clash_mode(mode.into());
            }

            "open_window" => resolve::create_window(app_handle),
            "system_proxy" => feat::toggle_system_proxy(),
            "tun_mode" => feat::toggle_tun_mode(),
            "copy_env_sh" => feat::copy_clash_env(app_handle, "sh"),
            #[cfg(target_os = "windows")]
            "copy_env_cmd" => feat::copy_clash_env(app_handle, "cmd"),
            #[cfg(target_os = "windows")]
            "copy_env_ps" => feat::copy_clash_env(app_handle, "ps"),
            "open_app_config_dir" => crate::log_err!(ipc::open_app_config_dir()),
            "open_app_data_dir" => crate::log_err!(ipc::open_app_data_dir()),
            "open_core_dir" => crate::log_err!(ipc::open_core_dir()),
            "open_logs_dir" => crate::log_err!(ipc::open_logs_dir()),
            "restart_clash" => feat::restart_clash_core(),
            "restart_app" => help::restart_application(app_handle),
            "quit" => {
                help::quit_application(app_handle);
            }
            _ => {
                proxies::on_system_tray_event(id);
            }
        }
    }

    pub fn on_system_tray_event(tray_icon: &TrayIcon, event: TrayIconEvent) {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            ..
        } = event
        {
            resolve::create_window(tray_icon.app_handle());
        }
    }
}
