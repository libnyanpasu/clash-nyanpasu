use crate::{key::Key, query::Groups};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    model::{Dimension, Dimensions, Metric},
    query::Usage,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyNode {
    /// Unique over layer and key, see `node_id`.
    pub id: String,
    /// Index into the requested layers.
    pub layer: u8,
    /// The dimension value; `None` for the node that merges the layer's remaining nodes.
    pub key: Option<String>,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyEdge {
    pub source: String,
    pub target: String,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Topology {
    /// By layer, heaviest first, the merged node last.
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
}

/// Flows through the same borrowed grouping and projection path used for stored queries.
pub fn project<'a, I>(
    rows: I,
    layers: &[Dimension],
    metric: Metric,
    limit: Option<usize>,
) -> Topology
where
    I: IntoIterator<Item = (&'a Dimensions, Usage)>,
    I::IntoIter: Clone,
{
    let rows = rows.into_iter();
    let mut groups: Vec<_> = layers
        .iter()
        .map(|_| Groups::new(rows.size_hint().1.unwrap_or(0)))
        .collect();
    for (d, u) in rows.clone() {
        for (&dimension, g) in layers.iter().zip(&mut groups) {
            let key = Key::of(d, dimension);
            if dimension != Dimension::Chain || !key.is_empty() {
                g.add(Some(key), u, None);
            }
        }
    }
    let mut projection = Projection::new(groups.iter(), metric, limit);
    for (d, u) in rows {
        projection.session(d, u, layers);
    }
    projection.finish()
}

pub(crate) struct Projection<'a> {
    kept: Vec<HashMap<Key<'a>, usize>>,
    merged: Vec<Option<usize>>,
    nodes: Vec<TopologyNode>,
    edges: HashMap<(usize, usize), Usage>,
}
impl<'a> Projection<'a> {
    pub fn new<'g>(
        groups: impl Iterator<Item = &'g Groups<'a>>,
        metric: Metric,
        limit: Option<usize>,
    ) -> Self
    where
        'a: 'g,
    {
        let groups: Vec<_> = groups.collect();
        let node_count = groups
            .iter()
            .map(|g| {
                let kept = limit.unwrap_or(g.values.len()).min(g.values.len());
                kept + usize::from(g.has_overflow || g.values.len() > kept)
            })
            .sum();
        let mut out = Self {
            kept: Vec::with_capacity(5),
            merged: Vec::with_capacity(5),
            nodes: Vec::with_capacity(node_count),
            edges: HashMap::new(),
        };
        for (layer, g) in groups.into_iter().enumerate() {
            let ordered = g.ordered(metric);
            let count = limit.unwrap_or(ordered.len()).min(ordered.len());
            let mut kept = HashMap::with_capacity(count);
            for &(key, value) in &ordered[..count] {
                let text = key.owned();
                kept.insert(key, out.nodes.len());
                out.nodes.push(TopologyNode {
                    id: node_id(layer as u8, Some(&text)),
                    layer: layer as u8,
                    key: Some(text),
                    usage: value.usage,
                });
            }
            let merged = if g.has_overflow || ordered.len() > count {
                let usage = ordered[count..]
                    .iter()
                    .fold(g.overflow, |s, (_, v)| s.saturating_add(v.usage));
                let index = out.nodes.len();
                out.nodes.push(TopologyNode {
                    id: node_id(layer as u8, None),
                    layer: layer as u8,
                    key: None,
                    usage,
                });
                Some(index)
            } else {
                None
            };
            out.kept.push(kept);
            out.merged.push(merged);
        }
        let bound = out.nodes.len().saturating_mul(out.nodes.len());
        out.edges = HashMap::with_capacity(bound.min(4096));
        out
    }
    pub fn kept(&self, layer: usize, key: Option<Key<'_>>) -> Option<usize> {
        key.and_then(|k| self.kept[layer].get(&k).copied())
    }
    pub fn resolve(&self, layer: usize, key: Option<Key<'_>>) -> Option<usize> {
        key.and_then(|k| self.kept[layer].get(&k).copied())
            .or(self.merged[layer])
    }
    pub fn session(&mut self, d: &Dimensions, usage: Usage, layers: &[Dimension]) {
        let mut path = [None; 5];
        for (i, &dimension) in layers.iter().enumerate() {
            let key = Key::of(d, dimension);
            if dimension != Dimension::Chain || !key.is_empty() {
                path[i] = self.resolve(i, Some(key));
            }
        }
        self.path(path.into_iter().take(layers.len()), usage);
    }
    pub fn path(&mut self, path: impl Iterator<Item = Option<usize>>, usage: Usage) {
        let mut previous = None;
        for node in path.flatten() {
            if let Some(source) = previous.replace(node) {
                let slot = self.edges.entry((source, node)).or_default();
                *slot = slot.saturating_add(usage);
            }
        }
    }
    pub fn finish(self) -> Topology {
        let mut edges: Vec<_> = self
            .edges
            .into_iter()
            .map(|((s, t), usage)| TopologyEdge {
                source: self.nodes[s].id.clone(),
                target: self.nodes[t].id.clone(),
                usage,
            })
            .collect();
        edges.sort_unstable_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
        Topology {
            nodes: self.nodes,
            edges,
        }
    }
}

/// JSON keeps separators inside keys from colliding, and None never equals a real key.
fn node_id(layer: u8, key: Option<&str>) -> String {
    serde_json::to_string(&(layer, key)).expect("a number and a string always encode")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bytes, RuleKey};

    fn dims(process: &str, rule: &str, groups: &[&str], exit: &str) -> Dimensions {
        Dimensions {
            process: process.into(),
            source: "192.168.1.2".into(),
            inbound: "mixed".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: rule.into(),
                payload: String::new(),
            },
            chains: std::iter::once(exit)
                .chain(groups.iter().rev().copied())
                .map(str::to_owned)
                .collect(),
            profile: None,
            source_region: "unknown".into(),
            destination_region: "unknown".into(),
            destination_basis: None,
        }
    }

    fn usage(bytes: u64, connections: u64) -> Usage {
        Usage {
            bytes: Bytes {
                upload: bytes,
                download: 0,
            },
            connections,
        }
    }

    const LAYERS: [Dimension; 4] = [
        Dimension::Origin,
        Dimension::Rule,
        Dimension::Chain,
        Dimension::Exit,
    ];

    fn run(
        rows: &[(Dimensions, Usage)],
        layers: &[Dimension],
        metric: Metric,
        limit: Option<usize>,
    ) -> Topology {
        project(rows.iter().map(|(d, u)| (d, *u)), layers, metric, limit)
    }

    fn node<'a>(t: &'a Topology, layer: u8, key: Option<&str>) -> &'a TopologyNode {
        t.nodes
            .iter()
            .find(|n| n.layer == layer && n.key.as_deref() == key)
            .unwrap_or_else(|| panic!("missing node {layer}:{key:?}"))
    }

    fn edge(t: &Topology, from: (u8, Option<&str>), to: (u8, Option<&str>)) -> Usage {
        let (from, to) = (node(t, from.0, from.1), node(t, to.0, to.1));
        t.edges
            .iter()
            .find(|e| e.source == from.id && e.target == to.id)
            .unwrap_or_else(|| panic!("missing edge {} -> {}", from.id, to.id))
            .usage
    }

    fn layer_total(t: &Topology, layer: u8) -> Usage {
        t.nodes
            .iter()
            .filter(|n| n.layer == layer)
            .fold(Usage::default(), |sum, n| sum.saturating_add(n.usage))
    }

    #[test]
    fn sums_shared_nodes_and_edges() {
        let rows = [
            (
                dims("curl", "Match", &["Proxy", "Auto"], "Node-A"),
                usage(10, 1),
            ),
            (
                dims("curl", "Match", &["Proxy", "Auto"], "Node-B"),
                usage(20, 1),
            ),
            (
                dims("firefox", "Match", &["Proxy", "Auto"], "Node-A"),
                usage(5, 1),
            ),
        ];
        let t = run(&rows, &LAYERS, Metric::Bytes, None);

        assert_eq!(t.nodes.len(), 2 + 1 + 1 + 2);
        assert_eq!(node(&t, 0, Some("curl")).usage, usage(30, 2));
        assert_eq!(node(&t, 1, Some("Match")).usage, usage(35, 3));
        assert_eq!(node(&t, 2, Some("Proxy → Auto")).usage, usage(35, 3));
        assert_eq!(node(&t, 3, Some("Node-A")).usage, usage(15, 2));
        assert_eq!(
            edge(&t, (2, Some("Proxy → Auto")), (3, Some("Node-A"))),
            usage(15, 2)
        );
        assert_eq!(t.edges.len(), 2 + 1 + 2);
    }

    #[test]
    fn nodes_are_ordered_by_layer_then_metric() {
        let rows = [
            (dims("small", "Match", &[], "X"), usage(1, 9)),
            (dims("big", "Match", &[], "X"), usage(9, 1)),
        ];
        let by = |metric| {
            run(&rows, &[Dimension::Origin], metric, None)
                .nodes
                .into_iter()
                .filter_map(|n| n.key)
                .collect::<Vec<_>>()
        };
        assert_eq!(by(Metric::Bytes), ["big", "small"]);
        assert_eq!(by(Metric::Connections), ["small", "big"]);
    }

    #[test]
    fn an_empty_chain_skips_its_layer() {
        let rows = [(dims("curl", "Match", &[], "DIRECT"), usage(1, 1))];
        let t = run(&rows, &LAYERS, Metric::Bytes, None);

        assert_eq!(
            t.nodes.iter().map(|n| n.layer).collect::<Vec<_>>(),
            [0, 1, 3]
        );
        assert_eq!(t.edges.len(), 2);
        assert_eq!(
            edge(&t, (1, Some("Match")), (3, Some("DIRECT"))),
            usage(1, 1)
        );
    }

    #[test]
    fn the_same_key_on_different_layers_gets_distinct_ids() {
        let rows = [(dims("x", "Match", &[], "x"), usage(1, 1))];
        let t = run(
            &rows,
            &[Dimension::Origin, Dimension::Exit],
            Metric::Bytes,
            None,
        );

        assert_eq!(t.nodes.len(), 2);
        assert_ne!(node(&t, 0, Some("x")).id, node(&t, 1, Some("x")).id);
        assert_eq!(node(&t, 0, Some("x")).id, r#"[0,"x"]"#);
    }

    #[test]
    fn the_merged_node_never_collides_with_a_key() {
        assert_ne!(node_id(0, None), node_id(0, Some("null")));
        assert_ne!(node_id(0, None), node_id(0, Some("")));
    }

    fn crowded() -> Vec<(Dimensions, Usage)> {
        // Per origin: a 10, b 6, c 3, d 1 (bytes); each goes to its own exit.
        [("a", 10), ("b", 6), ("c", 3), ("d", 1)]
            .into_iter()
            .map(|(p, b)| (dims(p, "Match", &[], &format!("exit-{p}")), usage(b, 1)))
            .collect()
    }

    #[test]
    fn the_rest_of_a_layer_merges_into_other_and_flow_is_conserved() {
        let t = run(
            &crowded(),
            &[Dimension::Origin, Dimension::Exit],
            Metric::Bytes,
            Some(2),
        );

        assert_eq!(t.nodes.len(), 3 + 3);
        assert_eq!(node(&t, 0, None).usage, usage(4, 2));
        assert_eq!(node(&t, 1, None).usage, usage(4, 2));
        for layer in 0..2 {
            assert_eq!(layer_total(&t, layer), usage(20, 4));
        }
        assert_eq!(edge(&t, (0, Some("a")), (1, Some("exit-a"))), usage(10, 1));
        // c -> exit-c and d -> exit-d both land between the two merged nodes.
        assert_eq!(edge(&t, (0, None), (1, None)), usage(4, 2));
        let edges: u64 = t.edges.iter().map(|e| e.usage.bytes.upload).sum();
        assert_eq!(edges, 20);
    }

    #[test]
    fn edges_between_a_kept_and_a_merged_node_are_redirected() {
        let rows = [
            (dims("a", "Match", &[], "big"), usage(10, 1)),
            (dims("a", "Match", &[], "mid"), usage(5, 1)),
            (dims("a", "Match", &[], "small"), usage(1, 1)),
            (dims("a", "Match", &[], "tiny"), usage(1, 1)),
        ];
        let t = run(
            &rows,
            &[Dimension::Origin, Dimension::Exit],
            Metric::Bytes,
            Some(2),
        );

        assert_eq!(edge(&t, (0, Some("a")), (1, None)), usage(2, 2));
        assert_eq!(edge(&t, (0, Some("a")), (1, Some("big"))), usage(10, 1));
    }

    #[test]
    fn without_a_limit_nothing_merges() {
        let t = run(
            &crowded(),
            &[Dimension::Origin, Dimension::Exit],
            Metric::Bytes,
            None,
        );
        assert_eq!(t.nodes.len(), 8);
        assert!(t.nodes.iter().all(|n| n.key.is_some()));
    }

    #[test]
    fn a_layer_within_the_limit_has_no_other_node() {
        let t = run(
            &crowded(),
            &[Dimension::Origin, Dimension::Exit],
            Metric::Bytes,
            Some(4),
        );
        assert!(t.nodes.iter().all(|n| n.key.is_some()));
    }

    #[test]
    fn connections_decide_what_is_kept() {
        let rows = [
            (dims("chatty", "Match", &[], "X"), usage(1, 50)),
            (dims("heavy", "Match", &[], "X"), usage(100, 1)),
        ];
        let t = run(&rows, &[Dimension::Origin], Metric::Connections, Some(1));
        assert!(t.nodes.iter().any(|n| n.key.as_deref() == Some("chatty")));
        assert_eq!(node(&t, 0, None).usage, usage(100, 1));
    }

    #[test]
    fn empty_input_projects_nothing() {
        let t = run(&[], &LAYERS, Metric::Bytes, Some(3));
        assert!(t.nodes.is_empty());
        assert!(t.edges.is_empty());
    }
}
