use super::{
    Tray, TrayState, TrayWork,
    display::{Paint, ProxyItem, ProxySection, Shown},
};
use crate::{client::effects::plan::TrayView, core::clash::proxies::Proxies, log_err};
use indexmap::IndexMap;
use nyanpasu_config::{application::ProxiesSelectorMode, clash::config::overrides::Mode};
use std::ops::ControlFlow;
use tauri::{AppHandle, Emitter, Manager, Runtime, menu::MenuBuilder};
use tracing::{debug, error, warn};
use tracing_attributes::instrument;

type GroupName = String;
type ProxyName = String;
/// Maps each proxy node to the id of its menu item.
// TODO: use Cow<str> instead of String
pub(super) type ProxyItemIds = bimap::BiMap<(GroupName, ProxyName), usize>;
type FromProxy = ProxyName;
type ToProxy = ProxyName;
type ProxySelectAction = (GroupName, FromProxy, ToProxy);
#[derive(Debug, PartialEq)]
pub(super) enum TrayUpdateType {
    None,
    Full,
    Part(Vec<ProxySelectAction>),
}

pub(super) struct TrayProxyItem {
    pub(super) current: Option<String>,
    pub(super) all: Vec<String>,
    pub(super) r#type: String, // TODO: 转成枚举
}
pub(super) type TrayProxies = IndexMap<String, TrayProxyItem>;

/// Convert raw proxies to tray proxies
fn to_tray_proxies(mode: Mode, raw_proxies: &Proxies) -> TrayProxies {
    let mut tray_proxies = TrayProxies::new();
    if matches!(mode, Mode::Global | Mode::Rule | Mode::Script) {
        if mode == Mode::Global || raw_proxies.proxies.is_empty() {
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

pub(super) fn diff_proxies(old_proxies: &TrayProxies, new_proxies: &TrayProxies) -> TrayUpdateType {
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
            // A selection that appears or disappears has no pair of items to
            // switch between, so only a rebuild shows it.
            let (Some(from), Some(to)) = (&old_item.current, &item.current) else {
                return TrayUpdateType::Full;
            };
            actions.push((group.clone(), from.clone(), to.clone()));
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
    while rx.changed().await.is_ok() {
        let _ = app_handle.emit(
            crate::client::STATE_CHANGED_URI,
            crate::client::StateChanged::Proxies,
        );
        log_err!(Tray::request(&app_handle, TrayWork::PROXIES));
    }
}

/// Brings the tray's proxy selection up to the latest proxy snapshot.
///
/// A step of the tray drain, as rebuilds are, so it never interleaves with
/// one and diffs against the selection the published menu shows. A rebuild
/// that read an older snapshot is corrected by the reconcile that the newer
/// snapshot requested behind it.
pub(super) fn repaint_proxies(
    app_handle: &AppHandle,
    state: &TrayState<tauri::Wry>,
) -> anyhow::Result<ControlFlow<()>> {
    let view = *state.view.lock();
    if view.menu.selector_mode == ProxiesSelectorMode::Hidden {
        return Ok(ControlFlow::Continue(()));
    }
    let snapshot = app_handle
        .state::<crate::client::NyanpasuClient>()
        .proxies_snapshot();
    let current = to_tray_proxies(view.part.mode, &snapshot);
    let update = state.display.lock().update_to(&current);
    match update {
        // The rebuild records what it publishes.
        TrayUpdateType::Full => {
            Tray::request(app_handle, TrayWork::REBUILD)?;
            Ok(ControlFlow::Break(()))
        }
        TrayUpdateType::Part(actions) => {
            let shown = platform_impl::update_selected_proxies(state, &actions);
            state.display.lock().repainted(Some(current), shown);
            Ok(match shown {
                Shown::Whole => ControlFlow::Continue(()),
                Shown::Partly => ControlFlow::Break(()),
            })
        }
        TrayUpdateType::None => Ok(ControlFlow::Continue(())),
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
    use super::{
        GroupName, Paint, ProxyItemIds, ProxyName, ProxySection, ProxySelectAction, Shown,
        TrayProxyItem,
    };
    use crate::{client::effects::plan::TrayView, core::tray::TrayState};
    use nyanpasu_config::application::ProxiesSelectorMode;
    use rust_i18n::t;
    use tauri::{
        AppHandle, Manager, Runtime,
        menu::{
            CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, Submenu,
            SubmenuBuilder,
        },
    };
    use tracing::warn;

    pub fn generate_group_selector<R: Runtime>(
        app_handle: &AppHandle<R>,
        item_ids: &mut ProxyItemIds,
        group_name: &str,
        group: &TrayProxyItem,
    ) -> anyhow::Result<Submenu<R>> {
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

    /// The selector items, with the node behind each of their ids.
    pub fn generate_selectors<R: Runtime>(
        app_handle: &AppHandle<R>,
        proxies: &super::TrayProxies,
    ) -> anyhow::Result<(Vec<MenuItemKind<R>>, ProxyItemIds)> {
        let mut items = Vec::new();
        let mut item_ids = ProxyItemIds::new();
        if proxies.is_empty() {
            items.push(MenuItemKind::MenuItem(
                MenuItemBuilder::new(t!("tray.no_proxies"))
                    .id("no_proxies")
                    .enabled(false)
                    .build(app_handle)?,
            ));
            return Ok((items, item_ids));
        }
        for (group, item) in proxies.iter() {
            let group_menu = generate_group_selector(app_handle, &mut item_ids, group, item)?;
            items.push(MenuItemKind::Submenu(group_menu));
        }
        Ok((items, item_ids))
    }

    /// Adds the proxy section, returned apart so that it is recorded only
    /// once the menu is published.
    pub fn setup_tray<'m, R: Runtime, M: Manager<R>>(
        app_handle: &AppHandle<R>,
        view: &TrayView,
        mut menu: MenuBuilder<'m, R, M>,
    ) -> anyhow::Result<(MenuBuilder<'m, R, M>, ProxySection)> {
        let selector_mode = view.menu.selector_mode;
        menu = match selector_mode {
            ProxiesSelectorMode::Hidden => return Ok((menu, ProxySection::default())),
            ProxiesSelectorMode::Normal => menu.separator(),
            ProxiesSelectorMode::Submenu => menu,
        };
        let proxies = app_handle
            .state::<crate::client::NyanpasuClient>()
            .proxies_snapshot();
        let tray_proxies = super::to_tray_proxies(view.part.mode, &proxies);
        let (items, item_ids) = generate_selectors::<R>(app_handle, &tray_proxies)?;
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
        // Tray steps run one at a time, so no proxy reconcile can land between
        // this snapshot and publishing the menu built from it.
        Ok((
            menu,
            ProxySection {
                proxies: tray_proxies,
                item_ids,
            },
        ))
    }

    /// Only the proxy reconcile step calls this, so no rebuild can replace
    /// the menu while it repaints it. Returns how much of the change took.
    #[tracing_attributes::instrument(skip(state))]
    pub fn update_selected_proxies(
        state: &TrayState<tauri::Wry>,
        actions: &[ProxySelectAction],
    ) -> Shown {
        let Paint::Menu(attached) = state.display.lock().paint() else {
            warn!("the tray menu is not built or not known, skip this update");
            return Shown::Partly;
        };
        let menu = attached.menu;
        let mut shown = Shown::Whole;
        // comment it just because we could not get the access to the menu item via the id
        // If the tauri team fixes this issue, we could use the following code to update the tray item
        // let item_ids = app_handle.state::<TrayState<tauri::Wry>>().item_ids.lock();
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
                    if let Err(error) = item.set_checked(false) {
                        warn!("failed to deselect {} {}: {error}", action.0, action.1);
                        shown = Shown::Partly;
                    }
                }
                None => {
                    warn!(
                        "failed to deselect, item not found: {} {}",
                        action.0, action.1
                    );
                    shown = Shown::Partly;
                }
            }

            let to_item = find_check_item(&menu, action.0.clone(), action.2.clone());
            match to_item {
                Some(item) => {
                    if let Err(error) = item.set_checked(true) {
                        warn!("failed to select {} {}: {error}", action.0, action.2);
                        shown = Shown::Partly;
                    }
                }
                None => {
                    warn!(
                        "failed to select, item not found: {} {}",
                        action.0, action.2
                    );
                    shown = Shown::Partly;
                }
            }
        }
        shown
    }
}

pub(super) trait SystemTrayMenuProxiesExt<R: Runtime> {
    fn setup_proxies(
        self,
        app_handle: &AppHandle<R>,
        view: &TrayView,
    ) -> anyhow::Result<(Self, ProxySection)>
    where
        Self: Sized;
}

impl<R: Runtime, M: Manager<R>> SystemTrayMenuProxiesExt<R> for MenuBuilder<'_, R, M> {
    fn setup_proxies(
        self,
        app_handle: &AppHandle<R>,
        view: &TrayView,
    ) -> anyhow::Result<(Self, ProxySection)> {
        platform_impl::setup_tray(app_handle, view, self)
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

    let item = app_handle
        .state::<TrayState<tauri::Wry>>()
        .display
        .lock()
        .proxy_item(node_id);
    let (group, name) = match item {
        ProxyItem::Node { group, name } => (group, name),
        ProxyItem::NotInMenu => {
            error!("node id not found: {}", node_id);
            return;
        }
        ProxyItem::Unknown => {
            warn!("ignored proxy item {node_id}: the tray menu is unknown until it is rebuilt");
            return;
        }
    };

    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    tauri::async_runtime::spawn(async move {
        debug!("received select proxy event: {} {}", group, name);
        match client.select_proxy(group.clone(), name.clone()).await {
            Ok(outcome) => {
                debug!("select proxy success: {} {}", group, name);
                for degradation in outcome.degradations() {
                    warn!(code = %degradation.code, message = %degradation.message, "proxy selection degraded");
                }
            }
            Err(error) => error!("select proxy failed, {} {}: {:#}", group, name, error),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selecting(current: Option<&str>) -> TrayProxies {
        TrayProxies::from([(
            "Proxy".to_owned(),
            TrayProxyItem {
                current: current.map(str::to_owned),
                all: vec!["a".to_owned(), "b".to_owned()],
                r#type: "Selector".to_owned(),
            },
        )])
    }

    /// The diff runs inside the main-thread drain, where a panic could take
    /// the app down.
    #[test]
    fn a_selection_that_appears_or_disappears_needs_a_rebuild() {
        let (none, a, b) = (selecting(None), selecting(Some("a")), selecting(Some("b")));
        assert_eq!(diff_proxies(&none, &a), TrayUpdateType::Full);
        assert_eq!(diff_proxies(&a, &none), TrayUpdateType::Full);
        assert_eq!(
            diff_proxies(&a, &b),
            TrayUpdateType::Part(vec![("Proxy".to_owned(), "a".to_owned(), "b".to_owned())])
        );
        assert_eq!(diff_proxies(&none, &none), TrayUpdateType::None);
    }
}
