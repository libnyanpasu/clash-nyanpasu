use crate::model::*;
use std::collections::BTreeMap;
/// Chains are logical selections in exit-first wire order, not network hops.
pub fn project(paths: Vec<TopologyPath>) -> TrafficResult<(Vec<TopologyNode>, Vec<TopologyEdge>)> {
    let mut nodes: BTreeMap<String, TopologyNode> = BTreeMap::new();
    let mut edges: BTreeMap<(String, String), TopologyEdge> = BTreeMap::new();
    for p in paths {
        let rule = serde_json::to_string(&p.dimensions.rule)
            .map_err(|e| StoreError::InvalidData(e.to_string()))?;
        let process = if p.dimensions.process == "unknown" {
            p.dimensions.source.clone()
        } else {
            p.dimensions.process.clone()
        };
        let group_chain: Vec<_> = p.dimensions.path.iter().skip(1).rev().cloned().collect();
        let mut layers = vec![(0, process), (1, rule)];
        if !group_chain.is_empty() {
            layers.push((2, group_chain.join(" → ")))
        }
        layers.push((3, p.dimensions.exit.clone()));
        let mut prior: Option<String> = None;
        for (layer, label) in layers {
            let identity = match layer {
                0 if p.dimensions.process == "unknown" => {
                    serde_json::json!([layer, "source", p.dimensions.source])
                }
                0 => serde_json::json!([layer, "process", p.dimensions.process]),
                1 => serde_json::json!([layer, p.dimensions.rule]),
                2 => serde_json::json!([layer, group_chain]),
                _ => serde_json::json!([layer, p.dimensions.exit]),
            };
            let id = identity.to_string();
            let filter = match layer {
                0 if p.dimensions.process == "unknown" => ConnectionFilter {
                    process: Some("unknown".into()),
                    source: Some(label.clone()),
                    ..Default::default()
                },
                0 => ConnectionFilter {
                    process: Some(label.clone()),
                    ..Default::default()
                },
                1 => ConnectionFilter {
                    rule: Some(p.dimensions.rule.clone()),
                    ..Default::default()
                },
                3 => ConnectionFilter {
                    exit: Some(label.clone()),
                    ..Default::default()
                },
                _ => ConnectionFilter {
                    group_chain: Some(group_chain.clone()),
                    ..Default::default()
                },
            };
            let node = nodes.entry(id.clone()).or_insert(TopologyNode {
                id: id.clone(),
                layer,
                label,
                bytes: Bytes::default(),
                filter,
                current_rate: Some(Rate {
                    upload: 0.0,
                    download: 0.0,
                }),
            });
            node.bytes = node.bytes.checked_add(&p.bytes)?;
            add_rate(&mut node.current_rate, &p.current_rate);
            if let Some(source) = prior {
                let edge = edges
                    .entry((source.clone(), id.clone()))
                    .or_insert(TopologyEdge {
                        source,
                        target: id.clone(),
                        bytes: Bytes::default(),
                        current_rate: Some(Rate {
                            upload: 0.0,
                            download: 0.0,
                        }),
                    });
                edge.bytes = edge.bytes.checked_add(&p.bytes)?;
                add_rate(&mut edge.current_rate, &p.current_rate)
            }
            prior = Some(id);
        }
    }
    Ok((nodes.into_values().collect(), edges.into_values().collect()))
}
fn add_rate(total: &mut Option<Rate>, rate: &Option<Rate>) {
    match (total.as_mut(), rate.as_ref()) {
        (Some(total), Some(rate)) => {
            total.upload += rate.upload;
            total.download += rate.download
        }
        _ => *total = None,
    }
}

/// Visible topology identity excludes target/protocol, preserving a source fallback.
pub fn path_dimensions(d: &Dimensions) -> Dimensions {
    Dimensions {
        process: d.process.clone(),
        source: if d.process == "unknown" {
            d.source.clone()
        } else {
            "unknown".into()
        },
        target: "unknown".into(),
        protocol: "unknown".into(),
        rule: d.rule.clone(),
        path: d.path.clone(),
        exit: d.exit.clone(),
    }
}
