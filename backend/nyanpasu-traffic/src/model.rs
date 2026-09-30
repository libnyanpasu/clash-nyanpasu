use serde::{Deserialize, Serialize};

const UNKNOWN: &str = "unknown";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Bytes {
    pub upload: u64,
    pub download: u64,
}

impl Bytes {
    pub fn saturating_add(self, other: Bytes) -> Bytes {
        Bytes {
            upload: self.upload.saturating_add(other.upload),
            download: self.download.saturating_add(other.download),
        }
    }

    pub fn total(self) -> u128 {
        u128::from(self.upload) + u128::from(self.download)
    }
}

/// Whole bytes per second, rounded down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Rate {
    pub upload: u64,
    pub download: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RuleKey {
    pub kind: String,
    pub payload: String,
}

impl RuleKey {
    pub fn label(&self) -> String {
        if self.payload.is_empty() {
            self.kind.clone()
        } else {
            format!("{},{}", self.kind, self.payload)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Dimensions {
    /// Process path or name.
    pub process: String,
    pub source: String,
    /// Host, else destination IP.
    pub target: String,
    pub protocol: String,
    pub rule: RuleKey,
    /// Clash wire order: exit first, outermost group last.
    pub chains: Vec<String>,
}

impl Dimensions {
    pub fn exit(&self) -> &str {
        self.chains.first().map_or(UNKNOWN, String::as_str)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum GroupBy {
    Process,
    Source,
    Target,
    Protocol,
    Rule,
    Exit,
    Chain,
}

impl GroupBy {
    pub const ALL: [GroupBy; 7] = [
        GroupBy::Process,
        GroupBy::Source,
        GroupBy::Target,
        GroupBy::Protocol,
        GroupBy::Rule,
        GroupBy::Exit,
        GroupBy::Chain,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GroupBy::Process => "process",
            GroupBy::Source => "source",
            GroupBy::Target => "target",
            GroupBy::Protocol => "protocol",
            GroupBy::Rule => "rule",
            GroupBy::Exit => "exit",
            GroupBy::Chain => "chain",
        }
    }
}

pub fn group_key(d: &Dimensions, g: GroupBy) -> String {
    match g {
        GroupBy::Process => d.process.clone(),
        GroupBy::Source => d.source.clone(),
        GroupBy::Target => d.target.clone(),
        GroupBy::Protocol => d.protocol.clone(),
        GroupBy::Rule => d.rule.label(),
        GroupBy::Exit => d.exit().to_owned(),
        GroupBy::Chain if d.chains.is_empty() => UNKNOWN.to_owned(),
        GroupBy::Chain => d
            .chains
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join(" → "),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyKey {
    /// Process, or the source IP when the process is unknown.
    pub source: String,
    pub rule: RuleKey,
    /// Outermost group first, exit excluded.
    pub groups: Vec<String>,
    pub exit: String,
}

impl TopologyKey {
    pub fn from_dimensions(d: &Dimensions) -> Self {
        Self {
            source: if d.process == UNKNOWN {
                d.source.clone()
            } else {
                d.process.clone()
            },
            rule: d.rule.clone(),
            groups: d.chains.iter().skip(1).rev().cloned().collect(),
            exit: d.exit().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SessionMeta {
    pub profile: Option<String>,
    /// Wall clock, milliseconds.
    pub started_at: i64,
    pub last_sample_at: Option<i64>,
    /// From the core's global counters.
    pub core_bytes: Bytes,
    /// Scope of the counter baselines.
    pub instance_id: Option<String>,
    /// Last observed core global counters of `instance_id`.
    pub global_counters: Option<Bytes>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ActiveConnection {
    pub id: String,
    pub started_at: i64,
    pub first_seen_at: i64,
    /// Last observed cumulative counters.
    pub counters: Bytes,
    /// Counted in this session.
    pub bytes: Bytes,
    pub dimensions: Dimensions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ClosedConnection {
    pub id: String,
    pub started_at: i64,
    pub first_seen_at: i64,
    pub closed_at: i64,
    pub bytes: Bytes,
    pub dimensions: Dimensions,
}

/// Exclusive position for newest-first paging.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ClosedCursor {
    pub closed_at: i64,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ClosedPage {
    pub connections: Vec<ClosedConnection>,
    pub next: Option<ClosedCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageGroup {
    pub key: String,
    pub bytes: Bytes,
    pub current_rate: Option<Rate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Usage {
    pub total: Bytes,
    pub groups: Vec<UsageGroup>,
    pub other: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyPath {
    pub key: TopologyKey,
    pub bytes: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyNode {
    pub id: String,
    pub layer: u8,
    pub label: String,
    pub bytes: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyEdge {
    pub source: String,
    pub target: String,
    pub bytes: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Topology {
    pub paths: Vec<TopologyPath>,
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub other: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficSummary {
    pub profile: Option<String>,
    pub started_at: i64,
    pub last_sample_at: Option<i64>,
    pub core_bytes: Bytes,
    pub active_connections: u64,
    pub current_rate: Option<Rate>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrafficError {
    #[error("traffic storage: {0}")]
    Storage(String),
}

pub type TrafficResult<T> = Result<T, TrafficError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(chains: &[&str]) -> Dimensions {
        Dimensions {
            process: "/usr/bin/curl".into(),
            source: "192.168.1.2".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "DomainSuffix".into(),
                payload: "example.com".into(),
            },
            chains: chains.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn group_keys() {
        let d = dims(&["Node-A", "Auto", "Proxy"]);
        assert_eq!(group_key(&d, GroupBy::Process), "/usr/bin/curl");
        assert_eq!(group_key(&d, GroupBy::Source), "192.168.1.2");
        assert_eq!(group_key(&d, GroupBy::Target), "example.com");
        assert_eq!(group_key(&d, GroupBy::Protocol), "tcp");
        assert_eq!(group_key(&d, GroupBy::Rule), "DomainSuffix,example.com");
        assert_eq!(group_key(&d, GroupBy::Exit), "Node-A");
        assert_eq!(group_key(&d, GroupBy::Chain), "Proxy → Auto → Node-A");
    }

    #[test]
    fn group_keys_with_empty_parts() {
        let mut d = dims(&["DIRECT"]);
        d.rule.payload.clear();
        assert_eq!(group_key(&d, GroupBy::Rule), "DomainSuffix");
        assert_eq!(group_key(&d, GroupBy::Chain), "DIRECT");

        d.chains.clear();
        assert_eq!(group_key(&d, GroupBy::Exit), "unknown");
        assert_eq!(group_key(&d, GroupBy::Chain), "unknown");
    }

    #[test]
    fn topology_key_falls_back_to_source_ip() {
        let mut d = dims(&["Node-A", "Auto", "Proxy"]);
        let key = TopologyKey::from_dimensions(&d);
        assert_eq!(key.source, "/usr/bin/curl");
        assert_eq!(key.groups, ["Proxy", "Auto"]);
        assert_eq!(key.exit, "Node-A");

        d.process = "unknown".into();
        assert_eq!(TopologyKey::from_dimensions(&d).source, "192.168.1.2");
    }

    #[test]
    fn bytes_saturate() {
        let a = Bytes {
            upload: u64::MAX,
            download: 1,
        };
        let sum = a.saturating_add(Bytes {
            upload: 1,
            download: 2,
        });
        assert_eq!(
            sum,
            Bytes {
                upload: u64::MAX,
                download: 3
            }
        );
        assert_eq!(sum.total(), u128::from(u64::MAX) + 3);
    }
}
