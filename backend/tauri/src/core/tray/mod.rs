use std::borrow::Cow;

use crate::{
    client::{NyanpasuClient, effects::plan::TrayView, hotkey::ports::HotkeyAction},
    feat::{self, CopyEnvOption},
    ipc, log_err,
    utils::{help, resolve},
};
use anyhow::Result;
use nyanpasu_config::{
    application::{ClashCore, TrayMenuMode},
    clash::config::overrides::Mode,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tracing_attributes::instrument;

pub mod icon;
pub mod proxies;
pub use self::icon::on_scale_factor_changed;
use self::proxies::SystemTrayMenuProxiesExt;

#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicU16;

/// Managed by the composition root before anything can build the tray.
pub struct TrayState<R: Runtime> {
    /// The only input tray rendering reads. Replaced by every tray effect.
    view: Mutex<TrayView>,
    /// The menu of the tray icon, and the mode it was built for; both `None`
    /// until the tray is first built.
    menu: Mutex<Option<Menu<R>>>,
    menu_mode: Mutex<Option<TrayMenuMode>>,
}

impl<R: Runtime> TrayState<R> {
    pub fn new(view: TrayView) -> Self {
        Self {
            view: Mutex::new(view),
            menu: Mutex::new(None),
            menu_mode: Mutex::new(None),
        }
    }
}

/// The view the tray renders, or `None` before the state is seeded.
fn tray_view<R: Runtime>(app_handle: &AppHandle<R>) -> Option<TrayView> {
    app_handle
        .try_state::<TrayState<R>>()
        .map(|state| *state.view.lock())
}

pub struct Tray {}

static UPDATE_SYSTRAY_MUTEX: Lazy<parking_lot::Mutex<()>> =
    Lazy::new(|| parking_lot::Mutex::new(()));

const TRAY_ID: &str = "main-tray";

#[cfg(target_os = "linux")]
static LINUX_TRAY_ID: AtomicU16 = AtomicU16::new(0);
// #[cfg(target_os = "linux")]
// fn bump_tray_id() -> Cow<'static, str> {
//     let id = LINUX_TRAY_ID.fetch_add(1, std::sync::atomic::Ordering::Release) + 1;
//     Cow::Owned(format!("{}-{}", TRAY_ID, id))
// }

#[inline]
fn get_tray_id<'n>() -> Cow<'n, str> {
    #[cfg(target_os = "linux")]
    {
        let id = LINUX_TRAY_ID.load(std::sync::atomic::Ordering::Acquire);
        Cow::Owned(format!("{}-{}", TRAY_ID, id))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Cow::Borrowed(TRAY_ID)
    }
}

// fn dummy_print_submenu<R: Runtime>(submenu: &Submenu<R>) {
//     for item in submenu.items().unwrap() {
//         tracing::debug!("item: {:#?}", item.id());
//         match item {
//             tauri::menu::MenuItemKind::MenuItem(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: MenuItem, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Submenu(submenu) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Submenu, text: {:#?}",
//                     submenu.id(),
//                     submenu.text()
//                 );
//                 dummy_print_submenu(&submenu);
//             }
//             tauri::menu::MenuItemKind::Predefined(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Predefined, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Check(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Check, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Icon(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Icon, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//         }
//     }
// }

// fn dummy_print_menu<R: Runtime>(menu: &Menu<R>) {
//     for item in menu.items().unwrap() {
//         tracing::debug!("item: {:#?}", item.id());
//         match item {
//             tauri::menu::MenuItemKind::MenuItem(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: MenuItem, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Submenu(submenu) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Submenu, text: {:#?}",
//                     submenu.id(),
//                     submenu.text()
//                 );
//                 dummy_print_submenu(&submenu);
//             }
//             tauri::menu::MenuItemKind::Predefined(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Predefined, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Check(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Check, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//             tauri::menu::MenuItemKind::Icon(item) => {
//                 tracing::debug!(
//                     "item: {:#?}, type: Icon, text: {:#?}",
//                     item.id(),
//                     item.text()
//                 );
//             }
//         }
//     }
// }

impl Tray {
    /// Stores the view that every later build and repaint renders.
    pub fn store_view<R: Runtime>(app_handle: &AppHandle<R>, view: TrayView) {
        match app_handle.try_state::<TrayState<R>>() {
            Some(state) => *state.view.lock() = view,
            None => tracing::warn!("a tray view arrived before the tray state was seeded"),
        }
    }

    #[instrument(skip(app_handle))]
    pub fn tray_menu<R: Runtime>(app_handle: &AppHandle<R>, view: &TrayView) -> Result<Menu<R>> {
        let version = env!("NYANPASU_VERSION");
        let mut menu = MenuBuilder::new(app_handle)
            .text("open_window", t!("tray.dashboard"))
            .setup_proxies(app_handle, view)? // Setup the proxies menu
            .separator()
            .check("rule_mode", t!("tray.rule_mode"))
            .check("global_mode", t!("tray.global_mode"))
            .check("direct_mode", t!("tray.direct_mode"));
        if view.menu.core == ClashCore::ClashPremium {
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
                    .text("restart_core", t!("tray.more.restart_core"))
                    .text("restart_app", t!("tray.more.restart_app"))
                    .item(
                        &MenuItemBuilder::new(format!("Version {version}"))
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
        let _guard = UPDATE_SYSTRAY_MUTEX.lock();
        let Some(state) = app_handle.try_state::<TrayState<tauri::Wry>>() else {
            tracing::warn!("the tray state is not seeded yet, skipping the tray rebuild");
            return Ok(());
        };
        let view = *state.view.lock();
        let tray_id = get_tray_id();
        let menu_mode = view.menu.menu_mode;
        let use_native_menu = menu_mode == TrayMenuMode::Native;
        let menu_mode_changed = state
            .menu_mode
            .lock()
            .is_some_and(|built| built != menu_mode);
        let tray = if menu_mode_changed {
            tracing::debug!("tray menu mode changed, recreating tray icon");
            let tray = app_handle.remove_tray_by_id(tray_id.as_ref());
            drop(tray);
            None
        } else {
            // if cfg!(target_os = "linux") {
            //     tracing::debug!("removing tray by id: {}", tray_id);
            //     let mut tray = app_handle.remove_tray_by_id(tray_id.as_ref());
            //     tray.take(); // Drop the tray
            //     tray_id = bump_tray_id();
            //     tracing::debug!("bumped tray id to: {}", tray_id);
            // }
            app_handle.tray_by_id(tray_id.as_ref())
        };

        let menu = Tray::tray_menu(app_handle, &view)?;
        let tray = match tray {
            None => {
                let mut builder = TrayIconBuilder::with_id(tray_id);
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
                            "../../../icons/tray-icon.png"
                        ))?)
                        .icon_as_template(true);
                }
                if use_native_menu {
                    builder = builder.menu(&menu).on_menu_event(|app, event| {
                        Tray::on_menu_item_event(app, event);
                    });
                }
                builder
                    .on_tray_icon_event(|tray_icon, event| {
                        Tray::on_system_tray_event(tray_icon, event);
                    })
                    .show_menu_on_left_click(false)
                    .build(app_handle)?
            }
            Some(tray) => {
                // This is a workaround for linux tray menu update. Due to the api disallow set_menu again
                // and recreate tray icon will cause buggy tray. No icon and no menu.
                // So this block is a dirty inheritance of the menu items from the previous tray menu.
                if cfg!(target_os = "linux") {
                    if use_native_menu && let Some(previous_menu) = state.menu.lock().as_ref() {
                        if let Ok(items) = previous_menu.items() {
                            tracing::debug!("removing previous tray menu items");
                            for item in items {
                                log_err!(previous_menu.remove(&item), "failed to remove menu item");
                            }
                        }
                        // migrate the menu items
                        if let Ok(items) = menu.items() {
                            tracing::debug!("migrating new tray menu items");
                            for item in items {
                                log_err!(previous_menu.append(&item), "failed to append menu item");
                            }
                        }
                    }
                } else if use_native_menu {
                    tray.set_menu(Some(menu.clone()))?;
                } else {
                    tray.set_menu(None::<tauri::menu::Menu<tauri::Wry>>)?;
                }
                tray
            }
        };
        tray.set_visible(true)?;
        {
            let mut built = state.menu.lock();
            if cfg!(not(target_os = "linux")) || menu_mode_changed || built.is_none() {
                tracing::debug!("replacing previous tray menu");
                *built = Some(menu);
            }
            *state.menu_mode.lock() = Some(menu_mode);
        }
        tracing::debug!("full update tray finished");
        Tray::update_part(app_handle)?;
        Ok(())
    }

    #[instrument(skip(app_handle))]
    pub fn update_part<R: Runtime>(app_handle: &AppHandle<R>) -> Result<()> {
        let tray_id = get_tray_id();
        tracing::debug!("updating tray part: {}", tray_id);
        // The first build renders the latest view, so a repaint that comes
        // before it has nothing to repaint and nothing to lose.
        let Some(tray) = app_handle.tray_by_id(tray_id.as_ref()) else {
            tracing::debug!("the tray is not built yet, skipping the part refresh");
            return Ok(());
        };
        let Some(state) = app_handle.try_state::<TrayState<R>>() else {
            tracing::warn!("the tray state is not seeded yet, skipping the part refresh");
            return Ok(());
        };
        let view = *state.view.lock();
        let menu = state.menu.lock();
        let Some(menu) = menu.as_ref() else {
            tracing::debug!("the tray menu is not built yet, skipping the part refresh");
            return Ok(());
        };
        let mode = view.part.mode;

        let _ = menu.get("rule_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(mode == Mode::Rule)
                .ok()
        });
        let _ = menu.get("global_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(mode == Mode::Global)
                .ok()
        });
        let _ = menu.get("direct_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(mode == Mode::Direct)
                .ok()
        });
        if view.menu.core == ClashCore::ClashPremium {
            let _ = menu.get("script_mode").and_then(|item| {
                item.as_check_menuitem()?
                    .set_checked(mode == Mode::Script)
                    .ok()
            });
        }

        #[allow(unused_variables)]
        let (system_proxy, tun_mode, enable_tray_text) =
            (view.part.system_proxy, view.part.tun, view.part.text);

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
            if enable_tray_text {
                let _ = tray.set_title(Some(&format!(
                    "{}: {}\n{}: {}",
                    t!("tray.system_proxy"),
                    switch_map[&system_proxy],
                    t!("tray.tun_mode"),
                    switch_map[&tun_mode]
                )));
            } else {
                let _ = tray.set_title::<&str>(None);
            }
        }

        Ok(())
    }

    /// Tray items and global shortcuts run the same actions, so they go
    /// through one mapping in the facade rather than two parallel ones.
    #[instrument(skip(app_handle, event))]
    pub fn on_menu_item_event(app_handle: &AppHandle, event: MenuEvent) {
        let id = event.id().0.as_str();
        match id {
            "rule_mode" => dispatch_action(app_handle, HotkeyAction::ClashModeRule),
            "global_mode" => dispatch_action(app_handle, HotkeyAction::ClashModeGlobal),
            "direct_mode" => dispatch_action(app_handle, HotkeyAction::ClashModeDirect),
            "script_mode" => dispatch_action(app_handle, HotkeyAction::ClashModeScript),

            "open_window" => resolve::create_window(app_handle),
            "system_proxy" => dispatch_action(app_handle, HotkeyAction::ToggleSystemProxy),
            "tun_mode" => dispatch_action(app_handle, HotkeyAction::ToggleTunMode),
            "copy_env_sh" => copy_clash_env(app_handle, CopyEnvOption::Shell),
            #[cfg(target_os = "windows")]
            "copy_env_cmd" => copy_clash_env(app_handle, CopyEnvOption::Cmd),
            #[cfg(target_os = "windows")]
            "copy_env_ps" => copy_clash_env(app_handle, CopyEnvOption::Pwsh),
            "open_app_config_dir" => crate::log_err!(ipc::open_app_config_dir()),
            "open_app_data_dir" => crate::log_err!(ipc::open_app_data_dir()),
            "open_core_dir" => crate::log_err!(ipc::open_core_dir()),
            "open_logs_dir" => crate::log_err!(ipc::open_logs_dir()),
            "restart_core" => feat::restart_clash_core(app_handle),
            "restart_app" => help::restart_application(app_handle),
            "quit" => {
                help::quit_application(app_handle);
            }
            _ => {
                proxies::on_system_tray_event(app_handle, id);
            }
        }
    }

    pub fn on_system_tray_event(tray_icon: &TrayIcon, event: TrayIconEvent) {
        match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } => {
                resolve::create_window(tray_icon.app_handle());
            }
            TrayIconEvent::Click {
                button: MouseButton::Right,
                position,
                ..
            } if tray_view(tray_icon.app_handle())
                .is_some_and(|view| view.menu.menu_mode == TrayMenuMode::Webview) =>
            {
                log_err!(
                    resolve::show_tray_menu_window(tray_icon.app_handle(), position),
                    "failed to show webview tray menu"
                );
            }
            // In Native mode the system shows the attached menu automatically
            _ => {}
        }
    }
}

/// Copies the proxy environment for the port the facade reports.
fn copy_clash_env(app_handle: &AppHandle, option: CopyEnvOption) {
    let Some(client) = app_handle.try_state::<NyanpasuClient>() else {
        tracing::warn!("the tray copied the proxy env before the client was ready");
        return;
    };
    feat::copy_clash_env(app_handle, client.clash_info().port, &option);
}

/// Runs a tray item through the facade, the same path a global shortcut takes.
fn dispatch_action(app_handle: &AppHandle, action: HotkeyAction) {
    // A tray click during startup can arrive before the client is managed;
    // dropping it is better than taking the whole app down.
    let Some(client) = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone())
    else {
        tracing::warn!(%action, "the tray fired before the client was ready");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = client.dispatch_hotkey_action(action).await {
            tracing::error!(%error, %action, "tray action failed");
        }
    });
}
