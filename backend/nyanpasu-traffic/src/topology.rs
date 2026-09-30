use crate::model::*;
use std::collections::BTreeMap;

/// Layers are source (0), rule (1), group chain (2, omitted without groups) and exit (3). Chains
/// are logical selections in clash's wire order, not network hops.
pub fn project(paths: &[TopologyPath]) -> (Vec<TopologyNode>, Vec<TopologyEdge>) {
    let mut nodes: BTreeMap<String, TopologyNode> = BTreeMap::new();
    let mut edges: BTreeMap<(String, String), TopologyEdge> = BTreeMap::new();

    for path in paths {
        let key = &path.key;
        let mut layers = vec![
            (0, key.source.clone(), node_id(0, &[&key.source])),
            (
                1,
                key.rule.label(),
                node_id(1, &[&key.rule.kind, &key.rule.payload]),
            ),
        ];
        if !key.groups.is_empty() {
            let groups: Vec<&str> = key.groups.iter().map(String::as_str).collect();
            layers.push((2, key.groups.join(" → "), node_id(2, &groups)));
        }
        layers.push((3, key.exit.clone(), node_id(3, &[&key.exit])));

        let mut previous: Option<String> = None;
        for (layer, label, id) in layers {
            let node = nodes.entry(id.clone()).or_insert_with(|| TopologyNode {
                id: id.clone(),
                layer,
                label,
                bytes: Bytes::default(),
            });
            node.bytes = node.bytes.saturating_add(path.bytes);

            if let Some(source) = previous {
                let edge = edges
                    .entry((source.clone(), id.clone()))
                    .or_insert_with(|| TopologyEdge {
                        source,
                        target: id.clone(),
                        bytes: Bytes::default(),
                    });
                edge.bytes = edge.bytes.saturating_add(path.bytes);
            }
            previous = Some(id);
        }
    }

    (nodes.into_values().collect(), edges.into_values().collect())
}

/// Unique per layer and identity; parts are JSON-encoded so separators inside them cannot collide.
fn node_id(layer: u8, parts: &[&str]) -> String {
    format!("{layer}:{}", serde_json::Value::from(parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(
        source: &str,
        kind: &str,
        payload: &str,
        groups: &[&str],
        exit: &str,
        up: u64,
    ) -> TopologyPath {
        TopologyPath {
            key: TopologyKey {
                source: source.into(),
                rule: RuleKey {
                    kind: kind.into(),
                    payload: payload.into(),
                },
                groups: groups.iter().map(|g| (*g).to_owned()).collect(),
                exit: exit.into(),
            },
            bytes: Bytes {
                upload: up,
                download: up * 2,
            },
        }
    }

    fn node<'a>(nodes: &'a [TopologyNode], layer: u8, label: &str) -> &'a TopologyNode {
        nodes
            .iter()
            .find(|n| n.layer == layer && n.label == label)
            .unwrap_or_else(|| panic!("missing node {layer}:{label}"))
    }

    fn edge_bytes(
        nodes: &[TopologyNode],
        edges: &[TopologyEdge],
        from: (u8, &str),
        to: (u8, &str),
    ) -> Bytes {
        let (from, to) = (node(nodes, from.0, from.1), node(nodes, to.0, to.1));
        edges
            .iter()
            .find(|e| e.source == from.id && e.target == to.id)
            .unwrap_or_else(|| panic!("missing edge {} -> {}", from.id, to.id))
            .bytes
    }

    #[test]
    fn sums_shared_nodes_and_edges() {
        let paths = [
            path("curl", "Match", "", &["Proxy", "Auto"], "Node-A", 10),
            path("curl", "Match", "", &["Proxy", "Auto"], "Node-B", 20),
            path("firefox", "Match", "", &["Proxy", "Auto"], "Node-A", 5),
        ];
        let (nodes, edges) = project(&paths);

        assert_eq!(nodes.len(), 2 + 1 + 1 + 2);
        assert_eq!(
            node(&nodes, 0, "curl").bytes,
            Bytes {
                upload: 30,
                download: 60
            }
        );
        assert_eq!(
            node(&nodes, 1, "Match").bytes,
            Bytes {
                upload: 35,
                download: 70
            }
        );
        assert_eq!(
            node(&nodes, 2, "Proxy → Auto").bytes,
            Bytes {
                upload: 35,
                download: 70
            }
        );
        assert_eq!(
            node(&nodes, 3, "Node-A").bytes,
            Bytes {
                upload: 15,
                download: 30
            }
        );

        assert_eq!(
            edge_bytes(&nodes, &edges, (0, "curl"), (1, "Match")),
            Bytes {
                upload: 30,
                download: 60
            }
        );
        assert_eq!(
            edge_bytes(&nodes, &edges, (2, "Proxy → Auto"), (3, "Node-A")),
            Bytes {
                upload: 15,
                download: 30
            }
        );
        assert_eq!(edges.len(), 2 + 1 + 2);
    }

    #[test]
    fn omits_the_group_layer_without_groups() {
        let (nodes, edges) = project(&[path(
            "curl",
            "DomainSuffix",
            "example.com",
            &[],
            "DIRECT",
            1,
        )]);

        assert_eq!(nodes.iter().map(|n| n.layer).collect::<Vec<_>>(), [0, 1, 3]);
        assert_eq!(node(&nodes, 1, "DomainSuffix,example.com").layer, 1);
        assert_eq!(edges.len(), 2);
        assert_eq!(
            edge_bytes(
                &nodes,
                &edges,
                (1, "DomainSuffix,example.com"),
                (3, "DIRECT")
            ),
            Bytes {
                upload: 1,
                download: 2
            }
        );
    }

    #[test]
    fn same_label_on_different_layers_gets_distinct_ids() {
        let (nodes, _) = project(&[path("x", "Match", "", &[], "x", 1)]);

        assert_eq!(nodes.len(), 3);
        assert_ne!(node(&nodes, 0, "x").id, node(&nodes, 3, "x").id);
    }

    #[test]
    fn empty_input_projects_nothing() {
        let (nodes, edges) = project(&[]);
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }
}
