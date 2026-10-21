//! Pure aggregation of usage rows. The rows come from the store, the pending buffer and the
//! live connections; this module only filters, groups and ranks them.
use std::{cmp::Ordering, collections::HashMap};

use crate::key::Key;

use serde::{Deserialize, Serialize};

use crate::{
    model::{
        Bytes, Dimension, Dimensions, Metric, Rate, TrafficError, TrafficFilter, TrafficQuery,
        TrafficResult, TrafficScope, UsageCursor,
    },
    topology::{self, Topology},
};

/// Upper bound of the groups on one ranking or page.
pub const MAX_LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Usage {
    pub bytes: Bytes,
    pub connections: u64,
}

impl Usage {
    pub fn saturating_add(self, other: Usage) -> Usage {
        Usage {
            bytes: self.bytes.saturating_add(other.bytes),
            connections: self.connections.saturating_add(other.connections),
        }
    }
}

impl Metric {
    /// The amount of `usage` this metric weighs.
    pub fn of(self, usage: &Usage) -> u128 {
        match self {
            Metric::Bytes => usage.bytes.total(),
            Metric::Connections => u128::from(usage.connections),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyRequest {
    /// Two to five distinct dimensions, from the first column to the last.
    pub layers: Vec<Dimension>,
    /// Nodes beyond this many per layer merge into one "other" node; `None` keeps them all.
    pub limit_per_layer: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ReportRequest {
    pub query: TrafficQuery,
    /// What the rankings and the topology order and merge their groups by.
    pub metric: Metric,
    /// One ranking per dimension.
    pub rankings: Vec<Dimension>,
    /// Groups per ranking, capped at `MAX_LIMIT`.
    pub ranking_limit: usize,
    pub topology: Option<TopologyRequest>,
}

impl ReportRequest {
    /// Rejects a query that repeats a filter dimension and a topology that does not have two to
    /// five distinct layers, and caps the group counts at `MAX_LIMIT`.
    pub fn checked(mut self) -> TrafficResult<Self> {
        self.query.check()?;
        self.ranking_limit = self.ranking_limit.clamp(1, MAX_LIMIT);
        if let Some(topology) = &mut self.topology {
            let layers = &topology.layers;
            if !(2..=5).contains(&layers.len()) {
                return Err(TrafficError::InvalidRequest(format!(
                    "a topology has 2 to 5 layers, not {}",
                    layers.len()
                )));
            }
            if let Some(i) = (1..layers.len()).find(|&i| layers[..i].contains(&layers[i])) {
                return Err(TrafficError::InvalidRequest(format!(
                    "the topology layer {} repeats {:?}",
                    i + 1,
                    layers[i]
                )));
            }
            topology.limit_per_layer = topology.limit_per_layer.map(|n| n.clamp(1, MAX_LIMIT));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageGroup {
    pub key: String,
    pub usage: Usage,
    pub current_rate: Option<Rate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Ranking {
    pub dimension: Dimension,
    /// Distinct values of the dimension among the rows the ranking counts: those matching every
    /// filter except one on this dimension.
    pub distinct: u64,
    /// Heaviest first.
    pub groups: Vec<UsageGroup>,
    /// The groups ranked after `groups`.
    pub other: Usage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficReport {
    pub total: Usage,
    pub current_rate: Option<Rate>,
    pub rankings: Vec<Ranking>,
    pub topology: Option<Topology>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsagePage {
    pub total: Usage,
    pub groups: Vec<UsageGroup>,
    /// Groups ranked after this page.
    pub other: Usage,
    pub next: Option<UsageCursor>,
}

/// One dimension combination with its usage and, for live connections, its rate.
pub type Row<'a> = (&'a Dimensions, Usage, Option<Rate>);

/// Which filters a row fails.
pub(crate) enum Miss {
    None,
    /// Only the filter at this index.
    Only(usize),
    Many,
}
impl Miss {
    pub fn of(d: &Dimensions, filters: &[TrafficFilter]) -> Self {
        let mut miss = Miss::None;
        for (i, f) in filters.iter().enumerate() {
            if Key::of(d, f.dimension) != Key::Text(&f.value) {
                if !matches!(miss, Miss::None) {
                    return Miss::Many;
                }
                miss = Miss::Only(i);
            }
        }
        miss
    }
}

pub fn filter_rows<'a, I>(
    rows: I,
    filters: &[TrafficFilter],
) -> impl Iterator<Item = Row<'a>> + Clone
where
    I: IntoIterator<Item = Row<'a>>,
    I::IntoIter: Clone,
{
    rows.into_iter()
        .filter(move |(d, _, _)| crate::key::matches(d, filters))
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Aggregate {
    pub usage: Usage,
    pub rate: Option<Rate>,
}
impl Aggregate {
    pub fn add(&mut self, usage: Usage, rate: Option<Rate>) {
        self.usage = self.usage.saturating_add(usage);
        if let Some(rate) = rate {
            self.rate = Some(add_rate(self.rate, rate));
        }
    }
    fn output(self, key: Key<'_>) -> UsageGroup {
        UsageGroup {
            key: key.owned(),
            usage: self.usage,
            current_rate: self.rate,
        }
    }
}

pub(crate) struct Groups<'a> {
    pub values: HashMap<Key<'a>, Aggregate>,
    pub overflow: Usage,
    pub has_overflow: bool,
}
impl<'a> Groups<'a> {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: HashMap::with_capacity(capacity),
            overflow: Usage::default(),
            has_overflow: false,
        }
    }
    pub fn add(&mut self, key: Option<Key<'a>>, usage: Usage, rate: Option<Rate>) {
        if let Some(key) = key {
            self.values.entry(key).or_default().add(usage, rate);
        } else {
            self.has_overflow = true;
            self.overflow = self.overflow.saturating_add(usage);
        }
    }
    pub fn ordered(&self, metric: Metric) -> Vec<(Key<'a>, Aggregate)> {
        let mut groups: Vec<_> = self.values.iter().map(|(k, v)| (*k, *v)).collect();
        groups.sort_unstable_by(|(ak, a), (bk, b)| {
            metric
                .of(&b.usage)
                .cmp(&metric.of(&a.usage))
                .then_with(|| ak.cmp(bk))
        });
        groups
    }
    pub fn ranking(&self, dimension: Dimension, metric: Metric, limit: usize) -> Ranking {
        let groups = self.ordered(metric);
        let limit = limit.clamp(1, MAX_LIMIT).min(groups.len());
        Ranking {
            dimension,
            distinct: groups.len() as u64,
            groups: groups[..limit].iter().map(|(k, v)| v.output(*k)).collect(),
            other: sum(groups[limit..].iter().map(|(_, v)| &v.usage)).saturating_add(self.overflow),
        }
    }
    pub fn page(
        &self,
        total: Usage,
        metric: Metric,
        after: Option<&UsageCursor>,
        limit: usize,
    ) -> UsagePage {
        let mut groups = self.ordered(metric);
        groups.retain(|(key, value)| {
            after.is_none_or(|c| {
                rank(metric, (*key, &value.usage), (Key::Text(&c.key), &c.usage))
                    == Ordering::Greater
            })
        });
        let limit = limit.clamp(1, MAX_LIMIT).min(groups.len());
        let next = (groups.len() > limit).then(|| UsageCursor {
            key: groups[limit - 1].0.owned(),
            usage: groups[limit - 1].1.usage,
        });
        UsagePage {
            total,
            groups: groups[..limit].iter().map(|(k, v)| v.output(*k)).collect(),
            other: sum(groups[limit..].iter().map(|(_, v)| &v.usage)).saturating_add(self.overflow),
            next,
        }
    }
    pub fn by_keys(self, keys: &[String]) -> Vec<UsageGroup> {
        let mut out = Vec::with_capacity(keys.len().min(self.values.len()));
        for (i, key) in keys.iter().enumerate() {
            if keys[..i].contains(key) {
                continue;
            }
            if let Some(value) = self.values.get(&Key::Text(key)) {
                out.push(value.output(Key::Text(key)));
            }
        }
        out
    }
}

/// One grouping of a report. A topology layer of Chain skips the empty chain. A ranking of a
/// filtered dimension ignores that filter, the one at index `ignored`, so the filtered value
/// keeps its alternatives beside it; the total and the topology honour every filter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grouping {
    pub dimension: Dimension,
    pub skip_empty: bool,
    pub ignored: Option<usize>,
}

/// Shared requests of one report.
pub(crate) struct ReportPlan {
    pub dimensions: Vec<Grouping>,
    pub rankings: Vec<usize>,
    pub layers: Vec<usize>,
}
impl ReportPlan {
    pub fn new(request: &ReportRequest) -> Self {
        let mut out = Self {
            dimensions: Vec::with_capacity(request.rankings.len() + 5),
            rankings: Vec::with_capacity(request.rankings.len()),
            layers: Vec::with_capacity(5),
        };
        let filters = &request.query.filters;
        for &d in &request.rankings {
            let i = out.slot(Grouping {
                dimension: d,
                skip_empty: false,
                ignored: filters.iter().position(|f| f.dimension == d),
            });
            out.rankings.push(i);
        }
        if let Some(t) = &request.topology {
            for &d in &t.layers {
                let i = out.slot(Grouping {
                    dimension: d,
                    skip_empty: d == Dimension::Chain,
                    ignored: None,
                });
                out.layers.push(i);
            }
        }
        out
    }
    fn slot(&mut self, grouping: Grouping) -> usize {
        if let Some(i) = self.dimensions.iter().position(|g| *g == grouping) {
            i
        } else {
            self.dimensions.push(grouping);
            self.dimensions.len() - 1
        }
    }
}

pub(crate) struct ReportState<'a> {
    pub total: Aggregate,
    pub groups: Vec<Groups<'a>>,
}
impl<'a> ReportState<'a> {
    pub fn new(capacities: impl Iterator<Item = usize>) -> Self {
        Self {
            total: Aggregate::default(),
            groups: capacities.map(Groups::new).collect(),
        }
    }
    /// Adds an unfiltered row: to the total and every grouping when it matches every filter, or
    /// to the groupings that ignore the one filter it fails.
    pub fn add(&mut self, row: Row<'a>, plan: &ReportPlan, filters: &[TrafficFilter], rated: bool) {
        let (d, usage, rate) = row;
        let miss = Miss::of(d, filters);
        let ignored = match miss {
            Miss::None => None,
            Miss::Only(i) => Some(i),
            Miss::Many => return,
        };
        let rate = rate.filter(|_| rated);
        if ignored.is_none() {
            self.total.add(usage, rate);
        }
        for (grouping, groups) in plan.dimensions.iter().zip(&mut self.groups) {
            if ignored.is_some() && grouping.ignored != ignored {
                continue;
            }
            let key = Key::of(d, grouping.dimension);
            if !grouping.skip_empty || !key.is_empty() {
                groups.add(Some(key), usage, rate);
            }
        }
    }
    pub fn finish(
        self,
        plan: &ReportPlan,
        request: &ReportRequest,
        topology: Option<Topology>,
    ) -> TrafficReport {
        TrafficReport {
            total: self.total.usage,
            current_rate: self.total.rate,
            rankings: request
                .rankings
                .iter()
                .zip(&plan.rankings)
                .map(|(&d, &i)| self.groups[i].ranking(d, request.metric, request.ranking_limit))
                .collect(),
            topology,
        }
    }
}

pub fn report<'a, I>(rows: I, request: &ReportRequest) -> TrafficReport
where
    I: IntoIterator<Item = Row<'a>>,
    I::IntoIter: Clone,
{
    let rows = rows.into_iter();
    let filters = &request.query.filters;
    let plan = ReportPlan::new(request);
    let mut state = ReportState::new(
        plan.dimensions
            .iter()
            .map(|_| rows.size_hint().1.unwrap_or(0)),
    );
    for row in rows.clone() {
        state.add(
            row,
            &plan,
            filters,
            request.query.scope != TrafficScope::Closed,
        );
    }
    let rows = filter_rows(rows, filters);
    let topology = request.topology.as_ref().map(|t| {
        let mut projection = topology::Projection::new(
            plan.layers.iter().map(|&i| &state.groups[i]),
            request.metric,
            t.limit_per_layer,
        );
        for (d, u, _) in rows {
            projection.session(d, u, &t.layers);
        }
        projection.finish()
    });
    state.finish(&plan, request, topology)
}

pub fn usage_page<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    dimension: Dimension,
    metric: Metric,
    after: Option<&UsageCursor>,
    limit: usize,
) -> UsagePage {
    let rows = rows.into_iter();
    let mut groups = Groups::new(rows.size_hint().1.unwrap_or(0));
    let mut total = Usage::default();
    for (d, u, r) in rows {
        total = total.saturating_add(u);
        groups.add(Some(Key::of(d, dimension)), u, r);
    }
    groups.page(total, metric, after, limit)
}

pub fn usage_by_keys<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    dimension: Dimension,
    keys: &[String],
) -> Vec<UsageGroup> {
    let rows = rows.into_iter();
    let mut groups = Groups::new(rows.size_hint().1.unwrap_or(0));
    for (d, u, r) in rows {
        groups.add(Some(Key::of(d, dimension)), u, r);
    }
    groups.by_keys(keys)
}

fn rank(metric: Metric, (ak, a): (Key<'_>, &Usage), (bk, b): (Key<'_>, &Usage)) -> Ordering {
    metric.of(b).cmp(&metric.of(a)).then_with(|| ak.cmp(&bk))
}
fn sum<'a>(usages: impl IntoIterator<Item = &'a Usage>) -> Usage {
    usages
        .into_iter()
        .fold(Usage::default(), |sum, u| sum.saturating_add(*u))
}
fn add_rate(sum: Option<Rate>, rate: Rate) -> Rate {
    let sum = sum.unwrap_or_default();
    Rate {
        upload: sum.upload.saturating_add(rate.upload),
        download: sum.download.saturating_add(rate.download),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RuleKey, TrafficRange};

    fn dims(process: &str, target: &str, exit: &str, profile: &str) -> Dimensions {
        Dimensions {
            process: process.into(),
            source: "192.168.1.2".into(),
            inbound: "mixed".into(),
            target: target.into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "Match".into(),
                payload: String::new(),
            },
            chains: vec![exit.into(), "Proxy".into()],
            profile: Some(profile.into()),
            source_region: "CN".into(),
            destination_region: "US".into(),
            destination_basis: None,
        }
    }

    fn usage(up: u64, down: u64, connections: u64) -> Usage {
        Usage {
            bytes: Bytes {
                upload: up,
                download: down,
            },
            connections,
        }
    }

    fn query(filters: &[(Dimension, &str)], scope: TrafficScope) -> TrafficQuery {
        TrafficQuery {
            range: TrafficRange::All,
            scope,
            filters: filters
                .iter()
                .map(|(dimension, value)| TrafficFilter {
                    dimension: *dimension,
                    value: (*value).into(),
                })
                .collect(),
        }
    }

    fn request(query: TrafficQuery, rankings: &[Dimension], limit: usize) -> ReportRequest {
        ReportRequest {
            query,
            metric: Metric::Bytes,
            rankings: rankings.to_vec(),
            ranking_limit: limit,
            topology: None,
        }
    }

    /// curl: 30 B over 2 conns, firefox: 12 B over 1, wget: 3 B over 1.
    struct Fixture(Vec<(Dimensions, Usage, Option<Rate>)>);

    impl Fixture {
        fn new() -> Self {
            Self(vec![
                (
                    dims("curl", "a.com", "Node-A", "p1"),
                    usage(10, 20, 1),
                    None,
                ),
                (dims("curl", "b.com", "Node-B", "p1"), usage(0, 0, 1), None),
                (
                    dims("firefox", "a.com", "Node-A", "p2"),
                    usage(4, 8, 1),
                    None,
                ),
                (dims("wget", "c.com", "Node-B", "p2"), usage(1, 2, 1), None),
            ])
        }

        fn rows(&self) -> impl Iterator<Item = Row<'_>> + Clone {
            self.0.iter().map(|(d, u, r)| (d, *u, *r))
        }
    }

    fn keys(groups: &[UsageGroup]) -> Vec<&str> {
        groups.iter().map(|g| g.key.as_str()).collect()
    }

    #[test]
    fn filters_combine_with_and() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(
                    &[(Dimension::Exit, "Node-A"), (Dimension::Profile, "p1")],
                    TrafficScope::All,
                ),
                &[Dimension::Process],
                10,
            ),
        );
        assert_eq!(r.total, usage(10, 20, 1));
        assert_eq!(keys(&r.rankings[0].groups), ["curl"]);
    }

    #[test]
    fn a_ranking_ignores_the_filter_on_its_own_dimension() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(&[(Dimension::Process, "firefox")], TrafficScope::All),
                &[Dimension::Process, Dimension::Target],
                10,
            ),
        );
        assert_eq!(r.total, usage(4, 8, 1));
        let process = &r.rankings[0];
        assert_eq!(keys(&process.groups), ["curl", "firefox", "wget"]);
        assert_eq!(process.distinct, 3);
        // The filtered value's group is the total.
        assert_eq!(process.groups[1].usage, r.total);
        assert_eq!(keys(&r.rankings[1].groups), ["a.com"]);
    }

    #[test]
    fn a_ranking_keeps_the_filters_on_other_dimensions() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(
                    &[(Dimension::Process, "curl"), (Dimension::Exit, "Node-B")],
                    TrafficScope::All,
                ),
                &[Dimension::Process, Dimension::Exit, Dimension::Target],
                10,
            ),
        );
        assert_eq!(r.total, usage(0, 0, 1));
        // Node-B carries curl and wget; curl reaches Node-A and Node-B.
        assert_eq!(keys(&r.rankings[0].groups), ["wget", "curl"]);
        assert_eq!(keys(&r.rankings[1].groups), ["Node-A", "Node-B"]);
        assert_eq!(keys(&r.rankings[2].groups), ["b.com"]);
    }

    #[test]
    fn the_topology_honours_every_filter() {
        let f = Fixture::new();
        let mut req = request(
            query(&[(Dimension::Process, "firefox")], TrafficScope::All),
            &[Dimension::Process],
            10,
        );
        req.topology = Some(TopologyRequest {
            layers: vec![Dimension::Process, Dimension::Exit],
            limit_per_layer: None,
        });
        let r = report(f.rows(), &req);
        assert_eq!(r.rankings[0].distinct, 3);
        let topology = r.topology.unwrap();
        let keys: Vec<_> = topology.nodes.iter().map(|n| n.key.as_deref()).collect();
        assert_eq!(keys, [Some("firefox"), Some("Node-A")]);
    }

    #[test]
    fn the_empty_key_can_be_filtered_on() {
        let mut f = Fixture::new();
        f.0[0].0.chains = vec!["DIRECT".into()];
        let r = report(
            f.rows(),
            &request(
                query(&[(Dimension::Chain, "")], TrafficScope::All),
                &[Dimension::Exit],
                10,
            ),
        );
        assert_eq!(keys(&r.rankings[0].groups), ["DIRECT"]);
    }

    #[test]
    fn two_values_on_one_dimension_match_nothing() {
        // The caller replaces a dimension's value; two at once are ANDed and exclude each other.
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(
                    &[(Dimension::Process, "curl"), (Dimension::Process, "wget")],
                    TrafficScope::All,
                ),
                &[],
                10,
            ),
        );
        assert_eq!(r.total, Usage::default());
    }

    #[test]
    fn rankings_sort_by_bytes_then_key_and_count_distinct_values() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(&[], TrafficScope::All),
                &[Dimension::Process, Dimension::Exit],
                10,
            ),
        );
        assert_eq!(r.total, usage(15, 30, 4));
        let process = &r.rankings[0];
        assert_eq!(process.distinct, 3);
        assert_eq!(keys(&process.groups), ["curl", "firefox", "wget"]);
        assert_eq!(process.groups[0].usage, usage(10, 20, 2));
        assert_eq!(process.other, Usage::default());
        // Node-A: 42 B, Node-B: 3 B.
        assert_eq!(keys(&r.rankings[1].groups), ["Node-A", "Node-B"]);
    }

    #[test]
    fn equal_bytes_rank_by_key() {
        let rows = [
            (dims("b", "x", "E", "p"), usage(1, 1, 1), None),
            (dims("a", "x", "E", "p"), usage(1, 1, 1), None),
        ];
        let r = report(
            rows.iter().map(|(d, u, r)| (d, *u, *r)),
            &request(query(&[], TrafficScope::All), &[Dimension::Process], 10),
        );
        assert_eq!(keys(&r.rankings[0].groups), ["a", "b"]);
    }

    #[test]
    fn rankings_and_pages_follow_the_metric() {
        // a.com: 42 B over 2 conns, b.com: 0 B over 1, c.com: 3 B over 1.
        let f = Fixture::new();
        let ranking = |metric| {
            let mut req = request(query(&[], TrafficScope::All), &[Dimension::Target], 10);
            req.metric = metric;
            report(f.rows(), &req).rankings.remove(0).groups
        };
        assert_eq!(keys(&ranking(Metric::Bytes)), ["a.com", "c.com", "b.com"]);
        assert_eq!(
            keys(&ranking(Metric::Connections)),
            ["a.com", "b.com", "c.com"]
        );

        let first = usage_page(f.rows(), Dimension::Target, Metric::Connections, None, 2);
        assert_eq!(keys(&first.groups), ["a.com", "b.com"]);
        let second = usage_page(
            f.rows(),
            Dimension::Target,
            Metric::Connections,
            first.next.as_ref(),
            2,
        );
        assert_eq!(keys(&second.groups), ["c.com"]);
        assert_eq!(second.next, None);
    }

    #[test]
    fn distinct_counts_only_the_filtered_values() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(
                query(&[(Dimension::Profile, "p2")], TrafficScope::All),
                &[Dimension::Process, Dimension::Target],
                10,
            ),
        );
        assert_eq!(r.rankings[0].distinct, 2);
        assert_eq!(r.rankings[1].distinct, 2);
    }

    #[test]
    fn other_is_the_sum_of_the_groups_beyond_the_limit() {
        let f = Fixture::new();
        let r = report(
            f.rows(),
            &request(query(&[], TrafficScope::All), &[Dimension::Process], 1),
        );
        let ranking = &r.rankings[0];
        assert_eq!(keys(&ranking.groups), ["curl"]);
        assert_eq!(ranking.other, usage(5, 10, 2));
        assert_eq!(
            ranking.groups[0].usage.saturating_add(ranking.other),
            r.total
        );
        assert_eq!(ranking.distinct, 3);
    }

    #[test]
    fn rates_are_summed_per_group_and_absent_for_closed_scope() {
        let rate = |u, d| {
            Some(Rate {
                upload: u,
                download: d,
            })
        };
        let rows = [
            (dims("curl", "a", "E", "p"), usage(1, 1, 1), rate(5, 10)),
            (dims("curl", "b", "E", "p"), usage(1, 1, 1), rate(1, 2)),
            (dims("wget", "c", "E", "p"), usage(1, 1, 1), None),
        ];
        let rows = || rows.iter().map(|(d, u, r)| (d, *u, *r));

        let active = report(
            rows(),
            &request(query(&[], TrafficScope::Active), &[Dimension::Process], 10),
        );
        assert_eq!(active.current_rate, rate(6, 12));
        assert_eq!(active.rankings[0].groups[0].current_rate, rate(6, 12));
        assert_eq!(active.rankings[0].groups[1].current_rate, None);

        let closed = report(
            rows(),
            &request(query(&[], TrafficScope::Closed), &[Dimension::Process], 10),
        );
        assert_eq!(closed.current_rate, None);
        assert_eq!(closed.rankings[0].groups[0].current_rate, None);
    }

    #[test]
    fn an_empty_input_reports_nothing() {
        let r = report(
            std::iter::empty(),
            &request(query(&[], TrafficScope::All), &[Dimension::Process], 10),
        );
        assert_eq!(r.total, Usage::default());
        assert_eq!(r.current_rate, None);
        assert_eq!(r.rankings[0].distinct, 0);
        assert!(r.rankings[0].groups.is_empty());
    }

    #[test]
    fn the_ranking_limit_is_clamped() {
        let rows: Vec<_> = (0..MAX_LIMIT + 5)
            .map(|i| {
                (
                    dims(&format!("p{i:03}"), "x", "E", "p"),
                    usage(1, 0, 1),
                    None,
                )
            })
            .collect();
        let rows = || rows.iter().map(|(d, u, r)| (d, *u, *r));
        let ask = |limit| {
            report(
                rows(),
                &request(query(&[], TrafficScope::All), &[Dimension::Process], limit),
            )
        };
        assert_eq!(ask(usize::MAX).rankings[0].groups.len(), MAX_LIMIT);
        assert_eq!(ask(0).rankings[0].groups.len(), 1);
    }

    #[test]
    fn pages_continue_after_the_cursor() {
        let f = Fixture::new();
        let first = usage_page(f.rows(), Dimension::Process, Metric::Bytes, None, 2);
        assert_eq!(keys(&first.groups), ["curl", "firefox"]);
        assert_eq!(first.total, usage(15, 30, 4));
        assert_eq!(first.other, usage(1, 2, 1));
        let cursor = first.next.unwrap();
        assert_eq!(cursor.key, "firefox");

        let second = usage_page(
            f.rows(),
            Dimension::Process,
            Metric::Bytes,
            Some(&cursor),
            2,
        );
        assert_eq!(keys(&second.groups), ["wget"]);
        assert_eq!(second.other, Usage::default());
        assert_eq!(second.next, None);
        assert_eq!(second.total, first.total);
    }

    #[test]
    fn a_cursor_survives_its_group_changing() {
        // The cursor's own group may have grown or vanished; paging goes by rank position.
        let f = Fixture::new();
        let cursor = UsageCursor {
            usage: usage(100, 0, 9),
            key: "gone".into(),
        };
        let page = usage_page(
            f.rows(),
            Dimension::Process,
            Metric::Bytes,
            Some(&cursor),
            10,
        );
        assert_eq!(keys(&page.groups), ["curl", "firefox", "wget"]);
    }

    #[test]
    fn by_keys_keeps_request_order_and_omits_missing_keys() {
        let f = Fixture::new();
        let keys = ["wget", "nope", "curl", "wget"].map(String::from);
        let groups = usage_by_keys(f.rows(), Dimension::Process, &keys);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.key.as_str(), g.usage))
                .collect::<Vec<_>>(),
            [("wget", usage(1, 2, 1)), ("curl", usage(10, 20, 2))]
        );
    }

    #[test]
    fn a_group_with_connections_but_no_bytes_still_exists() {
        let f = Fixture::new();
        let keys = ["b.com".to_owned()];
        let groups = usage_by_keys(f.rows(), Dimension::Target, &keys);
        assert_eq!(groups[0].usage, usage(0, 0, 1));
    }

    #[test]
    fn the_report_carries_a_topology_over_the_filtered_rows() {
        let f = Fixture::new();
        let mut req = request(
            query(&[(Dimension::Profile, "p1")], TrafficScope::All),
            &[],
            10,
        );
        req.topology = Some(TopologyRequest {
            layers: vec![Dimension::Origin, Dimension::Exit],
            limit_per_layer: None,
        });
        let topology = report(f.rows(), &req).topology.unwrap();
        assert_eq!(topology.nodes.len(), 3);
        assert_eq!(topology.edges.len(), 2);
    }

    #[test]
    fn a_query_filters_each_dimension_at_most_once() {
        use Dimension::*;
        let checked = |filters: &[(Dimension, &str)]| query(filters, TrafficScope::All).check();

        assert!(checked(&[]).is_ok());
        assert!(checked(&[(Process, "curl"), (Target, "a.com")]).is_ok());
        for filters in [
            &[(Process, "curl"), (Process, "wget")][..],
            &[(Process, "curl"), (Target, "a.com"), (Process, "curl")],
        ] {
            assert!(matches!(
                checked(filters),
                Err(TrafficError::InvalidRequest(_))
            ));
            assert!(matches!(
                request(query(filters, TrafficScope::All), &[], 10).checked(),
                Err(TrafficError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn a_checked_request_needs_two_to_five_distinct_layers() {
        use Dimension::*;
        let with_layers = |layers: &[Dimension], limit| {
            let mut req = request(query(&[], TrafficScope::All), &[], usize::MAX);
            req.topology = Some(TopologyRequest {
                layers: layers.to_vec(),
                limit_per_layer: limit,
            });
            req.checked()
        };

        for layers in [&[Origin][..], &[Origin, Rule, Chain, Exit, Target, Inbound]] {
            assert!(matches!(
                with_layers(layers, None),
                Err(TrafficError::InvalidRequest(_))
            ));
        }
        assert!(matches!(
            with_layers(&[Origin, Exit, Origin], None),
            Err(TrafficError::InvalidRequest(_))
        ));

        let checked = with_layers(&[Origin, Rule, Chain, Exit, Target], Some(usize::MAX)).unwrap();
        assert_eq!(checked.ranking_limit, MAX_LIMIT);
        assert_eq!(checked.topology.unwrap().limit_per_layer, Some(MAX_LIMIT));
        assert_eq!(
            with_layers(&[Origin, Exit], Some(0))
                .unwrap()
                .topology
                .unwrap()
                .limit_per_layer,
            Some(1)
        );
        assert!(
            with_layers(&[Origin, Exit], None)
                .unwrap()
                .topology
                .unwrap()
                .limit_per_layer
                .is_none()
        );
    }
}
