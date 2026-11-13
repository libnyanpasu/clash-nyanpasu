//! The proxy view the tray and the frontend share, assembled from one read
//! of the core's `/proxies`, `/providers/proxies` and group list.
use std::collections::HashSet;

use anyhow::Result;
use clash_api::{
    IndexMap, ProviderName, ProviderType, Proxy, ProxyName, ProxyProvider, SubscriptionInfo,
    VehicleType,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Provider metadata the application keeps after assembling its node view.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProviderSummary {
    pub name: ProviderName,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub vehicle_type: VehicleType,
    pub updated_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub subscription_info: Option<SubscriptionInfo>,
    pub proxy_count: usize,
}

impl From<ProxyProvider> for ProxyProviderSummary {
    fn from(provider: ProxyProvider) -> Self {
        Self {
            name: provider.name,
            provider_type: provider.provider_type,
            vehicle_type: provider.vehicle_type,
            updated_at: provider.updated_at,
            subscription_info: provider.subscription_info,
            proxy_count: provider.proxies.len(),
        }
    }
}

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
    /// Which of a node's delay records this group's members are shown with: [`DEFAULT_TEST_URL_KEY`]
    /// when the group tests with the app's default URL, else the group's own name. A test URL is
    /// configuration that may hold a credential, so the key names the source and never the URL;
    /// the backend resolves the URL itself.
    pub delay_key: String,
    /// The status codes a test must return, in the core's range syntax.
    pub expected_status: Option<String>,
    pub hidden: bool,
    /// Whether the group has an icon. The icon is a profile-provided URL or path that may hold a
    /// credential, so only `get_proxy_group_icons` (`sensitive_read`) returns it.
    pub icon_configured: bool,
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
            delay_key: if record
                .test_url
                .as_deref()
                .is_some_and(|url| !url.is_empty())
            {
                record.name.as_str().to_owned()
            } else {
                DEFAULT_TEST_URL_KEY.to_owned()
            },
            expected_status: record
                .expected_status
                .clone()
                .filter(|status| !status.is_empty()),
            hidden: record.hidden.unwrap_or(false),
            icon_configured: record.icon.as_deref().is_some_and(|icon| !icon.is_empty()),
            capabilities,
        }
    }
}

// TODO(typed-proxy-nodes): Evaluate distinct typed group/node variants instead
// of the shared wire `Proxy` with both sets of optional fields. Preserve unknown
// core kinds and provider-only nodes without serde_json::Value, avoid duplicate
// group-member storage, and measure layout/heap costs before changing the RPC model.
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

impl Proxies {
    /// Drops every node's per-URL delay records except those in `keep`.
    pub fn retain_extra(&mut self, keep: &HashSet<&str>) {
        for node in self.nodes.values_mut() {
            if let Some(extra) = node.extra.as_mut() {
                extra.retain(|url, _| keep.contains(url.as_str()));
            }
        }
    }

    /// The test URL the core uses for `group`'s members, when it has one of its own.
    pub fn group_test_url(&self, group: &str) -> Option<&str> {
        self.nodes
            .get(&ProxyName::from(group))?
            .test_url
            .as_deref()
            .filter(|url| !url.is_empty())
    }

    /// The test URL of `group` when the snapshot has such a group: `Some(None)` for a group that
    /// tests with the app's default, `None` for a name no group has.
    pub fn group_own_test_url(&self, group: &str) -> Option<Option<&str>> {
        self.global
            .iter()
            .chain(&self.groups)
            .any(|known| known.name.as_str() == group)
            .then(|| self.group_test_url(group))
    }

    /// The group icons of the snapshot, by group name.
    pub fn group_icons(&self) -> IndexMap<ProxyName, String> {
        self.global
            .iter()
            .chain(&self.groups)
            .filter_map(|group| {
                let icon = self.nodes.get(&group.name)?.icon.as_deref()?;
                (!icon.is_empty()).then(|| (group.name.clone(), icon.to_owned()))
            })
            .collect()
    }

    /// The snapshot a `read` caller may see.
    ///
    /// Test URLs and icons are configuration that may carry a credential in any part, so no URL
    /// leaves. Each distinct URL in use, the app default and every group's own, becomes one delay
    /// source with one key (see [`ProxyGroup::delay_key`]); groups that share a URL, or whose URL
    /// is the default, refer to the same key, so a node's history is sent once per source. A node's
    /// `extra` is re-keyed by source, holding the newest record among the raw and the normalized
    /// form of its URL (the core keys `extra` by the string a test was requested with, and by a
    /// group's raw `testUrl` for its own health checks). Records of any other URL are dropped.
    pub fn trim_for_frontend(mut self, default_url: &str) -> Self {
        let mut sources = vec![(DEFAULT_TEST_URL_KEY.to_owned(), url_forms(default_url))];
        let mut group_keys = Vec::new();
        for group in self.global.iter().chain(&self.groups) {
            let Some(url) = self.group_test_url(group.name.as_str()) else {
                group_keys.push((group.name.clone(), DEFAULT_TEST_URL_KEY.to_owned()));
                continue;
            };
            let forms = url_forms(url);
            let key = match sources
                .iter_mut()
                .find(|(_, known)| known.iter().any(|form| forms.contains(form)))
            {
                Some((key, known)) => {
                    // The core keys a sample by the spelling the group wrote, so the shared
                    // source answers to every spelling any of its groups used.
                    for form in forms {
                        if !known.contains(&form) {
                            known.push(form);
                        }
                    }
                    key.clone()
                }
                None => {
                    let key = group.name.as_str().to_owned();
                    sources.push((key.clone(), forms));
                    key
                }
            };
            group_keys.push((group.name.clone(), key));
        }
        for group in self.global.iter_mut().chain(&mut self.groups) {
            if let Some((_, key)) = group_keys.iter().find(|(name, _)| *name == group.name) {
                group.delay_key.clone_from(key);
            }
        }

        self.nodes = self
            .nodes
            .into_iter()
            .map(|(name, node)| (name, public_node(node, &sources)))
            .collect();
        self
    }
}

/// A node as a `read` caller may see it: every field of the core's record is either kept or
/// dropped here by name, so a field the SDK adds does not reach a reader unreviewed. `test_url`
/// and `icon` are dropped (see [`Proxies::trim_for_frontend`]), and `extra` is re-keyed by delay
/// source.
fn public_node(node: Proxy, sources: &[(String, Vec<String>)]) -> Proxy {
    let Proxy {
        name,
        proxy_type,
        history,
        extra,
        alive,
        udp,
        uot,
        xudp,
        tfo,
        mptcp,
        smux,
        interface,
        routing_mark,
        provider_name,
        dialer_proxy,
        id,
        now,
        all,
        test_url: _,
        expected_status,
        fixed,
        hidden,
        icon: _,
        empty_fallback,
        provider,
    } = node;

    let extra = extra.map(|extra| {
        sources
            .iter()
            .filter_map(|(key, forms)| {
                let newest = forms
                    .iter()
                    .filter_map(|form| extra.get(form))
                    .max_by_key(|entry| entry.history.last().map(|sample| sample.time))?;
                Some((key.clone(), newest.clone()))
            })
            .collect::<IndexMap<_, _>>()
    });

    Proxy {
        name,
        proxy_type,
        history,
        extra,
        alive,
        udp,
        uot,
        xudp,
        tfo,
        mptcp,
        smux,
        interface,
        routing_mark,
        provider_name,
        dialer_proxy,
        id,
        now,
        all,
        test_url: None,
        expected_status,
        fixed,
        hidden,
        icon: None,
        empty_fallback,
        provider,
    }
}

/// `url` as written and as the backend normalizes it (`Url::as_str`).
fn url_forms(url: &str) -> Vec<String> {
    url::Url::parse(url)
        .map(|parsed| parsed.as_str().to_owned())
        .into_iter()
        .chain([url.to_owned()])
        .collect()
}

/// The `extra` key of the samples taken against the app's default test URL.
pub const DEFAULT_TEST_URL_KEY: &str = "";

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
    fn a_group_with_its_own_test_url_is_keyed_by_its_name() {
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
        assert_eq!(by_name("g").delay_key, "g");
        assert_eq!(by_name("g").expected_status.as_deref(), Some("204"));
        assert_eq!(by_name("e").delay_key, DEFAULT_TEST_URL_KEY);
        assert_eq!(by_name("e").expected_status, None);
        // The URL itself stays on the snapshot for the backend to test with.
        assert_eq!(
            proxies.group_test_url("g"),
            Some("https://cp.cloudflare.com")
        );
        assert_eq!(proxies.group_test_url("e"), None);
        assert!(
            !serde_json::to_string(&proxies.groups)
                .unwrap()
                .contains("cloudflare")
        );
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

    #[test]
    fn provider_summary_preserves_metadata_and_counts_all_nodes() {
        for vehicle in ["HTTP", "File", "Inline", "Compatible", "FutureVehicle"] {
            let provider: ProxyProvider = serde_json::from_value(json!({
                "name": "subscription/日本",
                "type": "FutureProvider",
                "vehicleType": vehicle,
                "updatedAt": "2026-10-18T12:00:00+08:00",
                "subscriptionInfo": {"Upload": -1, "Download": 2, "Total": 3, "Expire": 4},
                "testUrl": "https://example.com/test",
                "expectedStatus": "204",
                "proxies": [
                    item("used", "Vless", None, None),
                    item("unused", "Vless", None, None),
                ],
            }))
            .unwrap();

            assert_eq!(
                serde_json::to_value(ProxyProviderSummary::from(provider)).unwrap(),
                json!({
                    "name": "subscription/日本",
                    "type": "FutureProvider",
                    "vehicleType": vehicle,
                    "updatedAt": "2026-10-18T12:00:00+08:00",
                    "subscriptionInfo": {"Upload": -1, "Download": 2, "Total": 3, "Expire": 4},
                    "proxyCount": 2,
                })
            );
        }
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
        assert!(pinned.icon_configured);
        assert!(!unpinned.icon_configured);

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

    fn extra(urls: &[&str]) -> Option<IndexMap<String, clash_api::ProxyExtra>> {
        Some(
            urls.iter()
                .map(|url| {
                    (
                        url.to_string(),
                        clash_api::ProxyExtra {
                            alive: true,
                            history: Vec::new(),
                        },
                    )
                })
                .collect(),
        )
    }

    const DEFAULT: &str = "https://probe.example/default-sentinel/generate_204";
    const GROUP_A: &str = "https://user:group-a-sentinel@A.EXAMPLE/path-a-sentinel?key=a-sentinel";
    const GROUP_B: &str = "https://group-b-sentinel.example";

    fn sample(time: &str, delay: u16) -> clash_api::DelayHistory {
        serde_json::from_value(json!({ "time": time, "delay": delay })).unwrap()
    }

    fn entry(time: &str, delay: u16) -> clash_api::ProxyExtra {
        clash_api::ProxyExtra {
            alive: delay > 0,
            history: vec![sample(time, delay)],
        }
    }

    /// A node with a record under each `(url, time, delay)`.
    fn measured(name_: &str, records: &[(&str, &str, u16)]) -> Proxy {
        let mut node = item(name_, "Vless", None, None);
        node.extra = Some(
            records
                .iter()
                .map(|(url, time, delay)| (url.to_string(), entry(time, *delay)))
                .collect(),
        );
        node.test_url = Some("https://node-sentinel.example/".into());
        node.icon = Some("https://user:node-icon-sentinel@icons.example/x.png".into());
        node
    }

    fn normalized(url: &str) -> String {
        url::Url::parse(url).unwrap().as_str().to_owned()
    }

    fn with_urls(group: &str, url: &str, members: &[&str]) -> Proxy {
        let mut group = item(group, "URLTest", Some(members.to_vec()), None);
        group.test_url = Some(url.into());
        group.icon = Some("https://user:icon-sentinel@icons.example/g.png?k=icon-sentinel".into());
        group
    }

    fn assert_secret_free(proxies: &Proxies) {
        let json = serde_json::to_string(proxies).unwrap();
        assert!(!json.contains("sentinel"), "{json}");
        for host in ["probe.example", "a.example", "b.example", "icons.example"] {
            assert!(!json.contains(host), "{host} in {json}");
        }
    }

    #[test]
    fn trimming_sends_each_delay_source_under_a_key_that_is_not_its_url() {
        let default_normalized = normalized(DEFAULT);
        let a_normalized = normalized(GROUP_A);
        let b_normalized = normalized(GROUP_B);
        let node = measured(
            "n",
            &[
                // The default URL, raw (older) and normalized (newer).
                (DEFAULT, "2026-10-06T12:00:00+08:00", 1),
                (&default_normalized, "2026-10-06T13:00:00+08:00", 2),
                // GROUP_A, raw (newer) and normalized (older).
                (GROUP_A, "2026-10-06T15:00:00+08:00", 3),
                (&a_normalized, "2026-10-06T14:00:00+08:00", 4),
                // GROUP_B has only its normalized form.
                (&b_normalized, "2026-10-06T16:00:00+08:00", 5),
                // A URL no group or default uses.
                (
                    "https://other-sentinel.example/",
                    "2026-10-06T17:00:00+08:00",
                    6,
                ),
            ],
        );
        let mut proxies = records(&[
            with_urls("A", GROUP_A, &["n", "inner"]),
            with_urls("B", GROUP_B, &["n"]),
            item("C", "Selector", Some(vec!["n"]), None),
            // A nested group shown through A.
            item("inner", "Selector", Some(vec!["n"]), None),
        ]);
        proxies.insert(name("n"), node);
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let trimmed = full.clone().trim_for_frontend(DEFAULT);

        let extra = trimmed.nodes[&name("n")].extra.as_ref().unwrap();
        assert_eq!(
            extra.keys().map(String::as_str).collect::<Vec<_>>(),
            [DEFAULT_TEST_URL_KEY, "A", "B"]
        );
        assert_eq!(extra[DEFAULT_TEST_URL_KEY].history[0].delay, 2);
        assert_eq!(extra["A"].history[0].delay, 3);
        assert_eq!(extra["B"].history[0].delay, 5);

        let key_of = |group: &str| {
            trimmed
                .groups
                .iter()
                .find(|g| g.name.as_str() == group)
                .unwrap()
                .delay_key
                .clone()
        };
        assert_eq!(key_of("A"), "A");
        assert_eq!(key_of("B"), "B");
        assert_eq!(key_of("C"), DEFAULT_TEST_URL_KEY);
        assert_eq!(key_of("inner"), DEFAULT_TEST_URL_KEY);

        assert_secret_free(&trimmed);
        // The backend still resolves the URL a group tests with.
        assert_eq!(full.group_test_url("A"), Some(GROUP_A));
        assert_eq!(full.group_test_url("B"), Some(GROUP_B));
        assert_eq!(full.group_test_url("C"), None);
    }

    #[test]
    fn groups_sharing_a_url_share_one_key_and_one_history_copy() {
        let a_normalized = normalized(GROUP_A);
        let default_normalized = normalized(DEFAULT);
        let node = measured(
            "n",
            &[
                (GROUP_A, "2026-10-06T14:00:00+08:00", 3),
                (&a_normalized, "2026-10-06T15:00:00+08:00", 4),
                (&default_normalized, "2026-10-06T13:00:00+08:00", 2),
            ],
        );
        let mut proxies = records(&[
            // First and second use GROUP_A (raw and normalized spelling), third is the default.
            with_urls("first", GROUP_A, &["n"]),
            with_urls("second", &a_normalized, &["n"]),
            with_urls("third", DEFAULT, &["n"]),
            item("plain", "Selector", Some(vec!["n"]), None),
        ]);
        proxies.insert(name("n"), node);
        let trimmed = Proxies::from_responses(proxies, &IndexMap::new(), None)
            .unwrap()
            .trim_for_frontend(DEFAULT);

        let key_of = |group: &str| {
            trimmed
                .groups
                .iter()
                .find(|g| g.name.as_str() == group)
                .unwrap()
                .delay_key
                .clone()
        };
        assert_eq!(key_of("first"), "first");
        assert_eq!(key_of("second"), "first");
        assert_eq!(key_of("third"), DEFAULT_TEST_URL_KEY);
        assert_eq!(key_of("plain"), DEFAULT_TEST_URL_KEY);

        let extra = trimmed.nodes[&name("n")].extra.as_ref().unwrap();
        assert_eq!(
            extra.keys().map(String::as_str).collect::<Vec<_>>(),
            [DEFAULT_TEST_URL_KEY, "first"]
        );
        assert_eq!(extra["first"].history[0].delay, 4);
        assert_eq!(extra[DEFAULT_TEST_URL_KEY].history[0].delay, 2);
        assert_secret_free(&trimmed);
    }

    /// The core keys a sample by the URL as the group wrote it, so the newest sample of a shared
    /// source may sit only under a spelling that no earlier group of that source used.
    #[test]
    fn a_shared_source_reads_the_newest_sample_across_every_spelling() {
        const OTHER: &str = "https://other-sentinel.example/ping";
        let spellings = [
            "https://PROBE.example/default-sentinel/generate_204",
            "https://probe.EXAMPLE/default-sentinel/generate_204",
        ];
        assert_eq!(normalized(spellings[0]), normalized(DEFAULT));
        assert_eq!(normalized(spellings[1]), normalized(DEFAULT));
        let other_spellings = ["https://OTHER-sentinel.example/ping", OTHER];
        let node = measured(
            "n",
            &[
                (DEFAULT, "2026-10-06T12:00:00+08:00", 2),
                (spellings[0], "2026-10-06T14:00:00+08:00", 3),
                (spellings[1], "2026-10-06T16:00:00+08:00", 9),
                (other_spellings[0], "2026-10-06T10:00:00+08:00", 5),
                (other_spellings[1], "2026-10-06T11:00:00+08:00", 6),
            ],
        );
        let mut proxies = records(&[
            with_urls("upper", spellings[0], &["n"]),
            with_urls("mixed", spellings[1], &["n"]),
            with_urls("plain-default", DEFAULT, &["n"]),
            with_urls("other-upper", other_spellings[0], &["n"]),
            with_urls("other-lower", other_spellings[1], &["n"]),
        ]);
        proxies.insert(name("n"), node);
        let trimmed = Proxies::from_responses(proxies, &IndexMap::new(), None)
            .unwrap()
            .trim_for_frontend(DEFAULT);

        let key_of = |group: &str| {
            trimmed
                .groups
                .iter()
                .find(|g| g.name.as_str() == group)
                .unwrap()
                .delay_key
                .clone()
        };
        for group in ["upper", "mixed", "plain-default"] {
            assert_eq!(key_of(group), DEFAULT_TEST_URL_KEY, "{group}");
        }
        let other = key_of("other-upper");
        assert_eq!(key_of("other-lower"), other);
        assert_ne!(other, DEFAULT_TEST_URL_KEY);

        let extra = trimmed.nodes[&name("n")].extra.as_ref().unwrap();
        assert_eq!(
            extra.keys().map(String::as_str).collect::<Vec<_>>(),
            [DEFAULT_TEST_URL_KEY, other.as_str()]
        );
        assert_eq!(extra[DEFAULT_TEST_URL_KEY].history[0].delay, 9);
        assert_eq!(extra[other.as_str()].history[0].delay, 6);
        assert_secret_free(&trimmed);
    }

    #[test]
    fn trimming_clears_urls_and_icons_on_every_node_record_and_leaves_history() {
        let mut global = item("GLOBAL", "Selector", Some(vec!["g"]), Some("g"));
        global.test_url = Some("https://global-sentinel.example/".into());
        let mut proxies = records(&[global, with_urls("g", GROUP_A, &["a"])]);
        let mut a = measured(
            "a",
            &[(
                "https://other-sentinel.example/",
                "2026-10-06T17:00:00+08:00",
                6,
            )],
        );
        a.history = vec![sample("2026-10-06T12:34:56+08:00", 7)];
        proxies.insert(name("a"), a);
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let history = serde_json::to_value(&full.nodes[&name("a")].history).unwrap();

        let trimmed = full.trim_for_frontend(DEFAULT);
        for node in trimmed.nodes.values() {
            assert_eq!(node.test_url, None, "{}", node.name.as_str());
            assert_eq!(node.icon, None, "{}", node.name.as_str());
        }
        assert_eq!(
            serde_json::to_value(&trimmed.nodes[&name("a")].history).unwrap(),
            history
        );
        assert!(trimmed.groups[0].icon_configured);
        assert_secret_free(&trimmed);
    }

    #[test]
    fn group_icons_come_from_the_snapshot_not_the_read_view() {
        let mut global = item("GLOBAL", "Selector", Some(vec!["g", "plain"]), None);
        global.icon = Some("https://icons.example/global.png".into());
        let plain = item("plain", "Selector", Some(vec!["DIRECT"]), None);
        let proxies = records(&[global, with_urls("g", GROUP_A, &["DIRECT"]), plain]);
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();

        let icons = full.group_icons();
        assert_eq!(icons.len(), 2);
        assert_eq!(
            icons[&name("g")],
            "https://user:icon-sentinel@icons.example/g.png?k=icon-sentinel"
        );
        assert!(icons.contains_key(&name("GLOBAL")));
        assert!(
            full.clone()
                .trim_for_frontend(DEFAULT)
                .group_icons()
                .is_empty()
        );
    }

    #[test]
    fn trimming_leaves_nodes_without_extra_alone() {
        let mut a = item("a", "Vless", None, None);
        a.extra = Some(IndexMap::new());
        let mut proxies = records(&[]);
        proxies.insert(name("a"), a);
        let trimmed = Proxies::from_responses(proxies, &IndexMap::new(), None)
            .unwrap()
            .trim_for_frontend("https://d/");
        assert_eq!(trimmed.nodes[&name("a")].extra, Some(IndexMap::new()));
        assert_eq!(trimmed.nodes[&name("DIRECT")].extra, None);
    }

    /// Prints the payload saved for a 1000-node subscription; numbers go in the PR.
    #[test]
    fn trimming_shrinks_a_large_snapshot() {
        let urls = ["https://g/", "https://other-1/", "https://other-2/"];
        let history: Vec<clash_api::DelayHistory> = (0..10)
            .map(|delay| {
                serde_json::from_value(
                    json!({"time":"2026-10-06T12:34:56.123456789+08:00","delay":delay}),
                )
                .unwrap()
            })
            .collect();
        let names: Vec<String> = (0..1000).map(|i| format!("node-{i}")).collect();
        let mut group = item(
            "g",
            "URLTest",
            Some(names.iter().map(String::as_str).collect()),
            None,
        );
        group.test_url = Some(urls[0].into());
        let mut proxies = records(&[group]);
        for node_name in &names {
            let mut node = item(node_name, "Vless", None, None);
            node.history = history.clone();
            node.extra = Some(
                urls.iter()
                    .map(|url| {
                        (
                            url.to_string(),
                            clash_api::ProxyExtra {
                                alive: true,
                                history: history.clone(),
                            },
                        )
                    })
                    .collect(),
            );
            proxies.insert(name(node_name), node);
        }
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let before = serde_json::to_vec(&full).unwrap().len();
        let after = serde_json::to_vec(&full.trim_for_frontend(urls[0]))
            .unwrap()
            .len();
        println!("proxies payload: {before} -> {after} bytes");
        assert!(after < before);
    }
}
