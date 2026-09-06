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
    pub all: Vec<api::ProxyItem>,
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
    pub direct: api::ProxyItem,
    pub groups: Vec<ProxyGroupItem>,
    pub records: IndexMap<String, api::ProxyItem>,
    pub proxies: Vec<api::ProxyItem>,
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
        // 1. filter out the Http or File type provider proxies
        let providers_proxies: IndexMap<String, api::ProxyProviderItem> = {
            let records = providers_proxies.providers;
            records
                .into_iter()
                .filter(|(_k, v)| {
                    matches!(
                        v.vehicle_type,
                        api::VehicleType::Http | api::VehicleType::File
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
        let direct = inner_proxies
            .get("DIRECT")
            .ok_or(anyhow::anyhow!("DIRECT is missing in /proxies"))?
            .clone(); // It should be always exists
        let reject = inner_proxies
            .get("REJECT")
            .ok_or(anyhow::anyhow!("REJECT is missing in /proxies"))?
            .clone(); // It should be always exists

        // 3. generate the proxies groups
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
                        let mut item: ProxyGroupItem = item.into();
                        item.all = item_all
                            .into_iter()
                            .map(|name| generate_item(&name))
                            .collect();
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
                        let mut item: ProxyGroupItem = v.clone().into();
                        item.all = all.into_iter().map(|name| generate_item(&name)).collect();
                        item
                    })
                    .collect();
                groups.sort_by_key(|a| std::cmp::Reverse(a.name.to_lowercase()));
                groups
            }
        };

        // 4. generate the proxies
        let mut proxies: Vec<api::ProxyItem> = vec![direct.clone(), reject];
        proxies.extend(inner_proxies.clone().into_values().filter(|v| {
            matches!(v.name.as_str(), "DIRECT" | "REJECT")
                && (v.all.is_none() || v.all.as_ref().unwrap().is_empty())
        }));

        // 5. generate the global
        let global: Option<ProxyGroupItem> = global.map(|v| {
            let all = v.all.clone().unwrap_or_default();
            let mut item: ProxyGroupItem = v.clone().into();
            item.all = all.into_iter().map(|name| generate_item(&name)).collect();
            item
        });

        Ok(Proxies {
            global: global.unwrap_or_default(),
            direct,
            groups,
            records: inner_proxies,
            proxies,
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
}
