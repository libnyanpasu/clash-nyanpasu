//! Frontend projection of the core owner's untrimmed proxy snapshot.
use std::collections::HashSet;

use nyanpasu_core::clash::proxies::Proxies;

/// Drops every node's per-URL delay records except those in `keep`.
fn retain_extra(proxies: &mut Proxies, keep: &HashSet<&str>) {
    for node in proxies.nodes.values_mut() {
        if let Some(extra) = node.extra.as_mut() {
            extra.retain(|url, _| keep.contains(url.as_str()));
        }
    }
}

/// The frontend reads delays only under a group's test URL or the default
/// one, so other URLs' records are not sent. Both the raw and the
/// normalized form are kept: the core keys `extra` by the string a test
/// was requested with (normalized by the backend) and by a group's raw
/// testUrl for its own health checks.
pub(crate) fn trim_for_frontend(mut proxies: Proxies, default_url: &str) -> Proxies {
    let mut urls = Vec::new();
    for url in proxies
        .global
        .iter()
        .chain(&proxies.groups)
        .filter_map(|group| group.test_url.as_deref())
        .chain([default_url])
    {
        urls.extend(url::Url::parse(url).map(|parsed| parsed.as_str().to_owned()));
        urls.push(url.to_owned());
    }
    let keep: HashSet<&str> = urls.iter().map(String::as_str).collect();
    retain_extra(&mut proxies, &keep);
    proxies
}

#[cfg(test)]
mod tests {
    use super::*;
    use clash_api::{IndexMap, Proxy, ProxyName};
    use serde_json::json;

    fn record(value: serde_json::Value) -> Proxy {
        serde_json::from_value(value).unwrap()
    }

    fn item(name: &str, kind: &str, all: Option<Vec<&str>>, now: Option<&str>) -> Proxy {
        record(json!({
            "name": name, "type": kind, "udp": false, "history": [], "all": all, "now": now
        }))
    }

    fn name(value: &str) -> ProxyName {
        ProxyName::from(value)
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

    #[test]
    fn trimming_keeps_only_group_urls_and_the_default() {
        let mut group = item("g", "URLTest", Some(vec!["a", "DIRECT"]), Some("a"));
        group.test_url = Some("https://g/".into());
        let mut a = item("a", "Vless", None, None);
        a.extra = extra(&["https://g/", "https://d/", "https://other/"]);
        let mut proxies = records(&[group]);
        proxies.insert(name("a"), a);
        let trimmed = trim_for_frontend(
            Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap(),
            "https://d/",
        );
        let kept: Vec<_> = trimmed.nodes[&name("a")]
            .extra
            .as_ref()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(kept, ["https://g/", "https://d/"]);
    }

    /// The core keys `extra` by the URL it was asked with (normalized by the
    /// backend) and by a group's raw testUrl for its own health checks.
    #[test]
    fn trimming_keeps_raw_and_normalized_forms_of_a_bare_host_url() {
        let mut group = item("g", "URLTest", Some(vec!["a"]), Some("a"));
        group.test_url = Some("https://g".into());
        let mut a = item("a", "Vless", None, None);
        a.extra = extra(&["https://g", "https://g/", "https://d/", "https://other/"]);
        let mut proxies = records(&[group]);
        proxies.insert(name("a"), a);
        let trimmed = trim_for_frontend(
            Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap(),
            "https://d",
        );
        let kept: Vec<_> = trimmed.nodes[&name("a")]
            .extra
            .as_ref()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(kept, ["https://g", "https://g/", "https://d/"]);
    }

    #[test]
    fn trimming_keeps_global_and_hidden_group_urls_and_leaves_history() {
        let mut global = item("GLOBAL", "Selector", Some(vec!["g"]), Some("g"));
        global.test_url = Some("https://global/".into());
        let mut hidden = item("g", "URLTest", Some(vec!["a"]), Some("a"));
        hidden.test_url = Some("https://hidden/".into());
        hidden.hidden = Some(true);
        let mut a = item("a", "Vless", None, None);
        a.history = vec![
            serde_json::from_value(json!({"time":"2026-10-06T12:34:56+08:00","delay":7})).unwrap(),
        ];
        a.extra = extra(&["https://global/", "https://hidden/", "https://other/"]);
        let mut proxies = records(&[global, hidden]);
        proxies.insert(name("a"), a);
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let history = serde_json::to_value(&full.nodes[&name("a")].history).unwrap();
        let trimmed = trim_for_frontend(full, "https://d/");
        let node = &trimmed.nodes[&name("a")];
        let kept: Vec<_> = node.extra.as_ref().unwrap().keys().cloned().collect();
        assert_eq!(kept, ["https://global/", "https://hidden/"]);
        assert_eq!(serde_json::to_value(&node.history).unwrap(), history);
    }

    #[test]
    fn trimming_leaves_nodes_without_extra_alone() {
        let mut a = item("a", "Vless", None, None);
        a.extra = Some(IndexMap::new());
        let mut proxies = records(&[]);
        proxies.insert(name("a"), a);
        let trimmed = trim_for_frontend(
            Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap(),
            "https://d/",
        );
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
        let after = serde_json::to_vec(&trim_for_frontend(full, urls[0]))
            .unwrap()
            .len();
        println!("proxies payload: {before} -> {after} bytes");
        assert!(after < before);
    }

    #[test]
    fn trimming_an_owned_copy_leaves_the_raw_snapshot_unchanged() {
        let mut a = item("a", "Vless", None, None);
        a.extra = extra(&["https://d/", "https://other/"]);
        let mut proxies = records(&[]);
        proxies.insert(name("a"), a);
        let raw = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let before = serde_json::to_value(&raw).unwrap();

        let trimmed = trim_for_frontend(raw.clone(), "https://d/");
        assert_eq!(serde_json::to_value(&raw).unwrap(), before);
        assert!(
            raw.nodes[&name("a")]
                .extra
                .as_ref()
                .unwrap()
                .contains_key("https://other/")
        );
        assert!(
            !trimmed.nodes[&name("a")]
                .extra
                .as_ref()
                .unwrap()
                .contains_key("https://other/")
        );
    }
}
