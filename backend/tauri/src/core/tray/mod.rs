use crate::{
    config::{nyanpasu::ClashCore, Config},
    feat, ipc,
    utils::{help, resolve},
};
use anyhow::Result;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    menu::{
        CheckMenuItemBuilder, Menu, MenuBuilder, MenuEvent, MenuItemBuilder, PredefinedMenuItem,
        SubmenuBuilder,
    },
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};
use tracing_attributes::instrument;

pub mod icon;
pub mod proxies;
pub use self::icon::on_scale_factor_changed;
use self::proxies::SystemTrayMenuProxiesExt;
mod utils;
pub struct Tray {}

pub struct TrayState {
    pub tray_icon: TrayIcon,
    pub menu: Mutex<Menu>,
}

impl Tray {
    #[instrument(skip(app_handle))]
    pub fn tray_menu(app_handle: &AppHandle) -> Result<Menu> {
        let version = env!("NYANPASU_VERSION");
        let core = {
            *Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::default())
        };
        let mut menu = MenuBuilder::new(app_handle)
            .item(
                &MenuItemBuilder::new(t!("tray.dashboard"))
                    .id("open_window")
                    .build(app_handle)?,
            )
            // .setup_proxies() // Setup the proxies menu
            .item(&PredefinedMenuItem::separator(app_handle)?)
            .item(
                &CheckMenuItemBuilder::new(t!("tray.rule_mode"))
                    .id("rule_mode")
                    .build(app_handle)?,
            )
            .item(
                &CheckMenuItemBuilder::new(t!("tray.global_mode"))
                    .id("global_mode")
                    .build(app_handle)?,
            )
            .item(
                &CheckMenuItemBuilder::new(t!("tray.direct_mode"))
                    .id("direct_mode")
                    .build(app_handle)?,
            );
        if core == ClashCore::ClashPremium {
            menu = menu.item(
                &CheckMenuItemBuilder::new(t!("tray.script_mode"))
                    .id("script_mode")
                    .build(app_handle)?,
            );
        }
        menu.item(&PredefinedMenuItem::separator(app_handle)?)
            .item(
                &CheckMenuItemBuilder::new(t!("tray.system_proxy"))
                    .id("system_proxy")
                    .build(app_handle)?,
            )
            .item(
                &CheckMenuItemBuilder::new(t!("tray.tun_mode"))
                    .id("tun_mode")
                    .build(app_handle)?,
            )
            .item(
                &MenuItemBuilder::new(t!("tray.copy_env_sh"))
                    .id("copy_env_sh")
                    .build(app_handle)?,
            )
            .item(
                &MenuItemBuilder::new(t!("tray.copy_env_cmd"))
                    .id("copy_env_cmd")
                    .build(app_handle)?,
            )
            .item(
                &MenuItemBuilder::new(t!("tray.copy_env_ps"))
                    .id("copy_env_ps")
                    .build(app_handle)?,
            )
            .item(
                &SubmenuBuilder::new(app_handle, t!("tray.open_dir.menu"))
                    .item(
                        &MenuItemBuilder::new(t!("tray.open_dir.app_config_dir"))
                            .id("open_app_config_dir")
                            .build(app_handle)?,
                    )
                    .item(
                        &MenuItemBuilder::new(t!("tray.open_dir.app_data_dir"))
                            .id("open_app_data_dir")
                            .build(app_handle)?,
                    )
                    .item(
                        &MenuItemBuilder::new(t!("tray.open_dir.core_dir"))
                            .id("open_core_dir")
                            .build(app_handle)?,
                    )
                    .item(
                        &MenuItemBuilder::new(t!("tray.open_dir.log_dir"))
                            .id("open_logs_dir")
                            .build(app_handle)?,
                    )
                    .build()?,
            )
            .item(
                &SubmenuBuilder::new(app_handle, t!("tray.more.menu"))
                    .item(
                        &MenuItemBuilder::new(t!("tray.more.restart_clash"))
                            .id("restart_clash")
                            .build(app_handle)?,
                    )
                    .item(
                        &MenuItemBuilder::new(t!("tray.more.restart_app"))
                            .id("restart_app")
                            .build(app_handle)?,
                    )
                    .item(
                        &MenuItemBuilder::new(format!("Version {}", version))
                            .id("app_version")
                            .enabled(false)
                            .build(app_handle)?,
                    )
                    .build()?,
            )
            .item(&PredefinedMenuItem::separator(app_handle)?)
            .item(
                &MenuItemBuilder::new(t!("tray.quit"))
                    .id("quit")
                    .accelerator("CmdOrControl+Q")
                    .build(app_handle)?,
            );
        Ok(menu.build()?)
    }

    pub fn setup_tray(app: &mut App) -> Result<()> {
        let menu = Tray::tray_menu(app.handle())?;
        let tray_icon = TrayIconBuilder::new()
            .icon(tauri::image::Image::from_bytes(&icon::get_icon(
                &icon::TrayIcon::Normal,
            ))?)
            .menu(&menu)
            .on_menu_event(|app, event| {
                Tray::on_menu_item_event(app, event);
            })
            .on_tray_icon_event(|tray_icon, event| {
                Tray::on_system_tray_event(tray_icon, event);
            })
            .build(app)?;
        app.manage::<TrayState>(TrayState {
            tray_icon,
            menu: Mutex::new(menu),
        });
        Ok(())
    }

    #[instrument(skip(app_handle))]
    pub fn update_systray(app_handle: &AppHandle) -> Result<()> {
        let tray_state = app_handle.state::<TrayState>();
        let menu = Tray::tray_menu(app_handle)?;
        tray_state.tray_icon.set_menu(Some(menu.clone()))?;
        {
            *tray_state.menu.lock() = menu;
        }
        Tray::update_part(app_handle)?;
        Ok(())
    }

    #[instrument(skip(app_handle))]
    pub fn update_part(app_handle: &AppHandle) -> Result<()> {
        let mode = crate::utils::config::get_current_clash_mode();
        let core = {
            *Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::default())
        };
        let tray = app_handle.state::<TrayState>();
        let menu = tray.menu.lock();
        #[cfg(target_os = "linux")]
        {
            let _ = tray.get_item("rule_mode").set_title(t!("tray.rule_mode"));
            let _ = tray
                .get_item("global_mode")
                .set_title(t!("tray.global_mode"));
            let _ = tray
                .get_item("direct_mode")
                .set_title(t!("tray.direct_mode"));
            if core == ClashCore::ClashPremium {
                let _ = tray
                    .get_item("script_mode")
                    .set_title(t!("tray.script_mode"));
            }
            match mode.as_str() {
                "rule" => {
                    let _ = tray
                        .get_item("rule_mode")
                        .set_title(utils::selected_title(t!("tray.rule_mode")));
                }
                "global" => {
                    let _ = tray
                        .get_item("global_mode")
                        .set_title(utils::selected_title(t!("tray.global_mode")));
                }
                "direct" => {
                    let _ = tray
                        .get_item("direct_mode")
                        .set_title(utils::selected_title(t!("tray.direct_mode")));
                }
                "script" => {
                    let _ = tray
                        .get_item("script_mode")
                        .set_title(utils::selected_title(t!("tray.script_mode")));
                }
                _ => {}
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
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
        }

        let (system_proxy, tun_mode) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                *verge.enable_system_proxy.as_ref().unwrap_or(&false),
                *verge.enable_tun_mode.as_ref().unwrap_or(&false),
            )
        };

        #[cfg(target_os = "windows")]
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
            let _ = tray
                .tray_icon
                .set_icon(Some(tauri::image::Image::from_bytes(&icon)?));
        }

        #[cfg(target_os = "linux")]
        {
            match system_proxy {
                true => {
                    let _ = tray
                        .get_item("system_proxy")
                        .set_title(utils::selected_title(t!("tray.system_proxy")));
                }
                false => {
                    let _ = tray
                        .get_item("system_proxy")
                        .set_title(t!("tray.system_proxy"));
                }
            }

            match tun_mode {
                true => {
                    let _ = tray
                        .get_item("tun_mode")
                        .set_title(utils::selected_title(t!("tray.tun_mode")));
                }
                false => {
                    let _ = tray.get_item("tun_mode").set_title(t!("tray.tun_mode"));
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = menu
                .get("system_proxy")
                .and_then(|item| item.as_check_menuitem()?.set_checked(system_proxy).ok());
            let _ = menu
                .get("tun_mode")
                .and_then(|item| item.as_check_menuitem()?.set_checked(tun_mode).ok());
        }

        #[cfg(not(target_os = "linux"))]
        {
            let switch_map = {
                let mut map = std::collections::HashMap::new();
                map.insert(true, t!("tray.proxy_action.on"));
                map.insert(false, t!("tray.proxy_action.off"));
                map
            };

            let _ = tray.tray_icon.set_tooltip(Some(&format!(
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
            "copy_env_sh" => feat::copy_clash_env("sh"),
            #[cfg(target_os = "windows")]
            "copy_env_cmd" => feat::copy_clash_env("cmd"),
            #[cfg(target_os = "windows")]
            "copy_env_ps" => feat::copy_clash_env("ps"),
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
        match event {
            TrayIconEvent::Click { button, .. } if button == MouseButton::Left => {
                resolve::create_window(tray_icon.app_handle());
            }
            _ => {}
        }
    }
}
