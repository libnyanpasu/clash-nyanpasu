//! Pure aggregation of usage rows. The rows come from the store, the pending buffer and the
//! live connections; this module only filters, groups and ranks them.
use std::{cmp::Ordering, collections::HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    model::{
        Bytes, Dimension, Dimensions, Metric, Rate, TrafficError, TrafficFilter, TrafficQuery,
        TrafficResult, TrafficScope, UsageCursor, group_key,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyRequest {
    /// Two to five distinct dimensions, from the first column to the last.
    pub layers: Vec<Dimension>,
    pub metric: Metric,
    /// Nodes beyond this many per layer merge into one "other" node; `None` keeps them all.
    pub limit_per_layer: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ReportRequest {
    pub query: TrafficQuery,
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
    /// Distinct values of the dimension among the filtered rows.
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

/// The rows that satisfy every filter.
pub fn filter_rows<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    filters: &[TrafficFilter],
) -> impl Iterator<Item = Row<'a>> {
    rows.into_iter().filter(move |(dimensions, _, _)| {
        filters
            .iter()
            .all(|f| group_key(dimensions, f.dimension) == f.value)
    })
}

pub fn report<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    request: &ReportRequest,
) -> TrafficReport {
    let rows: Vec<Row<'a>> = filter_rows(rows, &request.query.filters).collect();
    // Only live connections have a rate, and a closed-only view has none of them.
    let rated = request.query.scope != TrafficScope::Closed;
    let limit = request.ranking_limit.clamp(1, MAX_LIMIT);

    TrafficReport {
        total: sum(rows.iter().map(|(_, usage, _)| usage)),
        current_rate: rated
            .then(|| sum_rates(rows.iter().map(|(_, _, rate)| rate)))
            .flatten(),
        rankings: request
            .rankings
            .iter()
            .map(|&dimension| {
                let mut groups = ranked(&rows, dimension, rated);
                let distinct = groups.len() as u64;
                let other = sum(groups
                    .split_off(limit.min(groups.len()))
                    .iter()
                    .map(|g| &g.usage));
                Ranking {
                    dimension,
                    distinct,
                    groups,
                    other,
                }
            })
            .collect(),
        topology: request.topology.as_ref().map(|t| {
            topology::project(
                rows.iter()
                    .map(|(dimensions, usage, _)| (*dimensions, *usage)),
                &t.layers,
                t.metric,
                t.limit_per_layer,
            )
        }),
    }
}

/// The `limit` groups ranked after `after`, or the top ones; the rest after the page is folded
/// into `other`. `total` covers every group.
pub fn usage_page<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    dimension: Dimension,
    after: Option<&UsageCursor>,
    limit: usize,
) -> UsagePage {
    let mut groups = ranked(&rows.into_iter().collect::<Vec<_>>(), dimension, true);
    let total = sum(groups.iter().map(|g| &g.usage));
    groups.retain(|g| {
        after
            .is_none_or(|c| rank((&g.key, &g.usage.bytes), (&c.key, &c.bytes)) == Ordering::Greater)
    });
    let rest = groups.split_off(limit.clamp(1, MAX_LIMIT).min(groups.len()));
    let next = if rest.is_empty() {
        None
    } else {
        groups.last().map(|g| UsageCursor {
            bytes: g.usage.bytes,
            key: g.key.clone(),
        })
    };
    UsagePage {
        total,
        groups,
        other: sum(rest.iter().map(|g| &g.usage)),
        next,
    }
}

/// The groups of `keys`, in request order; keys without any row are left out.
pub fn usage_by_keys<'a>(
    rows: impl IntoIterator<Item = Row<'a>>,
    dimension: Dimension,
    keys: &[String],
) -> Vec<UsageGroup> {
    let mut groups: HashMap<String, UsageGroup> =
        ranked(&rows.into_iter().collect::<Vec<_>>(), dimension, true)
            .into_iter()
            .map(|g| (g.key.clone(), g))
            .collect();
    // `remove` also drops repeated keys after their first occurrence.
    keys.iter().filter_map(|key| groups.remove(key)).collect()
}

/// Every group of `dimension`, heaviest first; equal traffic ranks by key.
fn ranked(rows: &[Row<'_>], dimension: Dimension, rated: bool) -> Vec<UsageGroup> {
    let mut groups: HashMap<String, UsageGroup> = HashMap::new();
    for (dimensions, usage, rate) in rows {
        let group = groups
            .entry(group_key(dimensions, dimension))
            .or_insert_with_key(|key| UsageGroup {
                key: key.clone(),
                usage: Usage::default(),
                current_rate: None,
            });
        group.usage = group.usage.saturating_add(*usage);
        if let Some(rate) = rate.filter(|_| rated) {
            group.current_rate = Some(add_rate(group.current_rate, rate));
        }
    }
    let mut groups: Vec<UsageGroup> = groups.into_values().collect();
    groups.sort_by(|a, b| rank((&a.key, &a.usage.bytes), (&b.key, &b.usage.bytes)));
    groups
}

/// Heaviest first; equal traffic ranks by key.
fn rank((a_key, a): (&str, &Bytes), (b_key, b): (&str, &Bytes)) -> Ordering {
    b.total().cmp(&a.total()).then_with(|| a_key.cmp(b_key))
}

fn sum<'a>(usages: impl IntoIterator<Item = &'a Usage>) -> Usage {
    usages
        .into_iter()
        .fold(Usage::default(), |sum, usage| sum.saturating_add(*usage))
}

fn sum_rates<'a>(rates: impl IntoIterator<Item = &'a Option<Rate>>) -> Option<Rate> {
    rates
        .into_iter()
        .flatten()
        .fold(None, |sum, rate| Some(add_rate(sum, *rate)))
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

        fn rows(&self) -> impl Iterator<Item = Row<'_>> {
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
        let first = usage_page(f.rows(), Dimension::Process, None, 2);
        assert_eq!(keys(&first.groups), ["curl", "firefox"]);
        assert_eq!(first.total, usage(15, 30, 4));
        assert_eq!(first.other, usage(1, 2, 1));
        let cursor = first.next.unwrap();
        assert_eq!(cursor.key, "firefox");

        let second = usage_page(f.rows(), Dimension::Process, Some(&cursor), 2);
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
            bytes: Bytes {
                upload: 100,
                download: 0,
            },
            key: "gone".into(),
        };
        let page = usage_page(f.rows(), Dimension::Process, Some(&cursor), 10);
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
            metric: Metric::Bytes,
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
                metric: Metric::Bytes,
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
