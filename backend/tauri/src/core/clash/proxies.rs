/// This module is used to manage the proxies for the Tauri application.
/// It is used to provide the unite interface between tray and frontend.
/// TODO: add a diff algorithm to reduce the data transfer, and the rerendering of the tray menu.
use super::api;
use anyhow::Result;
use clash_api::{ProviderName, ProxyProvider, VehicleType};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupItem {
    pub name: String,
    pub r#type: String, // TODO: 考虑改成枚举
    pub udp: bool,
    pub history: Vec<api::ProxyItemHistory>,
    pub all: Vec<String>, // member names; look up the node in `Proxies::nodes`
    pub now: Option<String>, // 当前选中的代理
    pub provider: Option<String>,
    pub alive: Option<bool>, // Mihomo Or Premium Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xudp: Option<bool>, // Mihomo Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tfo: Option<bool>, // Mihomo Only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
                             // extra: {}, // Mihomo Only
}

impl From<api::ProxyItem> for ProxyGroupItem {
    fn from(item: api::ProxyItem) -> Self {
        let all = vec![];
        ProxyGroupItem {
            name: item.name,
            r#type: item.r#type,
            udp: item.udp,
            history: item.history,
            all,
            now: item.now,
            provider: item.provider,
            alive: item.alive,
            xudp: item.xudp,
            tfo: item.tfo,
            icon: item.icon,
            hidden: item.hidden,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    pub global: ProxyGroupItem,
    pub groups: Vec<ProxyGroupItem>,
    /// Every `/proxies` entry plus every provider-owned node referenced by a
    /// group, keyed by name. A node that belongs to several groups still has
    /// exactly one entry here; groups reference it by name in `all`.
    pub nodes: IndexMap<String, api::ProxyItem>,
}

pub(crate) fn proxy_items(
    proxies: clash_api::IndexMap<clash_api::ProxyName, clash_api::Proxy>,
) -> clash_api::IndexMap<String, api::ProxyItem> {
    proxies
        .into_iter()
        .map(|(name, proxy)| (name.as_str().to_owned(), proxy_item(proxy)))
        .collect()
}

pub(crate) fn proxy_item(proxy: clash_api::Proxy) -> api::ProxyItem {
    api::ProxyItem {
        name: proxy.name.as_str().to_owned(),
        r#type: proxy.proxy_type,
        udp: proxy.udp,
        history: proxy
            .history
            .into_iter()
            .map(|item| api::ProxyItemHistory {
                time: item.time.to_rfc3339(),
                delay: item.delay,
            })
            .collect(),
        all: proxy.all.map(|items| {
            items
                .into_iter()
                .map(|name| name.as_str().to_owned())
                .collect()
        }),
        now: proxy.now.map(|name| name.as_str().to_owned()),
        provider: proxy
            .provider
            .filter(|name| !name.is_empty())
            .or_else(|| proxy.provider_name.filter(|name| !name.is_empty())),
        alive: proxy.alive,
        xudp: proxy.xudp,
        tfo: proxy.tfo,
        icon: proxy.icon,
        hidden: proxy.hidden.unwrap_or(false),
    }
}

/// Every proxy of an HTTP, File, or Inline provider by name, tagged with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies.
fn provider_proxy_map(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<String, api::ProxyItem> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            let mut proxy = proxy_item(proxy.clone());
            proxy.provider = Some(provider.as_str().to_owned());
            proxies.insert(proxy.name.clone(), proxy);
        }
    }
    proxies
}

fn resolve_proxy(
    name: &str,
    inner_proxies: &IndexMap<String, api::ProxyItem>,
    provider_proxies: &IndexMap<String, api::ProxyItem>,
) -> api::ProxyItem {
    inner_proxies
        .get(name)
        .or_else(|| provider_proxies.get(name))
        .cloned()
        .unwrap_or_else(|| api::ProxyItem {
            name: name.to_string(),
            r#type: "Unknown".to_string(),
            udp: false,
            history: vec![],
            ..Default::default()
        })
}

impl Proxies {
    /// `group_list` is the core's own group list; `None` infers groups from
    /// every `/proxies` record with members.
    pub fn from_responses(
        inner_proxies: api::ProxiesRes,
        providers: &IndexMap<ProviderName, ProxyProvider>,
        group_list: Option<IndexMap<String, api::ProxyItem>>,
    ) -> Result<Self> {
        let mut inner_proxies = inner_proxies.proxies;
        // A listed group replaces its /proxies record, so one group never
        // mixes the members of one read with the selection of the other.
        let mut group_records = match group_list {
            Some(groups) => {
                for (name, group) in &groups {
                    inner_proxies.insert(name.clone(), group.clone());
                }
                groups
            }
            None => inner_proxies
                .iter()
                .filter(|(_, proxy)| proxy.all.is_some())
                .map(|(name, proxy)| (name.clone(), proxy.clone()))
                .collect(),
        };
        let global = group_records.swap_remove("GLOBAL");
        // 1. Map every provider-owned proxy by name.
        let provider_map = provider_proxy_map(providers);
        let generate_item = |name: &str| resolve_proxy(name, &inner_proxies, &provider_map);

        inner_proxies
            .get("DIRECT")
            .ok_or(anyhow::anyhow!("DIRECT is missing in /proxies"))?; // It should be always exists
        inner_proxies
            .get("REJECT")
            .ok_or(anyhow::anyhow!("REJECT is missing in /proxies"))?; // It should be always exists

        // 2. GLOBAL only orders the groups it lists, never decides which
        // groups exist; the rest follow by name.
        let mut ordered = Vec::with_capacity(group_records.len());
        if let Some(names) = global.as_ref().and_then(|group| group.all.as_ref()) {
            for name in names {
                if let Some(group) = group_records.swap_remove(name) {
                    ordered.push(group);
                }
            }
        }
        let mut remaining: Vec<_> = group_records.into_values().collect();
        remaining.sort_by(|a, b| a.name.cmp(&b.name));
        ordered.extend(remaining);

        // 3. every /proxies entry is a node; group members not already
        // covered by it (provider-owned nodes) are resolved and added once
        // (a node shared by several groups still has a single entry).
        let mut nodes = inner_proxies.clone();
        let mut convert = |record: api::ProxyItem| {
            let all = record.all.clone().unwrap_or_default();
            for name in &all {
                nodes
                    .entry(name.clone())
                    .or_insert_with(|| generate_item(name));
            }
            let mut item: ProxyGroupItem = record.into();
            item.all = all;
            item
        };
        let groups = ordered.into_iter().map(&mut convert).collect();
        let global = global.map(&mut convert).unwrap_or_default();

        Ok(Proxies {
            global,
            groups,
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clash_api::{ProviderName, ProxyProvider};
    use serde_json::json;

    fn provider(
        key: &str,
        vehicle: &str,
        proxies: serde_json::Value,
    ) -> (ProviderName, ProxyProvider) {
        let provider = serde_json::from_value(json!({
            "name": key, "type": "Proxy", "vehicleType": vehicle, "proxies": proxies
        }))
        .unwrap();
        (ProviderName::from(key), provider)
    }

    #[test]
    fn resolves_provider_owned_proxy_with_metadata() {
        let providers = IndexMap::from([provider(
            "subscription",
            "HTTP",
            json!([{"name": "provider-node", "type": "Vless", "udp": true, "history": []}]),
        )]);

        let provider_proxies = provider_proxy_map(&providers);
        let resolved = resolve_proxy("provider-node", &IndexMap::new(), &provider_proxies);

        assert_eq!(resolved.r#type, "Vless");
        assert!(resolved.udp);
        assert_eq!(resolved.provider.as_deref(), Some("subscription"));
    }

    fn item(name: &str, r#type: &str, all: Option<Vec<&str>>, now: Option<&str>) -> api::ProxyItem {
        api::ProxyItem {
            name: name.to_string(),
            r#type: r#type.to_string(),
            all: all.map(|names| names.into_iter().map(String::from).collect()),
            now: now.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn assembles_provider_owned_nodes_with_metadata_for_supported_vehicles() {
        for vehicle_type in ["HTTP", "File", "Inline"] {
            for proxy_type in ["Vless", "Trojan", "Hysteria2"] {
                let inner_proxies = api::ProxiesRes {
                    proxies: IndexMap::from([
                        (
                            "GLOBAL".into(),
                            item("GLOBAL", "Selector", Some(vec!["PROXY"]), Some("PROXY")),
                        ),
                        ("DIRECT".into(), item("DIRECT", "Direct", None, None)),
                        ("REJECT".into(), item("REJECT", "Reject", None, None)),
                        (
                            "PROXY".into(),
                            item(
                                "PROXY",
                                "Selector",
                                Some(vec!["provider-node"]),
                                Some("provider-node"),
                            ),
                        ),
                    ]),
                };
                let node: clash_api::Proxy = serde_json::from_value(json!({
                    "name": "provider-node", "type": proxy_type, "udp": true,
                    "history": [{"time": "2026-10-04T08:00:00Z", "delay": 42}],
                    "alive": true, "xudp": true, "tfo": true
                }))
                .unwrap();
                let providers =
                    IndexMap::from([provider("provider", vehicle_type, json!([node.clone()]))]);

                let proxies = Proxies::from_responses(inner_proxies, &providers, None).unwrap();
                let mut expected = proxy_item(node);
                expected.provider = Some("provider".into());

                assert_eq!(proxies.groups[0].all, vec!["provider-node"]);
                assert_eq!(
                    serde_json::to_value(&proxies.nodes["provider-node"]).unwrap(),
                    serde_json::to_value(&expected).unwrap(),
                    "metadata must be preserved for {vehicle_type} / {proxy_type}"
                );
            }
        }
    }

    /// A node shared by two groups is stored once in `nodes` (G7); group
    /// `all` keeps member names in order; a member absent from `/proxies`
    /// and from any provider falls back to the Unknown placeholder.
    #[test]
    fn shares_one_node_record_across_groups_and_preserves_member_order() {
        let inner_proxies = api::ProxiesRes {
            proxies: IndexMap::from([
                (
                    "GLOBAL".to_string(),
                    item(
                        "GLOBAL",
                        "Selector",
                        Some(vec!["GroupA", "GroupB"]),
                        Some("GroupA"),
                    ),
                ),
                ("DIRECT".to_string(), item("DIRECT", "Direct", None, None)),
                ("REJECT".to_string(), item("REJECT", "Reject", None, None)),
                (
                    "GroupA".to_string(),
                    item(
                        "GroupA",
                        "Selector",
                        Some(vec!["shared-node", "provider-only", "a-only"]),
                        Some("shared-node"),
                    ),
                ),
                (
                    "GroupB".to_string(),
                    item(
                        "GroupB",
                        "Selector",
                        Some(vec!["shared-node", "provider-only", "totally-unknown"]),
                        Some("shared-node"),
                    ),
                ),
                (
                    "shared-node".to_string(),
                    item("shared-node", "VmessSharedMarker", None, None),
                ),
                ("a-only".to_string(), item("a-only", "Vmess", None, None)),
            ]),
        };
        let providers = IndexMap::from([provider(
            "sub",
            "Inline",
            json!([
                {"name": "provider-only", "type": "Trojan", "udp": false, "history": []},
                {"name": "shared-node", "type": "ProviderIgnored", "udp": false, "history": []}
            ]),
        )]);

        let proxies = Proxies::from_responses(inner_proxies, &providers, None).unwrap();

        let group_a = proxies.groups.iter().find(|g| g.name == "GroupA").unwrap();
        let group_b = proxies.groups.iter().find(|g| g.name == "GroupB").unwrap();
        assert_eq!(group_a.all, vec!["shared-node", "provider-only", "a-only"]);
        assert_eq!(
            group_b.all,
            vec!["shared-node", "provider-only", "totally-unknown"]
        );

        // Serialized once: the marker only lives on the full node record, so
        // it must appear exactly once even though "shared-node" is a member
        // of two groups.
        let serialized = serde_json::to_string(&proxies).unwrap();
        assert_eq!(serialized.matches("VmessSharedMarker").count(), 1);
        assert_eq!(serialized.matches("Trojan").count(), 1);
        assert_eq!(proxies.nodes["shared-node"].r#type, "VmessSharedMarker");
        assert!(proxies.nodes["shared-node"].provider.is_none());

        let provider_node = &proxies.nodes["provider-only"];
        assert_eq!(provider_node.r#type, "Trojan");
        assert_eq!(provider_node.provider.as_deref(), Some("sub"));

        let unknown_node = &proxies.nodes["totally-unknown"];
        assert_eq!(unknown_node.r#type, "Unknown");
        assert!(unknown_node.history.is_empty());
    }

    fn records(groups: &[api::ProxyItem]) -> api::ProxiesRes {
        let mut proxies = IndexMap::from([
            ("DIRECT".into(), item("DIRECT", "Direct", None, None)),
            ("REJECT".into(), item("REJECT", "Reject", None, None)),
        ]);
        proxies.extend(
            groups
                .iter()
                .map(|group| (group.name.clone(), group.clone())),
        );
        api::ProxiesRes { proxies }
    }

    fn assemble(raw: &[api::ProxyItem], listed: Option<&[api::ProxyItem]>) -> Proxies {
        let listed = listed.map(|groups| {
            groups
                .iter()
                .map(|group| (group.name.clone(), group.clone()))
                .collect()
        });
        Proxies::from_responses(records(raw), &IndexMap::new(), listed).unwrap()
    }

    fn names(proxies: &Proxies) -> Vec<&str> {
        proxies.groups.iter().map(|g| g.name.as_str()).collect()
    }

    #[test]
    fn listed_groups_decide_membership_and_replace_proxy_records() {
        let old = item("Foo", "Selector", Some(vec!["DIRECT"]), Some("DIRECT"));
        let listed = item("Foo", "LoadBalance", Some(vec!["Bar", "missing"]), None);
        let bar = item("Bar", "FutureGroup", Some(vec![]), None);
        let extra = item("extra", "Selector", Some(vec![]), None);
        let result = assemble(&[old, extra], Some(&[listed, bar]));
        assert_eq!(names(&result), ["Bar", "Foo"]);
        assert_eq!(result.nodes["Foo"].r#type, "LoadBalance");
        assert_eq!(result.nodes["missing"].r#type, "Unknown");
        assert!(result.nodes.contains_key("extra"));
    }

    #[test]
    fn an_empty_group_list_is_not_replaced_by_inference() {
        let raw = [
            item("GLOBAL", "Selector", Some(vec!["Foo"]), None),
            item("Foo", "Selector", Some(vec![]), None),
        ];
        let result = assemble(&raw, Some(&[]));
        assert!(result.groups.is_empty());
        assert!(result.global.name.is_empty());
        assert!(result.nodes.contains_key("Foo"));
    }

    #[test]
    fn global_only_orders_groups_whether_listed_or_inferred() {
        let foo = item("Foo", "Selector", Some(vec!["Bar", "Foo"]), None);
        let bar = item("Bar", "UnknownGroup", Some(vec![]), None);
        let lower = item("global", "Fallback", Some(vec!["Foo"]), None);
        let global = item(
            "GLOBAL",
            "Selector",
            Some(vec!["Foo", "Foo", "DIRECT", "missing", "GLOBAL"]),
            None,
        );
        for listed in [false, true] {
            for has_global in [false, true] {
                let mut groups = vec![foo.clone(), bar.clone(), lower.clone()];
                if has_global {
                    groups.push(global.clone());
                }
                for reverse in [false, true] {
                    if reverse {
                        groups.reverse();
                    }
                    let result = assemble(&groups, listed.then_some(groups.as_slice()));
                    let expected = if has_global {
                        ["Foo", "Bar", "global"]
                    } else {
                        ["Bar", "Foo", "global"]
                    };
                    assert_eq!(names(&result), expected);
                    assert_eq!(result.global.name.is_empty(), !has_global);
                    assert_eq!(result.nodes["Foo"].all.as_ref().unwrap(), &["Bar", "Foo"]);
                }
            }
        }
    }
}
