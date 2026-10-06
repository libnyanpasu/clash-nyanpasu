use std::{borrow::Cow, ops::ControlFlow};

use crate::{
    client::{NyanpasuClient, effects::plan::TrayView, hotkey::ports::HotkeyAction},
    ipc, log_err,
    utils::{
        help,
        proxy_env::{self, CopyEnvOption},
    },
    window::{
        WindowManager,
        kinds::{MainWindow, show_tray_menu_window},
    },
};
use anyhow::Result;
use nyanpasu_config::{
    application::{ClashCore, TrayMenuMode},
    clash::config::overrides::Mode,
};
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tracing_attributes::instrument;

mod display;
mod executor;
pub mod icon;
pub mod proxies;
use self::{
    display::{Paint, ProxySection, Publication, Shown, TrayDisplay},
    executor::{Step, TrayQueue, TrayTarget},
    proxies::SystemTrayMenuProxiesExt,
};
pub use self::{executor::TrayWork, icon::on_scale_factor_changed};

/// Managed by the composition root before anything can build the tray.
///
/// Each lock is taken only to read or record, never held across a Tauri call
/// or a `Tray::request`: the steps run on the main thread, where tray and menu
/// event handlers read this state too, and none of these locks is
/// re-entrant.
pub struct TrayState<R: Runtime> {
    /// The only input tray rendering reads. Replaced by every tray effect.
    view: Mutex<TrayView>,
    /// Tray work waiting for the drain. Narrow boundary state rather than an
    /// actor: menu calls have to run on the main thread, which only takes
    /// closures, and this queue is what keeps one drain at a time there.
    queue: Mutex<TrayQueue>,
    /// What the tray displays: the menu and its proxy selections, as one
    /// record that only a complete publication replaces. Any failure after
    /// the live menu may have changed leaves it unknown until a rebuild
    /// completes.
    display: Mutex<TrayDisplay<Attached<R>>>,
}

/// A menu the tray icon holds, and the mode the icon was built for.
struct Attached<R: Runtime> {
    menu: Menu<R>,
    mode: TrayMenuMode,
}

// Not derived: a derive would also require `R: Clone`.
impl<R: Runtime> Clone for Attached<R> {
    fn clone(&self) -> Self {
        Self {
            menu: self.menu.clone(),
            mode: self.mode,
        }
    }
}

impl<R: Runtime> TrayState<R> {
    pub fn new(view: TrayView) -> Self {
        Self {
            view: Mutex::new(view),
            queue: Mutex::new(TrayQueue::default()),
            display: Mutex::new(TrayDisplay::new()),
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

const TRAY_ID: &str = "main-tray";

#[cfg(target_os = "linux")]
const LINUX_TRAY_ID: u16 = 0;

#[inline]
fn get_tray_id<'n>() -> Cow<'n, str> {
    #[cfg(target_os = "linux")]
    {
        Cow::Owned(format!("{}-{}", TRAY_ID, LINUX_TRAY_ID))
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

    /// Asks for tray work. Any thread may ask, a running step included: the
    /// work merges into the queue, and one drain on the main thread runs it.
    ///
    /// Requests made during a drain only merge work. A request made on an
    /// idle main thread may run the drain inline: Tauri runs the task it
    /// schedules there at once. Nothing is locked by then, and a running drain
    /// keeps the queue scheduled, so a request from inside one never starts
    /// another.
    pub fn request(app_handle: &AppHandle<tauri::Wry>, work: TrayWork) -> tauri::Result<()> {
        let Some(state) = app_handle.try_state::<TrayState<tauri::Wry>>() else {
            tracing::warn!("the tray state is not seeded yet, dropping the tray work");
            return Ok(());
        };
        executor::request(&state.queue, work, || {
            let handle = app_handle.clone();
            app_handle.run_on_main_thread(move || Tray::drain(&handle))
        })
    }

    /// Runs the queued tray work, on the main thread that `request` scheduled
    /// it on.
    fn drain(app_handle: &AppHandle<tauri::Wry>) {
        let Some(state) = app_handle.try_state::<TrayState<tauri::Wry>>() else {
            return;
        };
        executor::drain(
            &state.queue,
            &mut LiveTray {
                app_handle,
                state: state.inner(),
            },
        );
    }

    /// The menu for `view`, with its proxy section apart: that describes the
    /// tray only once the menu is published.
    #[instrument(skip(app_handle))]
    fn tray_menu<R: Runtime>(
        app_handle: &AppHandle<R>,
        view: &TrayView,
    ) -> Result<(Menu<R>, ProxySection)> {
        let version = env!("NYANPASU_VERSION");
        let (menu, section) = MenuBuilder::new(app_handle)
            .text("open_window", t!("tray.dashboard"))
            .setup_proxies(app_handle, view)?; // Setup the proxies menu
        let mut menu = menu
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
                    .text("restart_app", t!("tray.more.restart_app"))
                    .text("restart_core", t!("tray.more.restart_core"))
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

        Ok((menu.build()?, section))
    }

    /// Builds the menu for the stored view and publishes it.
    #[instrument(skip(app_handle, state))]
    fn rebuild(app_handle: &AppHandle<tauri::Wry>, state: &TrayState<tauri::Wry>) -> Result<()> {
        let view = *state.view.lock();
        // Built before the live tray is touched, so a failure here leaves the
        // tray, and the record of it, as they were.
        let (menu, section) = match Tray::tray_menu(app_handle, &view) {
            Ok(built) => built,
            Err(error) => {
                state.display.lock().published(Publication::NotStarted);
                return Err(error);
            }
        };
        let mut attached = state.display.lock().attached();
        let published = Tray::attach(app_handle, &view, menu, &mut attached);
        let publication = match (&published, attached) {
            (Ok(()), Some(attached)) => Publication::Complete { attached, section },
            (_, attached) => Publication::Interrupted { attached },
        };
        state.display.lock().published(publication);
        published?;
        tracing::debug!("full update tray finished");
        Ok(())
    }

    /// Puts `menu` on the tray and shows it. `attached` follows the menu
    /// object the icon holds through every step, so a failure part-way still
    /// says which one it holds.
    fn attach(
        app_handle: &AppHandle<tauri::Wry>,
        view: &TrayView,
        menu: Menu<tauri::Wry>,
        attached: &mut Option<Attached<tauri::Wry>>,
    ) -> Result<()> {
        let tray_id = get_tray_id();
        let menu_mode = view.menu.menu_mode;
        let use_native_menu = menu_mode == TrayMenuMode::Native;
        let menu_mode_changed = attached.as_ref().is_some_and(|held| held.mode != menu_mode);
        let tray = if menu_mode_changed {
            tracing::debug!("tray menu mode changed, recreating tray icon");
            let tray = app_handle.remove_tray_by_id(tray_id.as_ref());
            drop(tray);
            *attached = None;
            None
        } else {
            app_handle.tray_by_id(tray_id.as_ref())
        };

        let mut refill = Ok(());
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
                let tray = builder
                    .on_tray_icon_event(|tray_icon, event| {
                        Tray::on_system_tray_event(tray_icon, event);
                    })
                    .show_menu_on_left_click(false)
                    .build(app_handle)?;
                *attached = Some(Attached {
                    menu,
                    mode: menu_mode,
                });
                tray
            }
            Some(tray) => {
                // This is a workaround for linux tray menu update. Due to the api disallow set_menu again
                // and recreate tray icon will cause buggy tray. No icon and no menu.
                // So this block is a dirty inheritance of the menu items from the previous tray menu.
                if cfg!(target_os = "linux") {
                    match attached.as_ref().map(|held| held.menu.clone()) {
                        Some(previous_menu) if use_native_menu => {
                            refill = refill_menu(&previous_menu, &menu);
                        }
                        Some(_) => {}
                        None => {
                            *attached = Some(Attached {
                                menu,
                                mode: menu_mode,
                            })
                        }
                    }
                } else {
                    let native = use_native_menu.then(|| menu.clone());
                    tray.set_menu(native)?;
                    *attached = Some(Attached {
                        menu,
                        mode: menu_mode,
                    });
                }
                tray
            }
        };
        tray.set_visible(true)?;
        refill
    }

    /// Repaints the checkmarks, icon and tooltip from the stored view.
    #[instrument(skip(app_handle, state))]
    fn repaint_part(
        app_handle: &AppHandle<tauri::Wry>,
        state: &TrayState<tauri::Wry>,
    ) -> Result<ControlFlow<()>> {
        let tray = app_handle.tray_by_id(get_tray_id().as_ref());
        let paint = state.display.lock().paint();
        // Checked before the icon is needed, so an icon that is gone gets a
        // rebuild rather than a skipped repaint.
        if executor::needs_rebuild(&paint, tray.is_some()) {
            tracing::debug!("the tray menu is unknown or has no icon, rebuilding it instead");
            Tray::request(app_handle, TrayWork::REBUILD)?;
            return Ok(ControlFlow::Break(()));
        }
        let (Paint::Menu(attached), Some(tray)) = (paint, tray) else {
            // The first build renders the latest view, so a repaint that comes
            // before it has nothing to repaint and nothing to lose.
            tracing::debug!("the tray menu is not built yet, skipping the part refresh");
            return Ok(ControlFlow::Break(()));
        };
        let menu = attached.menu;
        let view = *state.view.lock();
        let mode = view.part.mode;

        #[allow(unused_variables)]
        let (system_proxy, tun_mode, enable_tray_text) =
            (view.part.system_proxy, view.part.tun, view.part.text);

        // Every item here exists in a menu this tray built, so one that is
        // missing or refuses its check leaves the menu unknown.
        let check = |id: &str, checked: bool| {
            menu.get(id)
                .and_then(|item| item.as_check_menuitem()?.set_checked(checked).ok())
                .is_some()
        };
        let mut whole = check("rule_mode", mode == Mode::Rule)
            & check("global_mode", mode == Mode::Global)
            & check("direct_mode", mode == Mode::Direct);
        if view.menu.core == ClashCore::ClashPremium {
            whole &= check("script_mode", mode == Mode::Script);
        }
        whole &= check("system_proxy", system_proxy) & check("tun_mode", tun_mode);
        let shown = if whole { Shown::Whole } else { Shown::Partly };
        state.display.lock().repainted(None, shown);

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

        let switch_map = {
            let mut map = std::collections::HashMap::new();
            map.insert(true, t!("tray.proxy_action.enabled"));
            map.insert(false, t!("tray.proxy_action.disabled"));
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

        // A partial repaint left the menu unknown, and the next request
        // rebuilds it.
        Ok(match shown {
            Shown::Whole => ControlFlow::Continue(()),
            Shown::Partly => ControlFlow::Break(()),
        })
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

            "open_window" => {
                log_err!(app_handle.state::<WindowManager>().open(&MainWindow, None))
            }
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
            "restart_core" => restart_core(app_handle),
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
                log_err!(
                    tray_icon
                        .app_handle()
                        .state::<WindowManager>()
                        .open(&MainWindow, None)
                );
            }
            TrayIconEvent::Click {
                button: MouseButton::Right,
                position,
                ..
            } if tray_view(tray_icon.app_handle())
                .is_some_and(|view| view.menu.menu_mode == TrayMenuMode::Webview) =>
            {
                log_err!(
                    show_tray_menu_window(tray_icon.app_handle(), position),
                    "failed to show webview tray menu"
                );
            }
            // In Native mode the system shows the attached menu automatically
            _ => {}
        }
    }
}

/// The live tray, as the drain runs it on the main thread.
struct LiveTray<'a> {
    app_handle: &'a AppHandle<tauri::Wry>,
    state: &'a TrayState<tauri::Wry>,
}

impl TrayTarget for LiveTray<'_> {
    type Menu = Attached<tauri::Wry>;

    fn observe(&self) -> (Paint<Self::Menu>, bool) {
        let paint = self.state.display.lock().paint();
        let icon = self.app_handle.tray_by_id(get_tray_id().as_ref()).is_some();
        (paint, icon)
    }

    fn run(&mut self, step: Step) -> ControlFlow<()> {
        let ran = match step {
            Step::Rebuild => Tray::rebuild(self.app_handle, self.state).map(ControlFlow::Continue),
            Step::Part => Tray::repaint_part(self.app_handle, self.state),
            Step::Proxies => proxies::repaint_proxies(self.app_handle, self.state),
        };
        ran.unwrap_or_else(|error| {
            tracing::error!(?step, "tray step failed: {error:#}");
            ControlFlow::Break(())
        })
    }
}

/// Replaces the items of the menu the Linux tray holds with those of `menu`,
/// as far as it can. The menu object itself stays: GTK cannot take a new one.
fn refill_menu(previous_menu: &Menu<tauri::Wry>, menu: &Menu<tauri::Wry>) -> Result<()> {
    let mut whole = true;
    if let Ok(items) = previous_menu.items() {
        tracing::debug!("removing previous tray menu items");
        for item in items {
            let removed = previous_menu.remove(&item);
            whole &= removed.is_ok();
            log_err!(removed, "failed to remove menu item");
        }
    } else {
        whole = false;
    }
    // migrate the menu items
    if let Ok(items) = menu.items() {
        tracing::debug!("migrating new tray menu items");
        for item in items {
            let appended = previous_menu.append(&item);
            whole &= appended.is_ok();
            log_err!(appended, "failed to append menu item");
        }
    } else {
        whole = false;
    }
    anyhow::ensure!(whole, "the tray menu was only partly replaced");
    Ok(())
}

/// Copies the proxy environment for the port the facade reports.
fn copy_clash_env(app_handle: &AppHandle, option: CopyEnvOption) {
    let Some(client) = app_handle.try_state::<NyanpasuClient>() else {
        tracing::warn!("the tray copied the proxy env before the client was ready");
        return;
    };
    proxy_env::copy_clash_env(app_handle, client.clash_info().port, &option);
}

/// Restarts the core from the tray menu.
fn restart_core(app_handle: &AppHandle) {
    let Some(client) = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone())
    else {
        log::warn!(target: "app", "the core restart fired before the client was ready");
        return;
    };
    // The clash view refresh follows from the effects every core lifecycle
    // command publishes.
    tauri::async_runtime::spawn(async move {
        if let Err(err) = client.reconcile_core().await {
            log::error!(target:"app", "{err:?}");
        }
    });
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
