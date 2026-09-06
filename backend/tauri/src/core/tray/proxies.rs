// TODO(actor-migration): compatibility bridge for legacy tray settings snapshots.
// Reason: synchronous menu construction still reads the legacy settings mirror.
// Remove when: tray settings snapshots are injected into menu construction.
use crate::{
    config::{Config, nyanpasu::ProxiesSelectorMode},
    core::clash::proxies::Proxies,
};
use indexmap::IndexMap;
use tauri::{AppHandle, Emitter, Manager, Runtime, menu::MenuBuilder};
use tracing::{debug, error, warn};
use tracing_attributes::instrument;

type GroupName = String;
type ProxyName = String;
type FromProxy = ProxyName;
type ToProxy = ProxyName;
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
    r#type: String, // TODO: 转成枚举
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
                    .iter()
                    .map(|x| x.name.to_owned())
                    .collect(),
                r#type: "Selector".to_string(),
            };
            tray_proxies.insert("global".to_owned(), global);
        }
        for raw_group in raw_proxies.groups.iter() {
            let group = TrayProxyItem {
                current: raw_group.now.clone(),
                all: raw_group.all.iter().map(|x| x.name.to_owned()).collect(),
                r#type: raw_group.r#type.clone(),
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

#[instrument(skip(app_handle, client))]
pub async fn proxies_updated_receiver(
    app_handle: AppHandle,
    client: crate::client::NyanpasuClient,
) {
    let mut rx = client.subscribe_proxy_changes();
    let mode = crate::utils::config::get_current_clash_mode();
    let mut tray_proxies_holder = to_tray_proxies(mode.as_str(), &client.proxies_snapshot());
    while rx.changed().await.is_ok() {
        let _ = app_handle.emit(
            crate::core::handle::STATE_CHANGED_URI,
            crate::core::handle::StateChanged::Proxies,
        );
        let is_tray_selector_enabled = Config::verge()
            .latest()
            .clash_tray_selector
            .unwrap_or_default()
            != ProxiesSelectorMode::Hidden;
        if !is_tray_selector_enabled {
            continue;
        }
        let mode = crate::utils::config::get_current_clash_mode();
        let current = to_tray_proxies(mode.as_str(), &client.proxies_snapshot());
        match diff_proxies(&tray_proxies_holder, &current) {
            TrayUpdateType::Full => {
                let _ = app_handle.emit("update_systray", ());
            }
            TrayUpdateType::Part(actions) => platform_impl::update_selected_proxies(&actions),
            TrayUpdateType::None => {}
        }
        tray_proxies_holder = current;
    }
}

pub fn setup_proxies(app_handle: &AppHandle) {
    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    client.request_proxy_refresh();
    tauri::async_runtime::spawn(proxies_updated_receiver(app_handle.clone(), client));
}

mod platform_impl {
    use super::{GroupName, ProxyName, ProxySelectAction, TrayProxyItem};
    use crate::{config::nyanpasu::ProxiesSelectorMode, core::handle::Handle};
    use bimap::BiMap;
    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use rust_i18n::t;
    use std::sync::atomic::AtomicBool;
    use tauri::{
        AppHandle, Manager, Runtime,
        menu::{
            CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, Submenu,
            SubmenuBuilder,
        },
    };
    use tracing::warn;

    // It store a map of proxy nodes like "GROUP_PROXY" -> ID
    // TODO: use Cow<str> instead of String
    pub(super) static ITEM_IDS: Lazy<Mutex<BiMap<(GroupName, ProxyName), usize>>> =
        Lazy::new(|| Mutex::new(BiMap::new()));

    pub fn generate_group_selector<R: Runtime>(
        app_handle: &AppHandle<R>,
        group_name: &str,
        group: &TrayProxyItem,
    ) -> anyhow::Result<Submenu<R>> {
        let mut item_ids = ITEM_IDS.lock();
        let mut group_menu = SubmenuBuilder::new(app_handle, group_name);
        if group.all.is_empty() {
            group_menu = group_menu.item(
                &MenuItemBuilder::new(t!("tray.no_proxies"))
                    .enabled(false)
                    .build(app_handle)?,
            );
            return Ok(group_menu.build()?);
        }
        for item in group.all.iter() {
            let key = (group_name.to_string(), item.to_string());
            let id = item_ids.len();
            item_ids.insert(key, id);
            let mut sub_item_builder = CheckMenuItemBuilder::new(item.clone())
                .id(format!("proxy_node_{id}"))
                .checked(false);
            if let Some(now) = group.current.clone()
                && now == item.as_str()
            {
                sub_item_builder = sub_item_builder.checked(true);
            }

            if !matches!(group.r#type.as_str(), "Selector" | "Fallback") {
                sub_item_builder = sub_item_builder.enabled(false);
            }

            group_menu = group_menu.item(&sub_item_builder.build(app_handle)?);
        }
        Ok(group_menu.build()?)
    }

    pub fn generate_selectors<R: Runtime>(
        app_handle: &AppHandle<R>,
        proxies: &super::TrayProxies,
    ) -> anyhow::Result<Vec<MenuItemKind<R>>> {
        let mut items = Vec::new();
        if proxies.is_empty() {
            items.push(MenuItemKind::MenuItem(
                MenuItemBuilder::new(t!("tray.no_proxies"))
                    .id("no_proxies")
                    .enabled(false)
                    .build(app_handle)?,
            ));
            return Ok(items);
        }
        {
            let mut item_ids = ITEM_IDS.lock();
            item_ids.clear(); // clear the item ids
        }
        for (group, item) in proxies.iter() {
            let group_menu = generate_group_selector(app_handle, group, item)?;
            items.push(MenuItemKind::Submenu(group_menu));
        }
        Ok(items)
    }

    pub fn setup_tray<'m, R: Runtime, M: Manager<R>>(
        app_handle: &AppHandle<R>,
        mut menu: MenuBuilder<'m, R, M>,
    ) -> anyhow::Result<MenuBuilder<'m, R, M>> {
        let selector_mode = crate::config::Config::verge()
            .latest()
            .clash_tray_selector
            .unwrap_or_default();
        menu = match selector_mode {
            ProxiesSelectorMode::Hidden => return Ok(menu),
            ProxiesSelectorMode::Normal => menu.separator(),
            ProxiesSelectorMode::Submenu => menu,
        };
        let proxies = app_handle
            .state::<crate::client::NyanpasuClient>()
            .proxies_snapshot();
        let mode = crate::utils::config::get_current_clash_mode();
        let tray_proxies = super::to_tray_proxies(mode.as_str(), &proxies);
        let items = generate_selectors::<R>(app_handle, &tray_proxies)?;
        match selector_mode {
            ProxiesSelectorMode::Normal => {
                for item in items {
                    menu = menu.item(&item);
                }
            }
            ProxiesSelectorMode::Submenu => {
                let mut submenu = SubmenuBuilder::with_id(
                    app_handle,
                    "select_proxies",
                    t!("tray.select_proxies"),
                );
                for item in items {
                    submenu = submenu.item(&item);
                }
                menu = menu.item(&submenu.build()?);
            }
            _ => {}
        }
        Ok(menu)
    }

    static TRAY_ITEM_UPDATE_BARRIER: AtomicBool = AtomicBool::new(false);

    #[tracing_attributes::instrument]
    pub fn update_selected_proxies(actions: &[ProxySelectAction]) {
        if TRAY_ITEM_UPDATE_BARRIER.load(std::sync::atomic::Ordering::Acquire) {
            warn!("tray item update is in progress, skip this update");
            return;
        }
        let app_handle = Handle::global().app_handle.lock();
        let tray_state = app_handle
            .as_ref()
            .unwrap()
            .state::<crate::core::tray::TrayState<tauri::Wry>>();
        TRAY_ITEM_UPDATE_BARRIER.store(true, std::sync::atomic::Ordering::Release);
        let menu = tray_state.menu.lock();
        // comment it just because we could not get the access to the menu item via the id
        // If the tauri team fixes this issue, we could use the following code to update the tray item
        // let item_ids = ITEM_IDS.lock();
        for action in actions {
            //     #[cfg(not(target_os = "linux"))]
            //     {
            //         tracing::debug!("update selected proxies: {:?}", action);
            //         let from_id = match item_ids.get_by_left(&(action.0.clone(), action.1.clone())) {
            //             Some(id) => *id,
            //             None => {
            //                 warn!("from item not found: {:?}", action);
            //                 continue;
            //             }
            //         };
            //         let from_id = format!("proxy_node_{}", from_id);

            //         let to_id = match item_ids.get_by_left(&(action.0.clone(), action.2.clone())) {
            //             Some(id) => *id,
            //             None => {
            //                 warn!("to item not found: {:?}", action);
            //                 continue;
            //             }
            //         };
            //         let to_id = format!("proxy_node_{}", to_id);

            //         match menu.get(&from_id) {
            //             Some(item) => match item.kind() {
            //                 MenuItemKind::Check(item) => {
            //                     if item.is_checked().is_ok_and(|x| x) {
            //                         let _ = item.set_checked(false);
            //                     }
            //                 }
            //                 MenuItemKind::MenuItem(item) => {
            //                     let _ = item.set_text(action.1.clone());
            //                 }
            //                 _ => {
            //                     warn!("failed to deselect, item is not a check item: {}", from_id);
            //                 }
            //             },
            //             None => {
            //                 warn!("failed to deselect, item not found: {}", from_id);
            //             }
            //         }
            //         match menu.get(&to_id) {
            //             Some(item) => match item.kind() {
            //                 MenuItemKind::Check(item) => {
            //                     if item.is_checked().is_ok_and(|x| !x) {
            //                         let _ = item.set_checked(true);
            //                     }
            //                 }
            //                 MenuItemKind::MenuItem(item) => {
            //                     let _ = item.set_text(action.2.clone());
            //                 }
            //                 _ => {
            //                     warn!("failed to select, item is not a check item: {}", to_id);
            //                 }
            //             },
            //             None => {
            //                 warn!("failed to select, item not found: {}", to_id);
            //             }
            //         }
            //     }
            // }

            // here is a fucking workaround for id getter
            #[inline]
            fn find_check_item<R: Runtime>(
                menu: &Menu<R>,
                group: GroupName,
                proxy: ProxyName,
            ) -> Option<tauri::menu::CheckMenuItem<R>> {
                menu.items()
                    .ok()
                    .and_then(|items| {
                        items.into_iter().find(|i| matches!(i, tauri::menu::MenuItemKind::Submenu(submenu) if submenu.text().is_ok_and(|text| text == group) || submenu.id() == "select_proxies"))
                    })
                    .and_then(|submenu| {
                        let submenu = submenu.as_submenu_unchecked();
                        if submenu.id() == "select_proxies" {
                            submenu.items().ok().and_then(|items| {
                                items.into_iter().find(|i| matches!(i, tauri::menu::MenuItemKind::Submenu(submenu) if submenu.text().is_ok_and(|text| text == group)))
                            })
                            .and_then(|submenu| {
                                submenu.as_submenu_unchecked().items().ok()
                            })
                        } else {
                            submenu.items().ok()
                        }
                    })
                    .and_then(|items| {
                        items.into_iter().find(|i| matches!(i, tauri::menu::MenuItemKind::Check(item) if item.text().is_ok_and(|text| text == proxy)))
                    }).map(|item| item.as_check_menuitem_unchecked().clone())
            }

            let from_item = find_check_item(&menu, action.0.clone(), action.1.clone());
            match from_item {
                Some(item) => {
                    let _ = item.set_checked(false);
                }
                None => {
                    warn!(
                        "failed to deselect, item not found: {} {}",
                        action.0, action.1
                    );
                }
            }

            let to_item = find_check_item(&menu, action.0.clone(), action.2.clone());
            match to_item {
                Some(item) => {
                    let _ = item.set_checked(true);
                }
                None => {
                    warn!(
                        "failed to select, item not found: {} {}",
                        action.0, action.2
                    );
                }
            }
        }
        TRAY_ITEM_UPDATE_BARRIER.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub trait SystemTrayMenuProxiesExt<R: Runtime> {
    fn setup_proxies(self, app_handle: &AppHandle<R>) -> anyhow::Result<Self>
    where
        Self: Sized;
}

impl<R: Runtime, M: Manager<R>> SystemTrayMenuProxiesExt<R> for MenuBuilder<'_, R, M> {
    fn setup_proxies(self, app_handle: &AppHandle<R>) -> anyhow::Result<Self> {
        platform_impl::setup_tray(app_handle, self)
    }
}

#[instrument]
pub fn on_system_tray_event(app_handle: &AppHandle, event: &str) {
    if !event.starts_with("proxy_node_") {
        return; // bypass non-select event
    }
    let node_id = event.split('_').next_back().unwrap(); // safe to unwrap
    let node_id = match node_id.parse::<usize>() {
        Ok(id) => id,
        Err(e) => {
            error!("parse node id failed: {:?}", e);
            return;
        }
    };

    let (group, name) = {
        let map = platform_impl::ITEM_IDS.lock();
        let item = map.get_by_right(&node_id);
        match item {
            Some((group, name)) => (group.clone(), name.clone()),
            None => {
                error!("node id not found: {}", node_id);
                return;
            }
        }
    };

    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    tauri::async_runtime::spawn(async move {
        debug!("received select proxy event: {} {}", group, name);
        match client.select_proxy(group.clone(), name.clone()).await {
            Ok(()) => debug!("select proxy success: {} {}", group, name),
            Err(error) => error!("select proxy failed, {} {}: {:#}", group, name, error),
        }
    });
}
