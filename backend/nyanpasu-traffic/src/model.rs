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

pub(crate) const MAX_CLOSED_PAGE: usize = 500;

/// The store's key order for closed connections; connections closed before the epoch sort
/// first.
pub(crate) fn closed_key(closed_at: i64) -> u64 {
    u64::try_from(closed_at).unwrap_or(0)
}

impl ClosedConnection {
    fn cursor(&self) -> ClosedCursor {
        ClosedCursor {
            closed_at: self.closed_at,
            id: self.id.clone(),
        }
    }

    fn key(&self) -> (u64, &str) {
        (closed_key(self.closed_at), &self.id)
    }
}

impl ClosedCursor {
    fn key(&self) -> (u64, &str) {
        (closed_key(self.closed_at), &self.id)
    }
}

/// Adds the closed connections that are not stored yet to a stored page, keeping its
/// newest-first order and its cursor, so paging never skips or repeats a connection when a
/// flush moves them into the store between pages.
pub fn merge_closed_page(
    stored: ClosedPage,
    pending: &[ClosedConnection],
    before: Option<&ClosedCursor>,
    limit: usize,
) -> ClosedPage {
    let limit = limit.clamp(1, MAX_CLOSED_PAGE);
    let before = before.map(ClosedCursor::key);
    // The store's remaining entries sort below its page, so pending ones there belong to a
    // later page, which starts from this page's last entry.
    let floor = stored.next.as_ref().map(ClosedCursor::key);
    let mut connections = stored.connections;
    connections.extend(
        pending
            .iter()
            .filter(|c| before.is_none_or(|b| c.key() < b) && floor.is_none_or(|f| c.key() > f))
            .cloned(),
    );
    connections.sort_unstable_by(|a, b| b.key().cmp(&a.key()));
    let more = connections.len() > limit || stored.next.is_some();
    connections.truncate(limit);
    let next = if more {
        connections.last().map(ClosedConnection::cursor)
    } else {
        None
    };
    ClosedPage { connections, next }
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
    /// Closed in this session, including those not flushed yet.
    pub closed_connections: u64,
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

    fn closed(id: &str, closed_at: i64) -> ClosedConnection {
        ClosedConnection {
            id: id.into(),
            started_at: 0,
            first_seen_at: 0,
            closed_at,
            bytes: Bytes::default(),
            dimensions: dims(&["DIRECT"]),
        }
    }

    fn page(connections: Vec<ClosedConnection>, next: Option<&ClosedConnection>) -> ClosedPage {
        ClosedPage {
            connections,
            next: next.map(ClosedConnection::cursor),
        }
    }

    fn ids(page: &ClosedPage) -> Vec<&str> {
        page.connections.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn pending_closed_connections_lead_a_complete_stored_page() {
        let stored = page(vec![closed("b", 20), closed("a", 10)], None);
        let merged = merge_closed_page(stored, &[closed("c", 30)], None, 10);
        assert_eq!(ids(&merged), ["c", "b", "a"]);
        assert_eq!(merged.next, None);
    }

    #[test]
    fn a_truncated_merge_continues_after_its_last_entry() {
        let stored = page(vec![closed("b", 20), closed("a", 10)], None);
        let merged = merge_closed_page(stored, &[closed("c", 30)], None, 2);
        assert_eq!(ids(&merged), ["c", "b"]);
        assert_eq!(merged.next, Some(closed("b", 20).cursor()));
    }

    #[test]
    fn pending_older_than_a_partial_stored_page_waits_for_a_later_page() {
        let c = closed("c", 30);
        let stored = page(vec![closed("d", 40), c.clone()], Some(&c));
        let merged = merge_closed_page(stored, &[closed("e", 50), closed("a", 10)], None, 10);
        assert_eq!(ids(&merged), ["e", "d", "c"]);
        assert_eq!(merged.next, Some(c.cursor()));
    }

    #[test]
    fn pending_at_or_after_the_cursor_is_skipped() {
        let pending = [closed("c", 30), closed("b", 20), closed("a", 10)];
        let merged = merge_closed_page(
            page(Vec::new(), None),
            &pending,
            Some(&closed("b", 20).cursor()),
            10,
        );
        assert_eq!(ids(&merged), ["a"]);
        assert_eq!(merged.next, None);
    }

    #[test]
    fn merged_ties_on_closed_at_fall_back_to_the_id() {
        let stored = page(vec![closed("b", 20)], None);
        let merged = merge_closed_page(stored, &[closed("c", 20), closed("a", 20)], None, 10);
        assert_eq!(ids(&merged), ["c", "b", "a"]);
    }

    #[test]
    fn merged_pages_clamp_the_limit() {
        let pending: Vec<_> = (0..MAX_CLOSED_PAGE + 1)
            .map(|i| closed(&format!("{i:04}"), 10))
            .collect();
        let merged = merge_closed_page(page(Vec::new(), None), &pending, None, usize::MAX);
        assert_eq!(merged.connections.len(), MAX_CLOSED_PAGE);
        assert!(merged.next.is_some());

        let merged = merge_closed_page(page(Vec::new(), None), &pending, None, 0);
        assert_eq!(merged.connections.len(), 1);
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
