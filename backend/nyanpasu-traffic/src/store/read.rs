use std::{collections::HashMap, sync::Arc};

use nyanpasu_tsdb::{
    ErrorKind, EventCondition, EventFilter, EventPredicate, EventQuery, EventValue, FieldMask,
    GroupKey, GroupRequest, KeyPart, LabelFilter, LabelId, LabelMap, SeriesGroups, SeriesQuery,
    Snapshot, StreamRef, TimeRange, Timestamp,
    storage::{Direction, Storage},
};

use super::{TrafficDb, schema::*, storage};
use crate::{
    accounting::Session,
    key::Key,
    model::ClosedRecords,
    query::{Groups, ReportPlan, ReportState},
    topology::Projection,
    *,
};

/// Real traffic uses [start, i64::MAX) as an unbounded end; a timestamp at i64::MAX
/// milliseconds (about year 292 million) is outside the application's supported data.
fn range(start: i64) -> TimeRange {
    TimeRange::new(Timestamp(start), Timestamp(i64::MAX))
        .expect("traffic range starts before its end")
}

struct Filters {
    ids: Vec<([u32; 2], usize)>,
    dimensions: Vec<LabelId>,
    missing: bool,
}
impl Filters {
    fn new<S: Storage>(snapshot: &Snapshot<S>, filters: &[TrafficFilter]) -> TrafficResult<Self> {
        let mut out = Self {
            ids: Vec::with_capacity(filters.len()),
            dimensions: Vec::with_capacity(filters.len()),
            missing: false,
        };
        for filter in filters {
            let dimension = LabelId(filter.dimension as u16);
            let mut ids = [0; 2];
            let mut count = 0;
            let mut add = |value: &[u8]| {
                if let Some(id) = snapshot.find_label(dimension, value) {
                    ids[count] = id;
                    count += 1;
                }
            };
            if filter.dimension == Dimension::Profile {
                if filter.value.is_empty() {
                    add(&[0]);
                }
                let encoded =
                    postcard::to_allocvec(&Some(filter.value.as_str())).map_err(storage)?;
                add(&encoded);
            } else {
                add(filter.value.as_bytes());
            }
            out.missing |= count == 0;
            out.ids.push((ids, count));
            out.dimensions.push(dimension);
        }
        Ok(out)
    }
    fn filters(&self) -> Vec<LabelFilter<'_>> {
        self.dimensions
            .iter()
            .zip(&self.ids)
            .map(|(&dimension, (ids, len))| LabelFilter {
                dimension,
                ids: &ids[..*len],
                exclude: false,
            })
            .collect()
    }
}

pub(super) fn display<S: Storage>(
    snapshot: &Snapshot<S>,
    dimension: Dimension,
    id: u32,
) -> TrafficResult<Option<Key<'_>>> {
    if id == 0 {
        return Ok(None);
    }
    let bytes = snapshot
        .label(LabelId(dimension as u16), id)
        .ok_or_else(|| storage("unknown label id"))?;
    let text = if dimension == Dimension::Profile {
        postcard::from_bytes::<Option<&str>>(bytes)
            .map_err(storage)?
            .unwrap_or_default()
    } else {
        std::str::from_utf8(bytes).map_err(storage)?
    };
    Ok(Some(Key::Text(text)))
}

fn usage(sums: &[u64]) -> Usage {
    Usage {
        bytes: Bytes {
            upload: sums[0],
            download: sums[1],
        },
        connections: sums[2],
    }
}

/// The session's rows of `query`'s range and scope that satisfy `filters`.
fn rows<'a>(
    session: &'a Session,
    query: &TrafficQuery,
    filters: &'a [TrafficFilter],
    now_ms: i64,
) -> impl Iterator<Item = Row<'a>> + Clone {
    let tier = query.range.tier();
    let from = query.range.start(now_ms);
    let pending = (query.scope != TrafficScope::Active)
        .then(|| session.pending_rows(tier, from))
        .into_iter()
        .flatten();
    let active = (query.scope != TrafficScope::Closed)
        .then(|| session.active_rows(tier, from))
        .into_iter()
        .flatten();
    filter_rows(pending.chain(active), filters)
}

impl<S: Storage> TrafficDb<S> {
    fn failed(&self, kind: ErrorKind, error: impl std::fmt::Display) -> TrafficError {
        if kind == ErrorKind::BackendFailed {
            self.recovery_needed.set(true);
        }
        storage(error)
    }
    fn snapshot(&self) -> TrafficResult<Snapshot<S>> {
        self.check()?;
        self.reader.capture().map_err(|e| self.failed(e.kind(), e))
    }
    fn stored_range(&self, query: &TrafficQuery, now_ms: i64) -> TimeRange {
        let cutoff = match query.range.tier() {
            Tier::Minute => minute_cutoff(now_ms),
            Tier::Hour => self.retention.map_or(0, |retention| {
                hour_of(
                    now_ms.saturating_sub(i64::try_from(retention.as_millis()).unwrap_or(i64::MAX)),
                )
            }),
        };
        let from = query.range.start(now_ms).unwrap_or(0).max(cutoff);
        range(
            i64::from(from)
                * match query.range.tier() {
                    Tier::Minute => MINUTE_MS,
                    Tier::Hour => HOUR_MS,
                },
        )
    }
    fn series(
        &self,
        snapshot: &Snapshot<S>,
        query: &TrafficQuery,
        now_ms: i64,
        requests: &[GroupRequest<'_>],
        missing: bool,
    ) -> TrafficResult<Option<SeriesGroups>> {
        if query.scope == TrafficScope::Active || missing {
            return Ok(None);
        }
        snapshot
            .series(&SeriesQuery {
                range: self.stored_range(query, now_ms),
                tier: match query.range.tier() {
                    Tier::Minute => MINUTE,
                    Tier::Hour => HOUR,
                },
                requests,
                fields: FieldMask(7),
            })
            .map(Some)
            .map_err(|e| self.failed(e.kind(), e))
    }

    pub fn report(
        &self,
        session: &Session,
        request: &ReportRequest,
        now_ms: i64,
    ) -> TrafficResult<TrafficReport> {
        request.query.check()?;
        let snapshot = self.snapshot()?;
        let plan = ReportPlan::new(request);
        let ids = Filters::new(&snapshot, &request.query.filters)?;
        let filters = ids.filters();
        let empty = snapshot.find_label(LabelId(Dimension::Chain as u16), b"");
        let excluded = [empty.unwrap_or(0)];
        let grouping_filters: Vec<Vec<_>> = plan
            .dimensions
            .iter()
            .map(|g| {
                let mut out = Vec::with_capacity(filters.len() + 1);
                out.extend(
                    filters
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| g.ignored != Some(*i))
                        .map(|(_, f)| *f),
                );
                if g.skip_empty && empty.is_some() {
                    out.push(LabelFilter {
                        dimension: LabelId(Dimension::Chain as u16),
                        ids: &excluded,
                        exclude: true,
                    });
                }
                out
            })
            .collect();
        let dimensions: Vec<_> = plan
            .dimensions
            .iter()
            .map(|g| [LabelId(g.dimension as u16)])
            .collect();
        let mut requests = Vec::with_capacity(dimensions.len() + 1);
        requests.push(GroupRequest {
            filters: &filters,
            key: GroupKey::Total,
        });
        for (filters, dimension) in grouping_filters.iter().zip(&dimensions) {
            requests.push(GroupRequest {
                filters,
                key: GroupKey::Labels(dimension),
            });
        }
        // A value missing from the catalog matches no stored row; the scan is skipped when no
        // request can admit one.
        let missing = requests
            .iter()
            .all(|r| r.filters.iter().any(|f| !f.exclude && f.ids.is_empty()));
        let stored = self.series(&snapshot, &request.query, now_ms, &requests, missing)?;
        let session_rows = rows(session, &request.query, &[], now_ms);
        let capacity = session.row_capacity(request.query.range.tier(), request.query.scope);
        let mut state = ReportState::new(
            (0..plan.dimensions.len())
                .map(|i| capacity + stored.as_ref().map_or(0, |s| s.lists()[i + 1].len())),
        );
        if let Some(stored) = &stored {
            for group in stored.lists()[0].iter() {
                state.total.add(usage(group.sums), None);
            }
            for (i, grouping) in plan.dimensions.iter().enumerate() {
                for group in stored.lists()[i + 1].iter() {
                    let KeyPart::Label(id) = group.key[0] else {
                        unreachable!("label group")
                    };
                    state.groups[i].add(
                        display(&snapshot, grouping.dimension, id)?,
                        usage(group.sums),
                        None,
                    );
                }
            }
        }
        for row in session_rows.clone() {
            state.add(
                row,
                &plan,
                &request.query.filters,
                request.query.scope != TrafficScope::Closed,
            );
        }
        let topology = if let Some(t) = &request.topology {
            let mut projection = Projection::new(
                plan.layers.iter().map(|&i| &state.groups[i]),
                request.metric,
                t.limit_per_layer,
            );
            let layers: Vec<_> = t.layers.iter().map(|d| LabelId(*d as u16)).collect();
            let mut nodes: Vec<Vec<(u32, u16)>> = plan
                .layers
                .iter()
                .map(|&i| {
                    Vec::with_capacity(t.limit_per_layer.map_or(0, |limit| {
                        stored.as_ref().map_or(0, |s| s.lists()[i + 1].len()).min(
                            limit
                                * if plan.dimensions[i].dimension == Dimension::Profile {
                                    2
                                } else {
                                    1
                                },
                        )
                    }))
                })
                .collect();
            if t.limit_per_layer.is_some()
                && let Some(stored) = &stored
            {
                for (layer, (&slot, &dimension)) in plan.layers.iter().zip(&t.layers).enumerate() {
                    for group in stored.lists()[slot + 1].iter() {
                        let KeyPart::Label(id) = group.key[0] else {
                            unreachable!()
                        };
                        if id == 0 {
                            continue;
                        }
                        let key = display(&snapshot, dimension, id)?;
                        if let Some(node) = projection.kept(layer, key) {
                            nodes[layer].push((id, node as u16));
                        }
                    }
                }
            }
            let maps: Vec<_> = layers
                .iter()
                .zip(&nodes)
                .enumerate()
                .map(|(i, (&dimension, nodes))| LabelMap {
                    dimension,
                    nodes,
                    skip: (t.layers[i] == Dimension::Chain).then_some(empty).flatten(),
                })
                .collect();
            let key = if t.limit_per_layer.is_some() {
                GroupKey::Mapped(&maps)
            } else {
                GroupKey::Labels(&layers)
            };
            if let Some(paths) = self.series(
                &snapshot,
                &request.query,
                now_ms,
                &[GroupRequest {
                    filters: &filters,
                    key,
                }],
                ids.missing,
            )? {
                for group in paths.lists()[0].iter() {
                    let mut path = [None; 5];
                    for (i, part) in group.key.iter().enumerate() {
                        path[i] = match part {
                            KeyPart::Node(node) => Some(*node as usize),
                            KeyPart::Skipped => None,
                            KeyPart::Other => projection.resolve(i, None),
                            KeyPart::Label(id) => {
                                let key = display(&snapshot, t.layers[i], *id)?;
                                if t.layers[i] == Dimension::Chain && key.is_some_and(Key::is_empty)
                                {
                                    None
                                } else {
                                    projection.resolve(i, key)
                                }
                            }
                        };
                    }
                    projection.path(path.into_iter().take(t.layers.len()), usage(group.sums));
                }
            }
            for (d, u, _) in filter_rows(session_rows, &request.query.filters) {
                projection.session(d, u, &t.layers);
            }
            Some(projection.finish())
        } else {
            None
        };
        Ok(state.finish(&plan, request, topology))
    }

    fn with_groups<T>(
        &self,
        session: &Session,
        query: &TrafficQuery,
        dimension: Dimension,
        now_ms: i64,
        finish: impl FnOnce(Groups<'_>, Usage) -> T,
    ) -> TrafficResult<T> {
        query.check()?;
        let snapshot = self.snapshot()?;
        let ids = Filters::new(&snapshot, &query.filters)?;
        let filters = ids.filters();
        let dimensions = [LabelId(dimension as u16)];
        let requests = [
            GroupRequest {
                filters: &filters,
                key: GroupKey::Total,
            },
            GroupRequest {
                filters: &filters,
                key: GroupKey::Labels(&dimensions),
            },
        ];
        let stored = self.series(&snapshot, query, now_ms, &requests, ids.missing)?;
        let rows = rows(session, query, &query.filters, now_ms);
        let mut groups = Groups::new(
            session.row_capacity(query.range.tier(), query.scope)
                + stored.as_ref().map_or(0, |s| s.lists()[1].len()),
        );
        let mut total = Usage::default();
        if let Some(stored) = &stored {
            for group in stored.lists()[0].iter() {
                total = total.saturating_add(usage(group.sums));
            }
            for group in stored.lists()[1].iter() {
                let KeyPart::Label(id) = group.key[0] else {
                    unreachable!()
                };
                groups.add(display(&snapshot, dimension, id)?, usage(group.sums), None);
            }
        }
        for (d, u, r) in rows {
            total = total.saturating_add(u);
            groups.add(Some(Key::of(d, dimension)), u, r);
        }
        Ok(finish(groups, total))
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "paging parameters mirror the traffic RPC"
    )]
    pub fn usage_page(
        &self,
        session: &Session,
        query: &TrafficQuery,
        dimension: Dimension,
        metric: Metric,
        after: Option<&UsageCursor>,
        limit: usize,
        now_ms: i64,
    ) -> TrafficResult<UsagePage> {
        // Checked before the listed dimension's filters are left out.
        query.check()?;
        let query = query.ignoring(dimension);
        self.with_groups(session, &query, dimension, now_ms, |groups, total| {
            groups.page(total, metric, after, limit)
        })
    }
    pub fn usage_by_keys(
        &self,
        session: &Session,
        query: &TrafficQuery,
        dimension: Dimension,
        keys: &[String],
        now_ms: i64,
    ) -> TrafficResult<Vec<UsageGroup>> {
        self.with_groups(session, query, dimension, now_ms, |groups, _| {
            groups.by_keys(keys)
        })
    }
    pub fn summary(&self, session: &Session) -> TrafficResult<TrafficSummary> {
        let snapshot = self.snapshot()?;
        let status = snapshot.status().map_err(|e| self.failed(e.kind(), e))?;
        let data = status
            .data()
            .ok_or_else(|| storage("traffic store unavailable"))?;
        let count = data
            .streams()
            .iter()
            .find(|s| s.stream == StreamRef::Events(CLOSED))
            .expect("closed stream in schema")
            .rows;
        Ok(session.summary(count))
    }

    pub fn closed_connections(
        &self,
        session: &Session,
        selection: &ClosedSelection,
        before: Option<&ClosedCursor>,
        limit: usize,
        now_ms: i64,
    ) -> TrafficResult<ClosedPage> {
        check_filters(&selection.filters)?;
        let snapshot = self.snapshot()?;
        let ids = Filters::new(&snapshot, &selection.filters)?;
        let filters = ids.filters();
        let conditions = [EventFilter {
            column: 4,
            condition: EventCondition::Combination(&filters),
        }];
        let cutoff = self.retention.map(|retention| {
            now_ms.saturating_sub(i64::try_from(retention.as_millis()).unwrap_or(i64::MAX))
        });
        let since = selection.since_ms.into_iter().chain(cutoff).max();
        let limit = limit.clamp(1, MAX_CLOSED_PAGE);
        let mut stored = ClosedRecords::default();
        if !ids.missing {
            let page = snapshot
                .scan_events(&EventQuery {
                    stream: CLOSED,
                    range: range(since.unwrap_or(i64::MIN)),
                    direction: Direction::Backward,
                    cursor: before.copied().map(Into::into),
                    predicate: &EventPredicate {
                        filters: &conditions,
                    },
                    limit,
                })
                .map_err(|e| self.failed(e.kind(), e))?;
            stored.next = page.cursor().map(Into::into);
            stored.connections.reserve(page.events().len());
            let mut dimensions = HashMap::with_capacity(page.events().len());
            for event in page.events() {
                let [
                    EventValue::I64(started_at),
                    EventValue::I64(first_seen_at),
                    EventValue::U64(upload),
                    EventValue::U64(download),
                    EventValue::Combo(combo),
                    EventValue::Bytes(id),
                    target,
                ] = &event.values[..]
                else {
                    return Err(storage("invalid closed event values"));
                };
                let d = if let Some(d) = dimensions.get(combo) {
                    Arc::clone(d)
                } else {
                    let d = Arc::new(restore_dimensions(&snapshot, *combo)?);
                    dimensions.insert(*combo, Arc::clone(&d));
                    d
                };
                let mut d = d;
                if let EventValue::Bytes(target) = target {
                    Arc::make_mut(&mut d).target =
                        std::str::from_utf8(target).map_err(storage)?.to_owned();
                }
                stored.connections.push(PendingClosed {
                    id: std::str::from_utf8(id).map_err(storage)?.to_owned(),
                    started_at: *started_at,
                    first_seen_at: *first_seen_at,
                    closed_at: event.key,
                    seq: event.seq,
                    bytes: Bytes {
                        upload: *upload,
                        download: *download,
                    },
                    dimensions: d,
                });
            }
        }
        Ok(crate::model::merge_closed_page(
            stored,
            session.pending_closed(),
            before,
            limit,
            selection,
            cutoff,
        ))
    }
}

fn restore_dimensions<S: Storage>(
    snapshot: &Snapshot<S>,
    combo: nyanpasu_tsdb::ComboId,
) -> TrafficResult<Dimensions> {
    let mut ids = snapshot
        .combination(combo)
        .ok_or_else(|| storage("unknown combination"))?;
    let labels: [u32; LABELS] =
        std::array::from_fn(|_| ids.next().expect("schema combination width"));
    let raw = |dimension: u16| -> TrafficResult<Option<&[u8]>> {
        if labels[dimension as usize] == 0 {
            Ok(None)
        } else {
            snapshot
                .label(LabelId(dimension), labels[dimension as usize])
                .map(Some)
                .ok_or_else(|| storage("unknown combination label"))
        }
    };
    let text = |dimension| -> TrafficResult<String> {
        Ok(raw(dimension)?
            .map(std::str::from_utf8)
            .transpose()
            .map_err(storage)?
            .unwrap_or("unknown")
            .to_owned())
    };
    let (kind, payload) = raw(13)?
        .map(postcard::from_bytes::<(&str, &str)>)
        .transpose()
        .map_err(storage)?
        .unwrap_or(("unknown", ""));
    let chains: Vec<&str> = raw(14)?
        .map(postcard::from_bytes)
        .transpose()
        .map_err(storage)?
        .unwrap_or_default();
    let profile: Option<&str> = raw(9)?
        .map(postcard::from_bytes)
        .transpose()
        .map_err(storage)?
        .flatten();
    let basis = match raw(12)?.unwrap_or_default() {
        b"dialed" => Some(GeoBasis::Dialed),
        b"resolved" => Some(GeoBasis::Resolved),
        b"" => None,
        _ => return Err(storage("invalid destination basis")),
    };
    Ok(Dimensions {
        process: text(1)?,
        source: text(2)?,
        inbound: text(3)?,
        target: text(4)?,
        protocol: text(5)?,
        rule: RuleKey {
            kind: kind.to_owned(),
            payload: payload.to_owned(),
        },
        chains: chains.into_iter().map(str::to_owned).collect(),
        profile: profile.map(str::to_owned),
        source_region: text(10)?,
        destination_region: text(11)?,
        destination_basis: basis,
    })
}
