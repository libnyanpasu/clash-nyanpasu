use super::{
    Tray, TrayState, TrayWork,
    display::{Paint, ProxySection, Shown},
};
use crate::{
    client::effects::plan::TrayView,
    core::clash::proxies::{Proxies, ProxyGroup},
    log_err,
};
use indexmap::IndexMap;
use nyanpasu_config::{application::ProxiesSelectorMode, clash::config::overrides::Mode};
use std::ops::ControlFlow;
use tauri::{AppHandle, Emitter, Manager, Runtime, menu::MenuBuilder};
use tracing::{debug, error, warn};
use tracing_attributes::instrument;

type GroupName = String;
type ProxyName = String;
type FromProxy = ProxyName;
type ToProxy = ProxyName;
type ProxySelectAction = (GroupName, FromProxy, ToProxy);

const NODE_ITEM_ID_PREFIX: &str = "proxy_node:";
const GROUP_MENU_ID_PREFIX: &str = "proxy_group:";
const UNFIX_ITEM_ID_PREFIX: &str = "proxy_unfix:";

/// A node item's id names its group and node, so a click needs no lookup.
fn node_item_id(group: &str, node: &str) -> String {
    format!("{NODE_ITEM_ID_PREFIX}{}", serde_json::json!([group, node]))
}

fn group_menu_id(group: &str) -> String {
    format!("{GROUP_MENU_ID_PREFIX}{group}")
}

/// The group and node an id names; `None` for any other menu item.
fn parse_node_item_id(id: &str) -> Option<(String, String)> {
    serde_json::from_str(id.strip_prefix(NODE_ITEM_ID_PREFIX)?).ok()
}

/// The id of a group's "restore automatic selection" item. The group name is
/// the whole rest of the id, so it needs no escaping.
fn unfix_item_id(group: &str) -> String {
    format!("{UNFIX_ITEM_ID_PREFIX}{group}")
}

/// The group an unfix item id names; `None` for any other menu item.
fn parse_unfix_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(UNFIX_ITEM_ID_PREFIX)
}

/// A node's menu text; the pinned member carries a pin.
fn node_item_text(node: &str, fixed: Option<&str>) -> String {
    if fixed == Some(node) {
        format!("{node} 📌")
    } else {
        node.to_owned()
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum TrayUpdateType {
    None,
    Full,
    Part(Vec<ProxySelectAction>),
}

pub(super) struct TrayGroup {
    pub(super) now: Option<String>,
    pub(super) all: Vec<String>,
    /// Whether the core accepts choosing one of `all`.
    pub(super) selectable: bool,
    /// The member a user pinned, marked in the menu.
    pub(super) fixed: Option<String>,
    /// Whether the menu offers to return the group to automatic selection.
    pub(super) clear_fixed: bool,
}
pub(super) type TrayProxies = IndexMap<String, TrayGroup>;

impl TrayGroup {
    fn of(group: &ProxyGroup) -> Self {
        Self {
            now: group.now.as_ref().map(|name| name.as_str().to_owned()),
            all: group
                .all
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
            selectable: group.capabilities.select,
            fixed: group.fixed.as_ref().map(|name| name.as_str().to_owned()),
            clear_fixed: group.capabilities.clear_fixed,
        }
    }
}

/// The groups the tray lists, keyed by each group's real name.
fn to_tray_proxies(mode: Mode, proxies: &Proxies) -> TrayProxies {
    if !matches!(mode, Mode::Global | Mode::Rule | Mode::Script) {
        return TrayProxies::new();
    }
    // GLOBAL stays in Global mode even when hidden, as on the page.
    let global = proxies.global.as_ref().filter(|_| mode == Mode::Global);
    let groups = proxies.groups.iter().filter(|group| !group.hidden);
    global
        .into_iter()
        .chain(groups)
        .map(|group| (group.name.as_str().to_owned(), TrayGroup::of(group)))
        .collect()
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

        // check if the length of all list, the selectability, or the pin is different
        if item.all.len() != old_item.all.len()
            || item.selectable != old_item.selectable
            || item.fixed != old_item.fixed
            || item.clear_fixed != old_item.clear_fixed
        {
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
        if item.now != old_item.now {
            // A selection that appears or disappears has no pair of items to
            // switch between, so only a rebuild shows it.
            let (Some(from), Some(to)) = (&old_item.now, &item.now) else {
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
        Paint, ProxySection, ProxySelectAction, Shown, TrayGroup, group_menu_id, node_item_id,
        node_item_text, unfix_item_id,
    };
    use crate::{client::effects::plan::TrayView, core::tray::TrayState};
    use nyanpasu_config::application::ProxiesSelectorMode;
    use rust_i18n::t;
    use tauri::{
        AppHandle, Manager, Runtime,
        menu::{
            CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, MenuItemKind,
            Submenu, SubmenuBuilder,
        },
    };
    use tracing::warn;

    pub fn generate_group_selector<R: Runtime>(
        app_handle: &AppHandle<R>,
        group_name: &str,
        group: &TrayGroup,
    ) -> anyhow::Result<Submenu<R>> {
        let mut group_menu =
            SubmenuBuilder::with_id(app_handle, group_menu_id(group_name), group_name);
        if group.all.is_empty() {
            group_menu = group_menu.item(
                &MenuItemBuilder::new(t!("tray.no_proxies"))
                    .enabled(false)
                    .build(app_handle)?,
            );
            return Ok(group_menu.build()?);
        }
        if group.clear_fixed {
            group_menu = group_menu
                .item(
                    &MenuItemBuilder::with_id(
                        unfix_item_id(group_name),
                        t!("tray.restore_auto_selection"),
                    )
                    .enabled(group.fixed.is_some())
                    .build(app_handle)?,
                )
                .separator();
        }
        for item in group.all.iter() {
            let mut sub_item_builder =
                CheckMenuItemBuilder::new(node_item_text(item, group.fixed.as_deref()))
                    .id(node_item_id(group_name, item))
                    .checked(false);
            if let Some(now) = group.now.clone()
                && now == item.as_str()
            {
                sub_item_builder = sub_item_builder.checked(true);
            }

            if !group.selectable {
                sub_item_builder = sub_item_builder.enabled(false);
            }

            group_menu = group_menu.item(&sub_item_builder.build(app_handle)?);
        }
        Ok(group_menu.build()?)
    }

    /// The selector items.
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
        for (group, item) in proxies.iter() {
            let group_menu = generate_group_selector(app_handle, group, item)?;
            items.push(MenuItemKind::Submenu(group_menu));
        }
        Ok(items)
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
        let items = generate_selectors::<R>(app_handle, &tray_proxies)?;
        match selector_mode {
            ProxiesSelectorMode::Normal => {
                for item in items {
                    menu = menu.item(&item);
                }
            }
            ProxiesSelectorMode::Submenu => {
                let mut submenu =
                    SubmenuBuilder::with_id(app_handle, "select_proxy", t!("tray.select_proxy"));
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
            },
        ))
    }

    /// The check item of `node` in `group`'s submenu. Tauri's `get` searches
    /// direct children only, so this walks down by id.
    fn find_node_item<R: Runtime>(
        menu: &Menu<R>,
        group: &str,
        node: &str,
    ) -> Option<CheckMenuItem<R>> {
        let group_id = group_menu_id(group);
        // Normal mode lists the groups at the top; Submenu mode nests them in `select_proxy`.
        let group_menu = menu.get(group_id.as_str()).or_else(|| {
            menu.get("select_proxy")?
                .as_submenu()?
                .get(group_id.as_str())
        })?;
        group_menu
            .as_submenu()?
            .get(node_item_id(group, node).as_str())?
            .as_check_menuitem()
            .cloned()
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
        for action in actions {
            let from_item = find_node_item(&menu, &action.0, &action.1);
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

            let to_item = find_node_item(&menu, &action.0, &action.2);
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
    if let Some(group) = parse_unfix_item_id(event) {
        clear_fixed(app_handle, group.to_owned());
        return;
    }

    let Some((group, name)) = parse_node_item_id(event) else {
        if event.starts_with(NODE_ITEM_ID_PREFIX) {
            error!("malformed proxy item id: {event}");
        }
        return; // not a proxy item
    };

    // The platform menu has already flipped the clicked item; the next proxy
    // repaint sets it back from the selection.
    app_handle
        .state::<TrayState<tauri::Wry>>()
        .display
        .lock()
        .clicked(group.clone(), name.clone());

    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        debug!("received select proxy event: {} {}", group, name);
        match client.select_proxy(group.clone(), name.clone()).await {
            Ok(outcome) => {
                debug!("select proxy success: {} {}", group, name);
                for degradation in outcome.degradations() {
                    warn!(reason = ?degradation.reason, message = %degradation.message, "proxy selection degraded");
                }
            }
            Err(error) => error!("select proxy failed, {} {}: {:#}", group, name, error),
        }
        // Requested here, off the menu-event listener loop and after the core answered: a
        // rejected or no-op selection sends no proxy change notification.
        log_err!(Tray::request(&app_handle, TrayWork::PROXIES));
    });
}

/// Returns `group` to automatic selection. Like a selection, the tray
/// repaints once the core answered; a plain item is not flipped by the
/// platform menu, so nothing is recorded as clicked.
fn clear_fixed(app_handle: &AppHandle, group: String) {
    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        debug!("received clear pinned proxy event: {group}");
        match client.clear_proxy_fixed(group.clone()).await {
            Ok(outcome) => {
                for degradation in outcome.degradations() {
                    warn!(reason = ?degradation.reason, message = %degradation.message, "clearing the pinned proxy degraded");
                }
            }
            Err(error) => error!("clear pinned proxy failed, {group}: {error:#}"),
        }
        log_err!(Tray::request(&app_handle, TrayWork::PROXIES));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::clash::proxies::{ProxyGroupCapabilities, ProxyGroupKind};

    fn selecting(now: Option<&str>) -> TrayProxies {
        TrayProxies::from([(
            "Proxy".to_owned(),
            TrayGroup {
                now: now.map(str::to_owned),
                all: vec!["a".to_owned(), "b".to_owned()],
                selectable: true,
                fixed: None,
                clear_fixed: false,
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

    fn group(name: &str, all: &[&str], now: &str) -> ProxyGroup {
        ProxyGroup {
            name: name.into(),
            kind: ProxyGroupKind::Selector,
            all: all.iter().map(|&member| member.into()).collect(),
            now: Some(now.into()),
            fixed: None,
            hidden: false,
            icon: None,
            capabilities: ProxyGroupCapabilities {
                select: true,
                clear_fixed: false,
            },
        }
    }

    fn sample_proxies() -> Proxies {
        Proxies {
            global: Some(group("GLOBAL", &["GroupA"], "GroupA")),
            groups: vec![group("GroupA", &["node-a"], "node-a")],
            ..Default::default()
        }
    }

    /// The `proxies.is_empty()` check this replaced was always false (DIRECT
    /// and REJECT always populate it), so it always reduced to `mode ==
    /// Global`; the built menu must match that behavior exactly.
    #[test]
    fn global_mode_adds_a_global_entry_rule_mode_does_not() {
        let proxies = sample_proxies();

        let global_mode = to_tray_proxies(Mode::Global, &proxies);
        assert!(global_mode.contains_key("GLOBAL"));
        assert!(!global_mode.contains_key("global"));
        assert!(global_mode.contains_key("GroupA"));
        assert_eq!(global_mode["GLOBAL"].all, vec!["GroupA".to_owned()]);

        for mode in [Mode::Rule, Mode::Script] {
            let tray = to_tray_proxies(mode, &proxies);
            assert!(!tray.contains_key("GLOBAL"), "{mode:?}");
            assert!(!tray.contains_key("global"), "{mode:?}");
            assert!(tray.contains_key("GroupA"), "{mode:?}");
            assert_eq!(tray["GroupA"].all, vec!["node-a".to_owned()]);
        }
    }

    /// The core looks a group up by its exact, case-sensitive name, so the
    /// tray must select "GLOBAL", never a lowercase alias; a core without
    /// GLOBAL gets no entry.
    #[test]
    fn global_mode_selects_global_by_its_real_name() {
        let mut proxies = sample_proxies();
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GLOBAL", "GroupA"]
        );

        proxies.global = None;
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GroupA"]
        );
    }

    /// The page filters hidden groups out; the tray must not list them
    /// either, while GLOBAL itself always stays in Global mode.
    #[test]
    fn hidden_groups_stay_out_of_the_tray() {
        let mut proxies = sample_proxies();
        proxies.global.as_mut().unwrap().hidden = true;
        let mut hidden = group("Hidden", &["node-a"], "node-a");
        hidden.hidden = true;
        proxies.groups.push(hidden);
        for mode in [Mode::Global, Mode::Rule, Mode::Script] {
            assert!(!to_tray_proxies(mode, &proxies).contains_key("Hidden"));
        }
        assert!(to_tray_proxies(Mode::Global, &proxies).contains_key("GLOBAL"));
    }

    /// Enabling or disabling items changes the menu's structure, which only
    /// a rebuild shows.
    #[test]
    fn a_selectability_change_needs_a_rebuild() {
        let open = selecting(Some("a"));
        let mut locked = selecting(Some("a"));
        locked["Proxy"].selectable = false;
        assert_eq!(diff_proxies(&open, &locked), TrayUpdateType::Full);
    }

    /// Mihomo pins a URLTest group on selection, while Clash-rs rejects
    /// selecting a Fallback group; the tray follows what the core reports.
    #[test]
    fn only_groups_the_core_lets_a_user_select_are_selectable() {
        let mut proxies = sample_proxies();
        let mut pinnable = group("Auto", &["node-a"], "node-a");
        pinnable.kind = ProxyGroupKind::UrlTest;
        pinnable.capabilities.select = true;
        let mut automatic = group("Fallback", &["node-a"], "node-a");
        automatic.kind = ProxyGroupKind::Fallback;
        automatic.capabilities.select = false;
        proxies.groups = vec![pinnable, automatic];

        let tray = to_tray_proxies(Mode::Rule, &proxies);
        assert!(tray["Auto"].selectable);
        assert!(!tray["Fallback"].selectable);
    }

    #[test]
    fn node_item_ids_round_trip() {
        let names = [
            "Proxy",
            "a:b",
            "q\"uote",
            "back\\slash",
            "com,ma",
            "[bracket]",
            "sp ace",
            "\u{1F680}",
            "",
        ];
        for group in names {
            for node in names {
                assert_eq!(
                    parse_node_item_id(&node_item_id(group, node)),
                    Some((group.to_owned(), node.to_owned())),
                    "{group:?} / {node:?}"
                );
            }
        }
    }

    #[test]
    fn node_item_ids_are_distinct() {
        assert_ne!(node_item_id("a:b", "c"), node_item_id("a", "b:c"));
        assert_ne!(node_item_id("a\",\"b", "c"), node_item_id("a", "b\",\"c"));
    }

    #[test]
    fn other_menu_ids_are_not_node_items() {
        for id in [
            "rule_mode",
            "select_proxy",
            "no_proxies",
            "quit",
            group_menu_id("Proxy").as_str(),
            unfix_item_id("Proxy").as_str(),
            "proxy_node_3",
            "proxy_node:not json",
        ] {
            assert_eq!(parse_node_item_id(id), None, "{id:?}");
        }
    }

    #[test]
    fn unfix_item_ids_round_trip() {
        for group in [
            "Proxy",
            "a:b",
            "sp ace",
            "\u{1F680}",
            "",
            "proxy_node:[\"x\"]",
        ] {
            assert_eq!(parse_unfix_item_id(&unfix_item_id(group)), Some(group));
        }
    }

    #[test]
    fn node_and_unfix_ids_never_parse_as_each_other() {
        assert_eq!(parse_unfix_item_id(&node_item_id("G", "n")), None);
        assert_eq!(parse_node_item_id(&unfix_item_id("G")), None);
        assert_eq!(parse_unfix_item_id(&group_menu_id("G")), None);
    }

    /// The pin follows `fixed`, not `now`: a pinned URLTest member that is
    /// down keeps its pin while the check moves to the member in use.
    #[test]
    fn a_pinned_node_carries_a_pin() {
        assert_eq!(node_item_text("a", Some("a")), "a 📌");
        assert_eq!(node_item_text("b", Some("a")), "b");
        assert_eq!(node_item_text("a", None), "a");
    }

    #[test]
    fn tray_groups_carry_the_pin_and_whether_it_can_be_cleared() {
        let mut pinned = group("Auto", &["node-a", "node-b"], "node-b");
        pinned.kind = ProxyGroupKind::UrlTest;
        pinned.fixed = Some("node-a".into());
        pinned.capabilities = ProxyGroupCapabilities {
            select: true,
            clear_fixed: true,
        };
        let tray = TrayGroup::of(&pinned);
        assert_eq!(tray.fixed.as_deref(), Some("node-a"));
        assert_eq!(tray.now.as_deref(), Some("node-b"));
        assert!(tray.clear_fixed);

        // Clash-rs reports no pin: nothing to clear.
        let automatic = group("Fallback", &["node-a"], "node-a");
        let tray = TrayGroup::of(&automatic);
        assert_eq!(tray.fixed, None);
        assert!(!tray.clear_fixed);
    }

    /// The pin is part of the item text and the restore item exists only
    /// for clearable groups, so either change needs a rebuild.
    #[test]
    fn a_pin_change_needs_a_rebuild() {
        let open = selecting(Some("a"));
        let mut pinned = selecting(Some("a"));
        pinned["Proxy"].fixed = Some("a".to_owned());
        assert_eq!(diff_proxies(&open, &pinned), TrayUpdateType::Full);
        let mut clearable = selecting(Some("a"));
        clearable["Proxy"].clear_fixed = true;
        assert_eq!(diff_proxies(&open, &clearable), TrayUpdateType::Full);
    }
}
