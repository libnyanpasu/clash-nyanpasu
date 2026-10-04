/// This module is used to manage the proxies for the Tauri application.
/// It is used to provide the unite interface between tray and frontend.
/// TODO: add a diff algorithm to reduce the data transfer, and the rerendering of the tray menu.
use super::api;
use anyhow::Result;
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

fn provider_proxy_map(
    providers: &IndexMap<String, api::ProxyProviderItem>,
) -> IndexMap<String, api::ProxyItem> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        for proxy in &record.proxies {
            let mut proxy = proxy.clone();
            proxy.provider = Some(provider.clone());
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
    pub fn from_responses(
        inner_proxies: api::ProxiesRes,
        providers_proxies: api::ProvidersProxiesRes,
    ) -> Result<Self> {
        let inner_proxies = inner_proxies.proxies;
        // 1. Include nodes from HTTP, File, and Inline providers.
        let providers_proxies: IndexMap<String, api::ProxyProviderItem> = {
            let records = providers_proxies.providers;
            records
                .into_iter()
                .filter(|(_k, v)| {
                    matches!(
                        v.vehicle_type,
                        api::VehicleType::Http | api::VehicleType::File | api::VehicleType::Inline
                    )
                })
                .collect()
        };

        // 2. Map every provider-owned proxy by name. Mihomo 1.19.28 no longer
        // includes these nodes in /proxies, so their metadata must come from
        // /providers/proxies.
        let provider_map = provider_proxy_map(&providers_proxies);
        let generate_item = |name: &str| resolve_proxy(name, &inner_proxies, &provider_map);

        let global = inner_proxies.get("GLOBAL");
        inner_proxies
            .get("DIRECT")
            .ok_or(anyhow::anyhow!("DIRECT is missing in /proxies"))?; // It should be always exists
        inner_proxies
            .get("REJECT")
            .ok_or(anyhow::anyhow!("REJECT is missing in /proxies"))?; // It should be always exists

        // 3. every /proxies entry is a node; group members not already
        // covered by it (provider-owned nodes) are resolved and added once
        // (a node shared by several groups still has a single entry).
        let mut nodes = inner_proxies.clone();
        let collect_members = |names: &[String], nodes: &mut IndexMap<String, api::ProxyItem>| {
            for name in names {
                nodes
                    .entry(name.clone())
                    .or_insert_with(|| generate_item(name));
            }
        };

        // 4. generate the proxies groups
        let groups: Vec<ProxyGroupItem> = match global {
            Some(api::ProxyItem { all: Some(all), .. }) => {
                let all = all.clone();
                all.into_iter()
                    .filter(|name| {
                        matches!(
                            inner_proxies.get(name),
                            Some(api::ProxyItem { all: Some(_), .. })
                        )
                    })
                    .map(|name| {
                        let item = inner_proxies
                            .get(&name)
                            .unwrap_or(&api::ProxyItem::default())
                            .clone();
                        let item_all = item.all.clone().unwrap_or_default();
                        collect_members(&item_all, &mut nodes);
                        let mut item: ProxyGroupItem = item.into();
                        item.all = item_all;
                        item
                    })
                    .collect()
            }
            _ => {
                let mut groups: Vec<ProxyGroupItem> = inner_proxies
                    .clone()
                    .into_values()
                    .filter(|v| v.name == "GLOBAL" && v.all.is_some())
                    .map(|v| {
                        let all = v.all.clone().unwrap_or_default();
                        collect_members(&all, &mut nodes);
                        let mut item: ProxyGroupItem = v.clone().into();
                        item.all = all;
                        item
                    })
                    .collect();
                groups.sort_by_key(|a| std::cmp::Reverse(a.name.to_lowercase()));
                groups
            }
        };

        // 5. generate the global
        let global: Option<ProxyGroupItem> = global.map(|v| {
            let all = v.all.clone().unwrap_or_default();
            collect_members(&all, &mut nodes);
            let mut item: ProxyGroupItem = v.clone().into();
            item.all = all;
            item
        });

        Ok(Proxies {
            global: global.unwrap_or_default(),
            groups,
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_provider_owned_proxy_with_metadata() {
        let node = api::ProxyItem {
            name: "provider-node".into(),
            r#type: "Vless".into(),
            udp: true,
            ..Default::default()
        };
        let providers = IndexMap::from([(
            "subscription".into(),
            api::ProxyProviderItem {
                name: "subscription".into(),
                r#type: api::ProviderType::Proxy,
                proxies: vec![node],
                vehicle_type: api::VehicleType::Http,
                updated_at: None,
                subscription_info: None,
                test_url: None,
                expected_status: None,
            },
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
        for vehicle_type in [
            api::VehicleType::Http,
            api::VehicleType::File,
            api::VehicleType::Inline,
        ] {
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
                let mut node = api::ProxyItem {
                    name: "provider-node".into(),
                    r#type: proxy_type.into(),
                    udp: true,
                    history: vec![api::ProxyItemHistory {
                        time: "2026-10-04T08:00:00Z".into(),
                        delay: 42,
                    }],
                    alive: Some(true),
                    xudp: Some(true),
                    tfo: Some(true),
                    ..Default::default()
                };
                let providers_proxies = api::ProvidersProxiesRes {
                    providers: IndexMap::from([(
                        "provider".into(),
                        api::ProxyProviderItem {
                            name: "provider".into(),
                            r#type: api::ProviderType::Proxy,
                            proxies: vec![node.clone()],
                            vehicle_type: vehicle_type.clone(),
                            updated_at: None,
                            subscription_info: None,
                            test_url: None,
                            expected_status: None,
                        },
                    )]),
                };

                let proxies = Proxies::from_responses(inner_proxies, providers_proxies).unwrap();
                node.provider = Some("provider".into());

                assert_eq!(proxies.groups[0].all, vec!["provider-node"]);
                assert_eq!(
                    serde_json::to_value(&proxies.nodes["provider-node"]).unwrap(),
                    serde_json::to_value(&node).unwrap(),
                    "metadata must be preserved for {vehicle_type:?} / {proxy_type}"
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
        let providers_proxies = api::ProvidersProxiesRes {
            providers: IndexMap::from([(
                "sub".to_string(),
                api::ProxyProviderItem {
                    name: "sub".into(),
                    r#type: api::ProviderType::Proxy,
                    proxies: vec![
                        item("provider-only", "Trojan", None, None),
                        item("shared-node", "ProviderIgnored", None, None),
                    ],
                    vehicle_type: api::VehicleType::Inline,
                    updated_at: None,
                    subscription_info: None,
                    test_url: None,
                    expected_status: None,
                },
            )]),
        };

        let proxies = Proxies::from_responses(inner_proxies, providers_proxies).unwrap();

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
}
