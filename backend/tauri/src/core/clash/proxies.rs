//! The proxy view the tray and the frontend share, assembled from one read
//! of the core's `/proxies`, `/providers/proxies` and group list.
use anyhow::Result;
use clash_api::{IndexMap, ProviderName, Proxy, ProxyName, ProxyProvider, VehicleType};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A group's `type`, as the core reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum ProxyGroupKind {
    Selector,
    #[serde(rename = "URLTest")]
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Smart,
    /// A type this build does not know, kept verbatim.
    #[serde(untagged)]
    Unknown(String),
}

impl ProxyGroupKind {
    fn parse(value: &str) -> Self {
        match value {
            "Selector" => Self::Selector,
            "URLTest" => Self::UrlTest,
            "Fallback" => Self::Fallback,
            "LoadBalance" => Self::LoadBalance,
            "Relay" => Self::Relay,
            "Smart" => Self::Smart,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

/// What the running core lets a user do with a group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupCapabilities {
    /// `PUT /proxies/{group}` chooses a member.
    pub select: bool,
    /// `DELETE /proxies/{group}` returns a pinned group to automatic selection.
    pub clear_fixed: bool,
}

impl ProxyGroupCapabilities {
    /// Mihomo and Meow report `fixed` (empty while unpinned) on exactly the
    /// URLTest and Fallback groups they let a user pin; Clash-rs omits it
    /// and rejects selecting those groups.
    fn infer(kind: &ProxyGroupKind, record: &Proxy) -> Self {
        match kind {
            ProxyGroupKind::Selector => Self {
                select: true,
                clear_fixed: false,
            },
            ProxyGroupKind::UrlTest | ProxyGroupKind::Fallback => {
                let pinnable = record.fixed.is_some();
                Self {
                    select: pinnable,
                    clear_fixed: pinnable,
                }
            }
            _ => Self::default(),
        }
    }
}

/// A group's meaning, derived from its record in `Proxies::nodes`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: ProxyName,
    #[serde(rename = "type")]
    pub kind: ProxyGroupKind,
    /// Member names; look each node up in `Proxies::nodes`.
    pub all: Vec<ProxyName>,
    pub now: Option<ProxyName>,
    /// The member a user pinned; `None` while the core selects on its own.
    pub fixed: Option<ProxyName>,
    /// The URL the core tests this group's members with; `None` when unset.
    pub test_url: Option<String>,
    /// The status codes a test must return, in the core's range syntax.
    pub expected_status: Option<String>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub capabilities: ProxyGroupCapabilities,
}

impl ProxyGroup {
    fn from_record(record: &Proxy) -> Self {
        let kind = ProxyGroupKind::parse(&record.proxy_type);
        let capabilities = ProxyGroupCapabilities::infer(&kind, record);
        Self {
            name: record.name.clone(),
            kind,
            all: record.all.clone().unwrap_or_default(),
            now: record.now.clone(),
            fixed: record
                .fixed
                .clone()
                .filter(|name| !name.as_str().is_empty()),
            test_url: record.test_url.clone().filter(|url| !url.is_empty()),
            expected_status: record
                .expected_status
                .clone()
                .filter(|status| !status.is_empty()),
            hidden: record.hidden.unwrap_or(false),
            icon: record.icon.clone(),
            capabilities,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    /// The core's GLOBAL group; `None` when the core has none.
    pub global: Option<ProxyGroup>,
    pub groups: Vec<ProxyGroup>,
    /// Every `/proxies` entry plus every provider-owned node referenced by a
    /// group, keyed by name. A node that belongs to several groups still has
    /// exactly one entry here; groups reference it by name in `all`.
    pub nodes: IndexMap<ProxyName, Proxy>,
}

/// `/proxies` names a node's provider in `provider` or `provider-name`.
fn with_provider(mut proxy: Proxy) -> Proxy {
    proxy.provider = proxy
        .provider
        .take()
        .filter(|name| !name.is_empty())
        .or_else(|| proxy.provider_name.clone().filter(|name| !name.is_empty()));
    proxy
}

/// Every proxy of an HTTP, File, or Inline provider by name, with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies. The index borrows, so
/// only the nodes a group references are ever copied.
fn provider_proxy_index(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<&ProxyName, (&ProviderName, &Proxy)> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            proxies.insert(&proxy.name, (provider, proxy));
        }
    }
    proxies
}

/// A group member found neither in /proxies nor in any provider.
fn unknown_proxy(name: &ProxyName) -> Proxy {
    Proxy {
        name: name.clone(),
        proxy_type: "Unknown".to_owned(),
        history: Vec::new(),
        extra: None,
        alive: None,
        udp: false,
        uot: None,
        xudp: None,
        tfo: None,
        mptcp: None,
        smux: None,
        interface: None,
        routing_mark: None,
        provider_name: None,
        dialer_proxy: None,
        id: None,
        now: None,
        all: None,
        test_url: None,
        expected_status: None,
        fixed: None,
        hidden: None,
        icon: None,
        empty_fallback: None,
        provider: None,
    }
}

impl Proxies {
    /// `group_list` is the core's own group list; `None` infers groups from
    /// every `/proxies` record with members.
    pub fn from_responses(
        proxies: IndexMap<ProxyName, Proxy>,
        providers: &IndexMap<ProviderName, ProxyProvider>,
        group_list: Option<IndexMap<ProxyName, Proxy>>,
    ) -> Result<Self> {
        let mut nodes: IndexMap<ProxyName, Proxy> = proxies
            .into_iter()
            .map(|(name, proxy)| (name, with_provider(proxy)))
            .collect();
        // A listed group replaces its /proxies record, so one group never
        // mixes the members of one read with the selection of the other.
        let mut group_records: IndexMap<ProxyName, Proxy> = match group_list {
            Some(groups) => {
                let groups: IndexMap<ProxyName, Proxy> = groups
                    .into_iter()
                    .map(|(name, group)| (name, with_provider(group)))
                    .collect();
                for (name, group) in &groups {
                    nodes.insert(name.clone(), group.clone());
                }
                groups
            }
            None => nodes
                .iter()
                .filter(|(_, proxy)| proxy.all.is_some())
                .map(|(name, proxy)| (name.clone(), proxy.clone()))
                .collect(),
        };
        let global = group_records.swap_remove(&ProxyName::from("GLOBAL"));

        for required in ["DIRECT", "REJECT"] {
            anyhow::ensure!(
                nodes.contains_key(&ProxyName::from(required)),
                "{required} is missing in /proxies"
            );
        }

        // GLOBAL only orders the groups it lists, never decides which
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
        remaining.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        ordered.extend(remaining);

        // Group members missing from /proxies (provider-owned nodes) are
        // added once; a node shared by several groups keeps a single entry.
        let provider_proxies = provider_proxy_index(providers);
        let mut convert = |record: Proxy| {
            for name in record.all.iter().flatten() {
                if !nodes.contains_key(name) {
                    let node = match provider_proxies.get(name) {
                        Some((provider, proxy)) => {
                            let mut node = Proxy::clone(proxy);
                            node.provider = Some(provider.as_str().to_owned());
                            node
                        }
                        None => unknown_proxy(name),
                    };
                    nodes.insert(name.clone(), node);
                }
            }
            ProxyGroup::from_record(&record)
        };
        let groups = ordered.into_iter().map(&mut convert).collect();
        let global = global.map(&mut convert);

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
    use serde_json::json;

    fn record(value: serde_json::Value) -> Proxy {
        serde_json::from_value(value).unwrap()
    }

    fn item(name: &str, kind: &str, all: Option<Vec<&str>>, now: Option<&str>) -> Proxy {
        record(json!({
            "name": name, "type": kind, "udp": false, "history": [], "all": all, "now": now
        }))
    }

    #[test]
    fn a_group_carries_its_test_url_and_expected_status() {
        let mut group = item("g", "URLTest", Some(vec!["DIRECT"]), Some("DIRECT"));
        group.test_url = Some("https://cp.cloudflare.com".into());
        group.expected_status = Some("204".into());
        let mut empty = item("e", "Selector", Some(vec!["DIRECT"]), Some("DIRECT"));
        empty.test_url = Some(String::new());
        empty.expected_status = Some(String::new());
        let proxies =
            Proxies::from_responses(records(&[group, empty]), &IndexMap::new(), None).unwrap();
        let by_name = |name: &str| {
            proxies
                .groups
                .iter()
                .find(|g| g.name.as_str() == name)
                .unwrap()
        };
        assert_eq!(
            by_name("g").test_url.as_deref(),
            Some("https://cp.cloudflare.com")
        );
        assert_eq!(by_name("g").expected_status.as_deref(), Some("204"));
        assert_eq!(by_name("e").test_url, None);
        assert_eq!(by_name("e").expected_status, None);
    }

    fn name(value: &str) -> ProxyName {
        ProxyName::from(value)
    }

    fn provider(key: &str, vehicle: &str, proxies: Vec<Proxy>) -> (ProviderName, ProxyProvider) {
        let provider = serde_json::from_value(json!({
            "name": key, "type": "Proxy", "vehicleType": vehicle, "proxies": proxies
        }))
        .unwrap();
        (ProviderName::from(key), provider)
    }

    fn records(groups: &[Proxy]) -> IndexMap<ProxyName, Proxy> {
        let mut proxies = IndexMap::from([
            (name("DIRECT"), item("DIRECT", "Direct", None, None)),
            (name("REJECT"), item("REJECT", "Reject", None, None)),
        ]);
        proxies.extend(
            groups
                .iter()
                .map(|group| (group.name.clone(), group.clone())),
        );
        proxies
    }

    fn assemble(raw: &[Proxy], listed: Option<&[Proxy]>) -> Proxies {
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

    /// A group record with `fields` merged over a Selector with one member.
    fn group_with(fields: serde_json::Value) -> ProxyGroup {
        let mut value = json!({
            "name": "G", "type": "Selector", "udp": false, "history": [], "all": ["a"]
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        ProxyGroup::from_record(&record(value))
    }

    /// A node listed by several providers takes the last one, a provider
    /// with an unsupported vehicle contributes nothing, and only members a
    /// group references are added.
    #[test]
    fn a_node_in_several_providers_takes_the_last_provider() {
        let providers = IndexMap::from([
            provider("first", "HTTP", vec![item("shared", "Vmess", None, None)]),
            provider("second", "File", vec![item("shared", "Trojan", None, None)]),
            provider(
                "compat",
                "Compatible",
                vec![item("compat-only", "Vless", None, None)],
            ),
            provider(
                "extra",
                "Inline",
                vec![item("unreferenced", "Vless", None, None)],
            ),
        ]);
        let group = item("G", "Selector", Some(vec!["shared", "compat-only"]), None);
        let result = Proxies::from_responses(records(&[group]), &providers, None).unwrap();

        let shared = &result.nodes[&name("shared")];
        assert_eq!(shared.proxy_type, "Trojan");
        assert_eq!(shared.provider.as_deref(), Some("second"));
        assert_eq!(result.nodes[&name("compat-only")].proxy_type, "Unknown");
        assert!(!result.nodes.contains_key(&name("unreferenced")));
    }

    #[test]
    fn assembles_provider_owned_nodes_with_metadata_for_supported_vehicles() {
        for vehicle_type in ["HTTP", "File", "Inline"] {
            for proxy_type in ["Vless", "Trojan", "Hysteria2"] {
                let node = record(json!({
                    "name": "provider-node", "type": proxy_type, "udp": true,
                    "history": [{"time": "2026-10-04T08:00:00Z", "delay": 42}],
                    "alive": true, "xudp": true, "tfo": true,
                    "testUrl": "https://example.com/204", "dialer-proxy": "relay"
                }));
                let group = item(
                    "PROXY",
                    "Selector",
                    Some(vec!["provider-node"]),
                    Some("provider-node"),
                );
                let providers =
                    IndexMap::from([provider("provider", vehicle_type, vec![node.clone()])]);

                let proxies = Proxies::from_responses(records(&[group]), &providers, None).unwrap();
                let mut expected = node;
                expected.provider = Some("provider".into());

                assert_eq!(proxies.groups[0].all, [name("provider-node")]);
                assert_eq!(
                    proxies.nodes[&name("provider-node")],
                    expected,
                    "metadata must be preserved for {vehicle_type} / {proxy_type}"
                );
            }
        }
    }

    #[test]
    fn compatible_providers_do_not_supply_members() {
        let group = item("PROXY", "Selector", Some(vec!["grouped"]), None);
        let providers = IndexMap::from([provider(
            "PROXY",
            "Compatible",
            vec![item("grouped", "Vless", None, None)],
        )]);
        let proxies = Proxies::from_responses(records(&[group]), &providers, None).unwrap();
        assert_eq!(proxies.nodes[&name("grouped")].proxy_type, "Unknown");
    }

    /// A node shared by two groups is stored once in `nodes`; group `all`
    /// keeps member names in order; a member absent from `/proxies` and from
    /// any provider falls back to the Unknown placeholder.
    #[test]
    fn shares_one_node_record_across_groups_and_preserves_member_order() {
        let raw = [
            item(
                "GLOBAL",
                "Selector",
                Some(vec!["GroupA", "GroupB"]),
                Some("GroupA"),
            ),
            item(
                "GroupA",
                "Selector",
                Some(vec!["shared-node", "provider-only", "a-only"]),
                Some("shared-node"),
            ),
            item(
                "GroupB",
                "Selector",
                Some(vec!["shared-node", "provider-only", "totally-unknown"]),
                Some("shared-node"),
            ),
            item("shared-node", "VmessSharedMarker", None, None),
            item("a-only", "Vmess", None, None),
        ];
        let providers = IndexMap::from([provider(
            "sub",
            "Inline",
            vec![
                item("provider-only", "Trojan", None, None),
                item("shared-node", "ProviderIgnored", None, None),
            ],
        )]);

        let proxies = Proxies::from_responses(records(&raw), &providers, None).unwrap();

        let members = |group: &str| {
            proxies
                .groups
                .iter()
                .find(|g| g.name.as_str() == group)
                .unwrap()
                .all
                .clone()
        };
        assert_eq!(
            members("GroupA"),
            [name("shared-node"), name("provider-only"), name("a-only")]
        );
        assert_eq!(
            members("GroupB"),
            [
                name("shared-node"),
                name("provider-only"),
                name("totally-unknown")
            ]
        );

        // Serialized once: the marker only lives on the full node record.
        let serialized = serde_json::to_string(&proxies).unwrap();
        assert_eq!(serialized.matches("VmessSharedMarker").count(), 1);
        assert_eq!(serialized.matches("Trojan").count(), 1);
        assert_eq!(
            proxies.nodes[&name("shared-node")].proxy_type,
            "VmessSharedMarker"
        );
        assert!(proxies.nodes[&name("shared-node")].provider.is_none());

        let provider_node = &proxies.nodes[&name("provider-only")];
        assert_eq!(provider_node.proxy_type, "Trojan");
        assert_eq!(provider_node.provider.as_deref(), Some("sub"));

        let unknown_node = &proxies.nodes[&name("totally-unknown")];
        assert_eq!(unknown_node.proxy_type, "Unknown");
        assert!(unknown_node.history.is_empty());
    }

    #[test]
    fn listed_groups_decide_membership_and_replace_proxy_records() {
        let old = item("Foo", "Selector", Some(vec!["DIRECT"]), Some("DIRECT"));
        let listed = item("Foo", "LoadBalance", Some(vec!["Bar", "missing"]), None);
        let bar = item("Bar", "FutureGroup", Some(vec![]), None);
        let extra = item("extra", "Selector", Some(vec![]), None);
        let result = assemble(&[old, extra], Some(&[listed, bar]));
        assert_eq!(names(&result), ["Bar", "Foo"]);
        assert_eq!(result.nodes[&name("Foo")].proxy_type, "LoadBalance");
        assert_eq!(result.nodes[&name("missing")].proxy_type, "Unknown");
        assert!(result.nodes.contains_key(&name("extra")));
    }

    #[test]
    fn an_empty_group_list_is_not_replaced_by_inference() {
        let raw = [
            item("GLOBAL", "Selector", Some(vec!["Foo"]), None),
            item("Foo", "Selector", Some(vec![]), None),
        ];
        let result = assemble(&raw, Some(&[]));
        assert!(result.groups.is_empty());
        assert!(result.global.is_none());
        assert!(result.nodes.contains_key(&name("Foo")));
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
                    assert_eq!(result.global.is_none(), !has_global);
                    assert_eq!(
                        result.nodes[&name("Foo")].all.as_ref().unwrap(),
                        &[name("Bar"), name("Foo")]
                    );
                }
            }
        }
    }

    #[test]
    fn capabilities_follow_the_type_and_the_fixed_field() {
        let cases = [
            ("Selector", None, (true, false)),
            ("Selector", Some(""), (true, false)),
            ("Selector", Some("a"), (true, false)),
            ("URLTest", None, (false, false)),
            ("URLTest", Some(""), (true, true)),
            ("URLTest", Some("a"), (true, true)),
            ("Fallback", None, (false, false)),
            ("Fallback", Some(""), (true, true)),
            ("Fallback", Some("a"), (true, true)),
            ("LoadBalance", None, (false, false)),
            ("Relay", None, (false, false)),
            ("Smart", None, (false, false)),
            ("Weighted", Some(""), (false, false)),
        ];
        for (kind, fixed, (select, clear_fixed)) in cases {
            let mut fields = json!({ "type": kind });
            if let Some(fixed) = fixed {
                fields["fixed"] = json!(fixed);
            }
            assert_eq!(
                group_with(fields).capabilities,
                ProxyGroupCapabilities {
                    select,
                    clear_fixed
                },
                "{kind} with fixed {fixed:?}"
            );
        }
    }

    #[test]
    fn group_fields_are_normalized_from_the_record() {
        let unpinned = group_with(json!({"type": "URLTest", "fixed": "", "now": "a"}));
        assert_eq!(unpinned.kind, ProxyGroupKind::UrlTest);
        assert_eq!(unpinned.fixed, None);
        assert!(!unpinned.hidden);

        // URLTest keeps a pin while it routes through another member.
        let pinned = group_with(json!({
            "type": "URLTest", "fixed": "a", "now": "b", "hidden": true, "icon": "i"
        }));
        assert_eq!(pinned.fixed, Some(name("a")));
        assert_eq!(pinned.now, Some(name("b")));
        assert!(pinned.hidden);
        assert_eq!(pinned.icon.as_deref(), Some("i"));

        let memberless = group_with(json!({"type": "LoadBalance", "all": null}));
        assert!(memberless.all.is_empty());
        assert_eq!(memberless.now, None);
    }

    #[test]
    fn the_group_type_serializes_as_the_core_reports_it() {
        for (raw, kind) in [
            ("Selector", ProxyGroupKind::Selector),
            ("URLTest", ProxyGroupKind::UrlTest),
            ("Fallback", ProxyGroupKind::Fallback),
            ("LoadBalance", ProxyGroupKind::LoadBalance),
            ("Relay", ProxyGroupKind::Relay),
            ("Smart", ProxyGroupKind::Smart),
            ("Weighted", ProxyGroupKind::Unknown("Weighted".into())),
        ] {
            assert_eq!(ProxyGroupKind::parse(raw), kind);
            assert_eq!(serde_json::to_value(&kind).unwrap(), json!(raw));
        }
    }

    #[test]
    fn node_provider_prefers_provider_then_provider_name() {
        let cases = [
            (json!({"provider": "p", "provider-name": "q"}), Some("p")),
            (json!({"provider": "", "provider-name": "q"}), Some("q")),
            (json!({"provider-name": ""}), None),
            (json!({}), None),
        ];
        for (fields, expected) in cases {
            let mut value = json!({"name": "n", "type": "Vless", "udp": false, "history": []});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let result = assemble(&[record(value)], None);
            assert_eq!(result.nodes[&name("n")].provider.as_deref(), expected);
        }
    }
}
