use crate::{config::Config, feat, ipc, utils, utils::resolve};
use anyhow::Result;
use rust_i18n::t;
use tauri::{
    AppHandle, CustomMenuItem, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
    SystemTraySubmenu,
};
use tracing_attributes::instrument;

pub mod icon;
pub mod proxies;
pub use self::icon::on_scale_factor_changed;
use self::proxies::SystemTrayMenuProxiesExt;

pub struct Tray {}

impl Tray {
    #[instrument(skip(_app_handle))]
    pub fn tray_menu(_app_handle: &AppHandle) -> SystemTrayMenu {
        let version = env!("NYANPASU_VERSION");

        SystemTrayMenu::new()
            .add_item(CustomMenuItem::new("open_window", t!("tray.dashboard")))
            .setup_proxies() // Setup the proxies menu
            .add_native_item(SystemTrayMenuItem::Separator)
            .add_item(CustomMenuItem::new("rule_mode", t!("tray.rule_mode")))
            .add_item(CustomMenuItem::new("global_mode", t!("tray.global_mode")))
            .add_item(CustomMenuItem::new("direct_mode", t!("tray.direct_mode")))
            .add_item(CustomMenuItem::new("script_mode", t!("tray.script_mode")))
            .add_native_item(SystemTrayMenuItem::Separator)
            .add_item(CustomMenuItem::new("system_proxy", t!("tray.system_proxy")))
            .add_item(CustomMenuItem::new("tun_mode", t!("tray.tun_mode")))
            .add_item(CustomMenuItem::new("copy_env_sh", t!("tray.copy_env.sh")))
            .add_item(CustomMenuItem::new("copy_env_cmd", t!("tray.copy_env.cmd")))
            .add_item(CustomMenuItem::new("copy_env_ps", t!("tray.copy_env.ps")))
            .add_submenu(SystemTraySubmenu::new(
                t!("tray.open_dir.menu"),
                SystemTrayMenu::new()
                    .add_item(CustomMenuItem::new(
                        "open_app_config_dir",
                        t!("tray.open_dir.app_config_dir"),
                    ))
                    .add_item(CustomMenuItem::new(
                        "open_app_data_dir",
                        t!("tray.open_dir.app_data_dir"),
                    ))
                    .add_item(CustomMenuItem::new(
                        "open_core_dir",
                        t!("tray.open_dir.core_dir"),
                    ))
                    .add_item(CustomMenuItem::new(
                        "open_logs_dir",
                        t!("tray.open_dir.log_dir"),
                    )),
            ))
            .add_submenu(SystemTraySubmenu::new(
                t!("tray.more.menu"),
                SystemTrayMenu::new()
                    .add_item(CustomMenuItem::new(
                        "restart_clash",
                        t!("tray.more.restart_clash"),
                    ))
                    .add_item(CustomMenuItem::new(
                        "restart_app",
                        t!("tray.more.restart_app"),
                    ))
                    .add_item(
                        CustomMenuItem::new("app_version", format!("Version {version}")).disabled(),
                    ),
            ))
            .add_native_item(SystemTrayMenuItem::Separator)
            .add_item(CustomMenuItem::new("quit", t!("tray.quit")).accelerator("CmdOrControl+Q"))
    }

    #[instrument(skip(app_handle))]
    pub fn update_systray(app_handle: &AppHandle) -> Result<()> {
        app_handle
            .tray_handle()
            .set_menu(Tray::tray_menu(app_handle))?;
        Tray::update_part(app_handle)?;
        Ok(())
    }

    #[instrument(skip(app_handle))]
    pub fn update_part(app_handle: &AppHandle) -> Result<()> {
        let mode = crate::utils::config::get_current_clash_mode();

        let tray = app_handle.tray_handle();

        let _ = tray.get_item("rule_mode").set_selected(mode == "rule");
        let _ = tray.get_item("global_mode").set_selected(mode == "global");
        let _ = tray.get_item("direct_mode").set_selected(mode == "direct");
        let _ = tray.get_item("script_mode").set_selected(mode == "script");

        let verge = Config::verge();
        let verge = verge.latest();
        let system_proxy = verge.enable_system_proxy.as_ref().unwrap_or(&false);
        let tun_mode = verge.enable_tun_mode.as_ref().unwrap_or(&false);

        #[cfg(target_os = "windows")]
        {
            use icon::TrayIcon;

            let mode = if *tun_mode {
                TrayIcon::Tun
            } else if *system_proxy {
                TrayIcon::SystemProxy
            } else {
                TrayIcon::Normal
            };
            let icon = icon::get_icon(&mode);
            let _ = tray.set_icon(tauri::Icon::Raw(icon));
        }

        let _ = tray.get_item("system_proxy").set_selected(*system_proxy);
        let _ = tray.get_item("tun_mode").set_selected(*tun_mode);

        #[cfg(not(target_os = "linux"))]
        {
            let switch_map = {
                let mut map = std::collections::HashMap::new();
                map.insert(true, t!("tray.proxy_action.on"));
                map.insert(false, t!("tray.proxy_action.off"));
                map
            };

            let _ = tray.set_tooltip(&format!(
                "{}: {}\n{}: {}",
                t!("tray.system_proxy"),
                switch_map[system_proxy],
                t!("tray.tun_mode"),
                switch_map[tun_mode]
            ));
        }

        Ok(())
    }

    #[instrument(skip(app_handle, event))]
    pub fn on_system_tray_event(app_handle: &AppHandle, event: SystemTrayEvent) {
        match event {
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
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
                "restart_app" => utils::help::restart_application(app_handle),
                "quit" => {
                    utils::help::quit_application(app_handle);
                }
                _ => {
                    proxies::on_system_tray_event(&id);
                }
            },
            #[cfg(target_os = "windows")]
            SystemTrayEvent::LeftClick { .. } => {
                resolve::create_window(app_handle);
            }
            _ => {}
        }
    }
}
