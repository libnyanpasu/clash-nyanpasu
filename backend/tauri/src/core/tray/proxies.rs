use crate::core::{
    clash::proxies::{Proxies, ProxiesGuard, ProxiesGuardExt},
    handle::Handle,
};
use indexmap::IndexMap;
use log::{debug, error, warn};
use tauri::SystemTrayMenu;

async fn loop_task() {
    loop {
        match ProxiesGuard::global().update().await {
            Ok(_) => {
                debug!(target: "tray", "update proxies success");
            }
            Err(e) => {
                warn!(target: "tray", "update proxies failed: {:?}", e);
            }
        }
        {
            let guard = ProxiesGuard::global().read();
            if guard.updated_at() == 0 {
                error!(target: "tray", "proxies not updated yet!!!!");
                // TODO: add a error dialog or notification, and panic?
            }

            // else {
            //     let proxies = guard.inner();
            //     let str = simd_json::to_string_pretty(proxies).unwrap();
            //     debug!(target: "tray", "proxies info: {:?}", str);
            // }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await; // TODO: add a config to control the interval
    }
}

type GroupName = String;
type FromProxy = String;
type ToProxy = String;
type ProxySelectAction = (GroupName, FromProxy, ToProxy);
#[derive(PartialEq)]
enum TrayUpdateType {
    None,
    Full,
    Part(Vec<ProxySelectAction>),
}

struct TrayProxyItem {
    current: Option<String>,
    all: Vec<String>,
}
type TrayProxies = IndexMap<String, TrayProxyItem>;

/// Convert raw proxies to tray proxies
fn to_tray_proxies(mode: &str, raw_proxies: &Proxies) -> TrayProxies {
    let mut tray_proxies = TrayProxies::new();
    if matches!(mode, "global" | "rule" | "script") {
        if mode == "global" || raw_proxies.proxies.is_empty() {
            let global = TrayProxyItem {
                current: raw_proxies.global.now.clone(),
                all: raw_proxies
                    .global
                    .all
                    .clone()
                    .into_iter()
                    .map(|x| x.name)
                    .collect(),
            };
            tray_proxies.insert("global".to_owned(), global);
        }
        for raw_group in raw_proxies.groups.iter() {
            let group = TrayProxyItem {
                current: raw_group.now.clone(),
                all: raw_group.all.clone().into_iter().map(|x| x.name).collect(),
            };
            tray_proxies.insert(raw_group.name.to_owned(), group);
        }
    }
    tray_proxies
}

fn diff_proxies(old_proxies: &TrayProxies, new_proxies: &TrayProxies) -> TrayUpdateType {
    // 1. check if the length of two map is different
    if old_proxies.len() != new_proxies.len() {
        return TrayUpdateType::Full;
    }
    // 2. check if the group matching
    let group_matching = new_proxies
        .keys()
        .cloned()
        .collect::<Vec<String>>()
        .iter()
        .zip(&old_proxies.keys().cloned().collect::<Vec<String>>())
        .filter(|&(new, old)| new == old)
        .count();
    if group_matching != old_proxies.len() {
        return TrayUpdateType::Full;
    }
    // 3. start checking the group content
    let mut actions = Vec::new();
    for (group, item) in new_proxies.iter() {
        let old_item = old_proxies.get(group).unwrap(); // safe to unwrap

        // check if the length of all list is different
        if item.all.len() != old_item.all.len() {
            return TrayUpdateType::Full;
        }

        // first diff the all list
        let all_matching = item
            .all
            .iter()
            .zip(&old_item.all)
            .filter(|&(new, old)| new == old)
            .count();
        if all_matching != old_item.all.len() {
            return TrayUpdateType::Full;
        }
        // then diff the current
        if item.current != old_item.current {
            actions.push((
                group.clone(),
                old_item.current.clone().unwrap(),
                item.current.clone().unwrap(),
            ));
        }
    }
    if actions.is_empty() {
        TrayUpdateType::None
    } else {
        TrayUpdateType::Part(actions)
    }
}

pub async fn proxies_updated_receiver() {
    let (mut rx, mut tray_proxies_holder) = {
        let guard = ProxiesGuard::global().read();
        let proxies = guard.inner().to_owned();
        let mode = crate::utils::config::get_current_clash_mode();
        (
            guard.get_receiver(),
            to_tray_proxies(mode.as_str(), &proxies),
        )
    };

    loop {
        match rx.recv().await {
            Ok(_) => {
                debug!(target: "tray::proxies", "proxies updated");
                if Handle::global().app_handle.lock().is_none() {
                    warn!(target: "tray::proxies", "app handle not found");
                    continue;
                }
                Handle::mutate_proxies();
                // Do diff check
                let mode = crate::utils::config::get_current_clash_mode();
                let current_tray_proxies =
                    to_tray_proxies(mode.as_str(), ProxiesGuard::global().read().inner());

                match diff_proxies(&tray_proxies_holder, &current_tray_proxies) {
                    TrayUpdateType::Full => {
                        debug!(target: "tray::proxies", "should do full update");
                        tray_proxies_holder = current_tray_proxies;
                        match Handle::update_systray() {
                            Ok(_) => {
                                debug!(target: "tray::proxies", "update systray success");
                            }
                            Err(e) => {
                                warn!(target: "tray::proxies", "update systray failed: {:?}", e);
                            }
                        }
                    }
                    TrayUpdateType::Part(action_list) => {
                        debug!(target: "tray::proxies", "should do partial update, op list: {:?}", action_list);
                        tray_proxies_holder = current_tray_proxies;
                        platform_impl::update_selected_proxies(&action_list);
                        debug!(target: "tray::proxies", "update selected proxies success");
                    }
                    _ => {}
                }
            }
            Err(e) => {
                warn!(target: "tray::proxies", "proxies updated receiver failed: {:?}", e);
            }
        }
    }
}

pub fn setup_proxies() {
    tauri::async_runtime::spawn(loop_task());
    tauri::async_runtime::spawn(proxies_updated_receiver());
}

mod platform_impl {
    use super::{ProxySelectAction, TrayProxyItem};
    use crate::core::{clash::proxies::ProxiesGuard, handle::Handle};
    use tauri::{CustomMenuItem, SystemTrayMenu, SystemTraySubmenu};
    pub fn generate_group_selector(group_name: &str, group: &TrayProxyItem) -> SystemTraySubmenu {
        let mut group_menu = SystemTrayMenu::new();
        for item in group.all.iter() {
            let mut sub_item = CustomMenuItem::new(
                format!("select_proxy_{}_{}", group_name, item),
                item.clone(),
            );
            if let Some(now) = group.current.clone() {
                if now == item.as_str() {
                    sub_item = sub_item.selected();
                }
            }
            group_menu = group_menu.add_item(sub_item);
        }
        SystemTraySubmenu::new(group_name.to_string(), group_menu)
    }

    pub fn generate_selectors(
        menu: &SystemTrayMenu,
        proxies: &super::TrayProxies,
    ) -> SystemTrayMenu {
        let mut menu = menu.to_owned();
        if proxies.is_empty() {
            return menu.add_item(CustomMenuItem::new("no_proxies", "No Proxies"));
        }
        for (group, item) in proxies.iter() {
            let group_menu = generate_group_selector(group, item);
            menu = menu.add_submenu(group_menu);
        }
        menu
    }

    pub fn setup_tray(menu: &mut SystemTrayMenu) -> SystemTrayMenu {
        let proxies = ProxiesGuard::global().read().inner().to_owned();
        let mode = crate::utils::config::get_current_clash_mode();
        let tray_proxies = super::to_tray_proxies(mode.as_str(), &proxies);
        generate_selectors(menu, &tray_proxies)
    }

    pub fn update_selected_proxies(actions: &[ProxySelectAction]) {
        let tray = Handle::global()
            .app_handle
            .lock()
            .as_ref()
            .unwrap()
            .tray_handle();
        for action in actions {
            let from = format!("select_proxy_{}_{}", action.0, action.1);
            let to = format!("select_proxy_{}_{}", action.0, action.2);
            if let Some(item) = tray.try_get_item(&from) {
                let _ = item.set_selected(false);
            }
            let _ = tray.get_item(&to).set_selected(true);
        }
    }
}

pub trait SystemTrayMenuProxiesExt {
    fn setup_proxies(&mut self) -> Self;
}

impl SystemTrayMenuProxiesExt for SystemTrayMenu {
    fn setup_proxies(&mut self) -> Self {
        platform_impl::setup_tray(self)
    }
}

pub fn on_system_tray_event(event: &str) {
    if !event.starts_with("select_proxy_") {
        return; // bypass non-select event
    }
    let parts: Vec<&str> = event.split('_').collect();
    if parts.len() != 4 {
        return; // bypass invalid event
    }
    let group = parts[2].to_owned();
    let name = parts[3].to_owned();
    tauri::async_runtime::spawn(async move {
        match ProxiesGuard::global().select_proxy(&group, &name).await {
            Ok(_) => {
                debug!(target: "tray", "select proxy success: {} {}", group, name);
            }
            Err(e) => {
                warn!(target: "tray", "select proxy failed, {} {}, cause: {:?}", group, name, e);
                // TODO: add a error dialog or notification
            }
        }
    });
}
