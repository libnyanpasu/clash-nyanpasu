use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::query::Usage;

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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Dimensions {
    /// Process path or name.
    pub process: String,
    pub source: String,
    /// `inbound_user`, else `inbound_name`.
    #[serde(default = "unknown")]
    pub inbound: String,
    /// Host, else destination IP.
    pub target: String,
    pub protocol: String,
    pub rule: RuleKey,
    /// Clash wire order: exit first, outermost group last.
    pub chains: Vec<String>,
    /// The profile that was current when the connection first appeared; never rewritten.
    #[serde(default)]
    pub profile: Option<String>,
    /// An upper-case country code, or `unknown`.
    #[serde(default = "unknown")]
    pub source_region: String,
    #[serde(default = "unknown")]
    pub destination_region: String,
    /// How `destination_region` was located; `None` while it is unknown.
    #[serde(default)]
    pub destination_basis: Option<GeoBasis>,
}

fn unknown() -> String {
    UNKNOWN.to_owned()
}

/// What the address a destination was located by is to the outbound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum GeoBasis {
    /// The outbound dialed this address.
    Dialed,
    /// The outbound was handed the host; the core resolved this address only for its rules.
    Resolved,
}

impl GeoBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            GeoBasis::Dialed => "dialed",
            GeoBasis::Resolved => "resolved",
        }
    }
}

impl Dimensions {
    pub fn exit(&self) -> &str {
        self.chains.first().map_or(UNKNOWN, String::as_str)
    }

    /// The process when it is known, else the source IP.
    pub fn origin(&self) -> &str {
        if self.process == UNKNOWN {
            &self.source
        } else {
            &self.process
        }
    }

    /// Strategy groups outermost first, exit excluded.
    pub fn groups(&self) -> impl Iterator<Item = &str> {
        self.chains.iter().skip(1).rev().map(String::as_str)
    }
}

/// A way to slice usage: ranking, filtering and topology layers all name dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Dimension {
    /// Derived: the process when known, else the source.
    Origin,
    Process,
    Source,
    Inbound,
    Target,
    Protocol,
    Rule,
    /// The strategy groups without the exit, outermost first; empty without groups.
    Chain,
    Exit,
    Profile,
    SourceRegion,
    DestinationRegion,
    /// How the destination region was located; empty while it is unknown.
    DestinationBasis,
}

impl Dimension {
    pub const ALL: [Dimension; 13] = [
        Dimension::Origin,
        Dimension::Process,
        Dimension::Source,
        Dimension::Inbound,
        Dimension::Target,
        Dimension::Protocol,
        Dimension::Rule,
        Dimension::Chain,
        Dimension::Exit,
        Dimension::Profile,
        Dimension::SourceRegion,
        Dimension::DestinationRegion,
        Dimension::DestinationBasis,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Dimension::Origin => "origin",
            Dimension::Process => "process",
            Dimension::Source => "source",
            Dimension::Inbound => "inbound",
            Dimension::Target => "target",
            Dimension::Protocol => "protocol",
            Dimension::Rule => "rule",
            Dimension::Chain => "chain",
            Dimension::Exit => "exit",
            Dimension::Profile => "profile",
            Dimension::SourceRegion => "source_region",
            Dimension::DestinationRegion => "destination_region",
            Dimension::DestinationBasis => "destination_basis",
        }
    }
}

pub fn group_key(d: &Dimensions, g: Dimension) -> String {
    match g {
        Dimension::Origin => d.origin().to_owned(),
        Dimension::Process => d.process.clone(),
        Dimension::Source => d.source.clone(),
        Dimension::Inbound => d.inbound.clone(),
        Dimension::Target => d.target.clone(),
        Dimension::Protocol => d.protocol.clone(),
        Dimension::Rule => d.rule.label(),
        Dimension::Chain => d.groups().collect::<Vec<_>>().join(" → "),
        Dimension::Exit => d.exit().to_owned(),
        Dimension::Profile => d.profile.clone().unwrap_or_default(),
        Dimension::SourceRegion => d.source_region.clone(),
        Dimension::DestinationRegion => d.destination_region.clone(),
        Dimension::DestinationBasis => d.destination_basis.map_or("", GeoBasis::as_str).to_owned(),
    }
}

/// Granularity of the usage tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Tier {
    Minute,
    Hour,
}

/// How far back a query looks; always ends now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum TrafficRange {
    LastHour,
    Last6Hours,
    Last24Hours,
    Last7Days,
    Last30Days,
    All,
}

/// Which connections a query counts: `All` is `Active` plus `Closed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum TrafficScope {
    All,
    Active,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficFilter {
    pub dimension: Dimension,
    pub value: String,
}

/// Filters on different dimensions combine with AND; a dimension holds at most one value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficQuery {
    pub range: TrafficRange,
    pub scope: TrafficScope,
    pub filters: Vec<TrafficFilter>,
}

impl TrafficQuery {
    /// Rejects filters that name a dimension more than once.
    pub fn check(&self) -> TrafficResult<()> {
        check_filters(&self.filters)
    }
}

/// Rejects filters that name a dimension more than once.
pub fn check_filters(filters: &[TrafficFilter]) -> TrafficResult<()> {
    match (1..filters.len()).find(|&i| {
        filters[..i]
            .iter()
            .any(|f| f.dimension == filters[i].dimension)
    }) {
        Some(i) => Err(TrafficError::InvalidRequest(format!(
            "the filter {} repeats {:?}",
            i + 1,
            filters[i].dimension
        ))),
        None => Ok(()),
    }
}

/// What a topology orders and merges its nodes by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Metric {
    Bytes,
    Connections,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SessionMeta {
    /// Wall clock, milliseconds.
    pub last_sample_at: Option<i64>,
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
    pub dimensions: Dimensions,
    /// Traffic per minute bucket; buckets older than the minute tier's retention are dropped,
    /// the hour buckets hold that traffic.
    pub minutes: BTreeMap<u32, Bytes>,
    /// Traffic per hour bucket.
    pub hours: BTreeMap<u32, Bytes>,
}

impl ActiveConnection {
    /// Everything counted for the connection.
    pub fn bytes(&self) -> Bytes {
        self.hours
            .values()
            .fold(Bytes::default(), |sum, bytes| sum.saturating_add(*bytes))
    }
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
/// How many stored closed connections one page reads at most; a selection that matches few of
/// them ends a page early with a cursor to continue from.
pub(crate) const MAX_CLOSED_SCAN: usize = 20_000;

/// Which closed connections a listing selects: closed at or after `since_ms`, satisfying every
/// filter the way a report does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClosedSelection {
    pub since_ms: Option<i64>,
    pub filters: Vec<TrafficFilter>,
}

impl ClosedSelection {
    pub fn matches(&self, conn: &ClosedConnection) -> bool {
        self.since_ms.is_none_or(|since| conn.closed_at >= since)
            && self
                .filters
                .iter()
                .all(|f| group_key(&conn.dimensions, f.dimension) == f.value)
    }

    /// Whether no connection closed before `closed_at` can match any more.
    pub fn exhausted_at(&self, closed_at: i64) -> bool {
        self.since_ms.is_some_and(|since| closed_at < since)
    }
}

/// The store's key order for closed connections; connections closed before the epoch sort
/// first.
pub(crate) fn closed_key(closed_at: i64) -> u64 {
    u64::try_from(closed_at).unwrap_or(0)
}

impl ClosedConnection {
    pub(crate) fn cursor(&self) -> ClosedCursor {
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

/// Adds the closed connections that are not stored yet and that `selection` picks to a stored
/// page of the same selection, keeping its newest-first order and its cursor, so paging never
/// skips or repeats a connection when a flush moves them into the store between pages.
pub fn merge_closed_page(
    stored: ClosedPage,
    pending: &[ClosedConnection],
    before: Option<&ClosedCursor>,
    limit: usize,
    selection: &ClosedSelection,
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
            .filter(|c| {
                before.is_none_or(|b| c.key() < b)
                    && floor.is_none_or(|f| c.key() > f)
                    && selection.matches(c)
            })
            .cloned(),
    );
    connections.sort_unstable_by(|a, b| b.key().cmp(&a.key()));
    // A stored page that hit the scan budget can be empty yet continue, so its own cursor carries
    // on unless pending connections push its entries off this page.
    let next = if connections.len() > limit {
        connections.truncate(limit);
        connections.last().map(ClosedConnection::cursor)
    } else {
        stored.next
    };
    ClosedPage { connections, next }
}

/// Exclusive position for heaviest-first paging of grouped usage: the last group of a page, with
/// its whole usage, so it places the next page under either metric.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageCursor {
    pub usage: Usage,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficSummary {
    pub last_sample_at: Option<i64>,
    pub active_connections: u64,
    /// Closed connections within the retention, including those not flushed yet.
    pub closed_connections: u64,
    pub current_rate: Option<Rate>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrafficError {
    #[error("traffic storage: {0}")]
    Storage(String),
    #[error("invalid traffic request: {0}")]
    InvalidRequest(String),
}

pub type TrafficResult<T> = Result<T, TrafficError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(chains: &[&str]) -> Dimensions {
        Dimensions {
            process: "/usr/bin/curl".into(),
            source: "192.168.1.2".into(),
            inbound: "mixed".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "DomainSuffix".into(),
                payload: "example.com".into(),
            },
            chains: chains.iter().map(|c| (*c).to_owned()).collect(),
            profile: Some("p1".into()),
            source_region: "CN".into(),
            destination_region: "US".into(),
            destination_basis: Some(GeoBasis::Dialed),
        }
    }

    #[test]
    fn group_keys() {
        let d = dims(&["Node-A", "Auto", "Proxy"]);
        let key = |g| group_key(&d, g);
        assert_eq!(key(Dimension::Origin), "/usr/bin/curl");
        assert_eq!(key(Dimension::Process), "/usr/bin/curl");
        assert_eq!(key(Dimension::Source), "192.168.1.2");
        assert_eq!(key(Dimension::Inbound), "mixed");
        assert_eq!(key(Dimension::Target), "example.com");
        assert_eq!(key(Dimension::Protocol), "tcp");
        assert_eq!(key(Dimension::Rule), "DomainSuffix,example.com");
        assert_eq!(key(Dimension::Chain), "Proxy → Auto");
        assert_eq!(key(Dimension::Exit), "Node-A");
        assert_eq!(key(Dimension::Profile), "p1");
        assert_eq!(key(Dimension::SourceRegion), "CN");
        assert_eq!(key(Dimension::DestinationRegion), "US");
        assert_eq!(key(Dimension::DestinationBasis), "dialed");
    }

    #[test]
    fn group_keys_with_empty_parts() {
        let mut d = dims(&["DIRECT"]);
        d.rule.payload.clear();
        d.profile = None;
        assert_eq!(group_key(&d, Dimension::Rule), "DomainSuffix");
        assert_eq!(group_key(&d, Dimension::Chain), "");
        assert_eq!(group_key(&d, Dimension::Exit), "DIRECT");
        assert_eq!(group_key(&d, Dimension::Profile), "");

        d.chains.clear();
        assert_eq!(group_key(&d, Dimension::Exit), "unknown");
        assert_eq!(group_key(&d, Dimension::Chain), "");
    }

    #[test]
    fn origin_falls_back_to_the_source() {
        let mut d = dims(&["Node-A"]);
        d.process = "unknown".into();
        assert_eq!(group_key(&d, Dimension::Origin), "192.168.1.2");
        assert_eq!(group_key(&d, Dimension::Process), "unknown");
    }

    #[test]
    fn every_dimension_has_a_distinct_name() {
        let names: std::collections::HashSet<_> = Dimension::ALL.iter().map(|d| d.name()).collect();
        assert_eq!(names.len(), Dimension::ALL.len());
    }

    #[test]
    fn dimensions_stored_before_the_new_fields_still_load() {
        let d: Dimensions = serde_json::from_str(
            r#"{"process":"curl","source":"1.1.1.1","target":"example.com","protocol":"tcp",
                "rule":{"kind":"Match","payload":""},"chains":["DIRECT"]}"#,
        )
        .unwrap();
        assert_eq!(d.inbound, "unknown");
        assert_eq!(d.profile, None);
        assert_eq!(d.source_region, "unknown");
        assert_eq!(d.destination_region, "unknown");
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

    fn everything() -> ClosedSelection {
        ClosedSelection::default()
    }

    fn ids(page: &ClosedPage) -> Vec<&str> {
        page.connections.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn pending_closed_connections_lead_a_complete_stored_page() {
        let stored = page(vec![closed("b", 20), closed("a", 10)], None);
        let merged = merge_closed_page(stored, &[closed("c", 30)], None, 10, &everything());
        assert_eq!(ids(&merged), ["c", "b", "a"]);
        assert_eq!(merged.next, None);
    }

    #[test]
    fn a_truncated_merge_continues_after_its_last_entry() {
        let stored = page(vec![closed("b", 20), closed("a", 10)], None);
        let merged = merge_closed_page(stored, &[closed("c", 30)], None, 2, &everything());
        assert_eq!(ids(&merged), ["c", "b"]);
        assert_eq!(merged.next, Some(closed("b", 20).cursor()));
    }

    #[test]
    fn pending_older_than_a_partial_stored_page_waits_for_a_later_page() {
        let c = closed("c", 30);
        let stored = page(vec![closed("d", 40), c.clone()], Some(&c));
        let merged = merge_closed_page(
            stored,
            &[closed("e", 50), closed("a", 10)],
            None,
            10,
            &everything(),
        );
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
            &everything(),
        );
        assert_eq!(ids(&merged), ["a"]);
        assert_eq!(merged.next, None);
    }

    #[test]
    fn merged_ties_on_closed_at_fall_back_to_the_id() {
        let stored = page(vec![closed("b", 20)], None);
        let merged = merge_closed_page(
            stored,
            &[closed("c", 20), closed("a", 20)],
            None,
            10,
            &everything(),
        );
        assert_eq!(ids(&merged), ["c", "b", "a"]);
    }

    #[test]
    fn merged_pages_clamp_the_limit() {
        let pending: Vec<_> = (0..MAX_CLOSED_PAGE + 1)
            .map(|i| closed(&format!("{i:04}"), 10))
            .collect();
        let merged = merge_closed_page(
            page(Vec::new(), None),
            &pending,
            None,
            usize::MAX,
            &everything(),
        );
        assert_eq!(merged.connections.len(), MAX_CLOSED_PAGE);
        assert!(merged.next.is_some());

        let merged = merge_closed_page(page(Vec::new(), None), &pending, None, 0, &everything());
        assert_eq!(merged.connections.len(), 1);
    }

    #[test]
    fn merge_keeps_paging_after_an_empty_budget_page() {
        let k = closed("k", 10);
        let merged = merge_closed_page(
            page(Vec::new(), Some(&k)),
            &[],
            None,
            10,
            &ClosedSelection::default(),
        );
        assert!(merged.connections.is_empty());
        assert_eq!(merged.next, Some(k.cursor()));
    }

    #[test]
    fn merge_filters_pending_connections() {
        let mut wget = closed("b", 20);
        wget.dimensions.process = "wget".into();
        let selection = ClosedSelection {
            since_ms: None,
            filters: vec![TrafficFilter {
                dimension: Dimension::Process,
                value: "/usr/bin/curl".into(),
            }],
        };
        let merged = merge_closed_page(
            page(Vec::new(), None),
            &[closed("c", 30), wget, closed("a", 10)],
            None,
            10,
            &selection,
        );
        assert_eq!(ids(&merged), ["c", "a"]);
        assert_eq!(merged.next, None);

        let since = ClosedSelection {
            since_ms: Some(20),
            filters: Vec::new(),
        };
        let merged = merge_closed_page(
            page(Vec::new(), None),
            &[closed("c", 30), closed("b", 20), closed("a", 10)],
            None,
            10,
            &since,
        );
        assert_eq!(ids(&merged), ["c", "b"]);
    }

    #[test]
    fn check_filters_rejects_a_repeated_dimension() {
        let filter = |dimension, value: &str| TrafficFilter {
            dimension,
            value: value.into(),
        };
        assert!(check_filters(&[]).is_ok());
        assert!(
            check_filters(&[
                filter(Dimension::Process, "a"),
                filter(Dimension::Rule, "b")
            ])
            .is_ok()
        );
        let error = check_filters(&[
            filter(Dimension::Process, "a"),
            filter(Dimension::Rule, "b"),
            filter(Dimension::Process, "c"),
        ])
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid traffic request: the filter 3 repeats Process"
        );
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
