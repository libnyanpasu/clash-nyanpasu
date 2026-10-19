//! Series queries end to end: against a model of the writes across chunking, late writes,
//! folds and reopens; rankings and a topology's two passes; the budget; skipping; and
//! snapshot isolation.

use std::{collections::HashMap, sync::Arc, time::Duration};

use proptest::prelude::*;

use super::*;
use crate::{
    Profile, Timestamp,
    catalog::pinned_combo,
    chunk::{
        BatchSink,
        builder::{ChunkBuilder, Column as BatchColumn, Values},
        error::ReadError,
        format::{BlockEntry, ChunkKind, MAX_DATA_FRAGMENT_LEN, MIN_FRAGMENT0_LEN},
    },
    codec::{MAX_BLOCK_LEN, compress},
    db::{
        Tsdb,
        tests::{
            HOST, HOUR, HOUR_MS, MINUTE, MINUTE_MS, PROCESS, RULE, TARGET, open_with, point, schema,
        },
    },
    layout::manifest::{ChunkEntry, Listed},
    storage::{
        ChunkId, Durability, FragmentId, MAX_FRAGMENT_LEN, StorageWrite,
        memory::{MemoryDisk, MemoryStorage},
    },
    version::{OpenMarker, Registry, Version},
    write::{ComboId, Input, PinOp, SeriesPoint, chunking::series_columns},
};

pub(crate) fn at(point: SeriesPoint<'static>, at: i64) -> SeriesPoint<'static> {
    SeriesPoint {
        at: Timestamp(at),
        ..point
    }
}

/// Writes `points` as batch `batch_id` at `now`, durably, and returns the combination each
/// point resolved to. Each point is pinned in the same batch under a key of its own: a pin
/// of the same labels resolves the same way (`write::stage` resolves the points, then the
/// pins, against one catalog draft), and the store keeps the combination under the key.
pub(crate) fn write_at<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    now: i64,
    points: &[SeriesPoint<'_>],
) -> Vec<ComboId> {
    let keys: Vec<Vec<u8>> = (0..points.len())
        .map(|index| format!("{batch_id}/{index}").into_bytes())
        .collect();
    let pins: Vec<PinOp<'_>> = points
        .iter()
        .zip(&keys)
        .map(|(point, key)| PinOp::Pin {
            key,
            labels: point.labels,
        })
        .collect();
    let input = Input {
        batch_id,
        now: Timestamp(now),
        series: points,
        events: &[],
        aux: &[],
        pins: &pins,
    };
    tsdb.write(&input, Durability::Durable).unwrap();
    let snapshot = tsdb.storage().snapshot().unwrap();
    keys.iter()
        .map(|key| ComboId::new(pinned_combo(&snapshot, key).unwrap().unwrap()))
        .collect()
}

pub(crate) fn write(
    tsdb: &mut Tsdb<MemoryStorage>,
    batch_id: u64,
    points: &[SeriesPoint<'_>],
) -> Vec<ComboId> {
    write_at(tsdb, batch_id, 0, points)
}

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Timestamp(start), Timestamp(end)).unwrap()
}

/// The groups of each list, by key.
type Lists = Vec<HashMap<Vec<KeyPart>, Vec<u64>>>;

fn lists(groups: &SeriesGroups) -> Lists {
    groups
        .lists()
        .iter()
        .map(|list| {
            let map: HashMap<_, _> = list
                .iter()
                .map(|group| (group.key.to_vec(), group.sums.to_vec()))
                .collect();
            assert_eq!(map.len(), list.len(), "a key appears once");
            map
        })
        .collect()
}

/// What a result's reservation must equal: its lists, and each list's buffers.
fn held_by(groups: &SeriesGroups) -> u64 {
    let lists = (groups.lists.get().capacity() * size_of::<GroupList>()) as u64;
    lists + groups.lists().iter().map(GroupList::bytes).sum::<u64>()
}

fn label(snapshot: &Snapshot<MemoryStorage>, dimension: LabelId, value: &[u8]) -> u32 {
    snapshot.find_label(dimension, value).unwrap()
}

fn hour(range_hours: Range<i64>) -> TimeRange {
    range(range_hours.start * HOUR_MS, range_hours.end * HOUR_MS)
}

fn query<'a>(range: TimeRange, requests: &'a [GroupRequest<'a>], fields: u16) -> SeriesQuery<'a> {
    SeriesQuery {
        range,
        tier: HOUR,
        requests,
        fields: FieldMask(fields),
    }
}

static LABEL_SETS: [[&[u8]; 3]; 6] = [
    [b"match", b"curl", b"a.com"],
    [b"match", b"curl", b"b.com"],
    [b"direct", b"git", b"a.com"],
    [b"direct", b"", b"c.com"],
    [b"match", b"git", b"d.com"],
    [b"geoip", b"curl", b"e.com"],
];
const DIMENSIONS: [LabelId; 3] = [RULE, PROCESS, TARGET];

/// One batch, and how far the clock moved before it.
#[derive(Clone, Debug)]
struct Step {
    advance: i64,
    /// (tier, how far back from now, label set, fields).
    points: Vec<(TierId, i64, usize, [u64; 2])>,
    reopen: bool,
}

fn step() -> impl Strategy<Value = Step> {
    let point = (
        prop_oneof![Just(MINUTE), Just(HOUR)],
        0..4 * HOUR_MS,
        0..LABEL_SETS.len(),
        prop::array::uniform2(prop_oneof![9 => 0..100u64, 1 => Just(u64::MAX)]),
    );
    (
        0..2 * HOUR_MS,
        prop::collection::vec(point, 0..6),
        prop::bool::weighted(0.15),
    )
        .prop_map(|(advance, points, reopen)| Step {
            advance,
            points,
            reopen,
        })
}

/// Chunking early, late or under memory pressure only; folding never, partly or always.
fn knobs() -> impl Strategy<Value = Profile> {
    (
        prop_oneof![
            Just(Profile::desktop().catalog),
            Just(1u64),
            2_000u64..12_000
        ],
        prop_oneof![
            Just(None),
            Just(Some(Duration::from_secs(300))),
            Just(Some(Duration::from_secs(86_400)))
        ],
        prop_oneof![Just(6 * 1024u64), Just(32_000_000)],
    )
        .prop_map(|(catalog, quiet, head)| Profile {
            catalog,
            series_quiet_time: quiet,
            head,
            // A head this small holds no larger buffer; no test here writes events.
            event_buffer_bytes: 4096,
            ..Profile::desktop()
        })
}

/// A query with owned parts, from which [`SeriesQuery`] borrows.
#[derive(Clone, Debug)]
struct QuerySpec {
    tier: TierId,
    range: (i64, i64),
    mask: u16,
    requests: Vec<RequestSpec>,
}

#[derive(Clone, Debug)]
struct RequestSpec {
    /// (dimension, ids, exclude).
    filters: Vec<(LabelId, Vec<u32>, bool)>,
    key: KeySpec,
}

#[derive(Clone, Debug)]
enum KeySpec {
    Total,
    Labels(Vec<LabelId>),
    Mapped(Vec<MapSpec>),
}

/// (dimension, (id, node) pairs, skip).
type MapSpec = (LabelId, Vec<(u32, u16)>, Option<u32>);

fn dimension() -> impl Strategy<Value = LabelId> {
    prop::sample::select(DIMENSIONS.to_vec())
}

/// Label ids of a handful of values: 0, the overflow marker, the values written, and ids
/// no value has.
fn id() -> impl Strategy<Value = u32> {
    0..7u32
}

fn request() -> impl Strategy<Value = RequestSpec> {
    let filter = (
        dimension(),
        prop::collection::vec(id(), 0..4),
        any::<bool>(),
    );
    let map = (
        dimension(),
        prop::collection::vec((id(), 0..3u16), 0..4),
        prop::option::of(id()),
    );
    let key = prop_oneof![
        Just(KeySpec::Total),
        prop::collection::vec(dimension(), 1..4).prop_map(KeySpec::Labels),
        prop::collection::vec(map, 1..4).prop_map(KeySpec::Mapped),
    ];
    (prop::collection::vec(filter, 0..3), key)
        .prop_map(|(filters, key)| RequestSpec { filters, key })
}

fn time() -> impl Strategy<Value = i64> {
    prop_oneof![
        -4 * HOUR_MS..30 * HOUR_MS,
        (-240..1800i64).prop_map(|minute| minute * MINUTE_MS),
        (-4..30i64).prop_map(|hour| hour * HOUR_MS),
    ]
}

fn query_spec() -> impl Strategy<Value = QuerySpec> {
    let range = prop_oneof![
        2 => (time(), time()).prop_map(|(a, b)| (a.min(b), a.max(b))),
        1 => Just((i64::MIN, i64::MAX)),
    ];
    (
        prop_oneof![Just(MINUTE), Just(HOUR)],
        range,
        0..4u16,
        prop::collection::vec(request(), 0..4),
    )
        .prop_map(|(tier, range, mask, requests)| QuerySpec {
            tier,
            range,
            mask,
            requests,
        })
}

/// The sums written, by (tier, bucket, combination).
type Model = HashMap<(TierId, i64, u32), [u64; 2]>;

fn width(tier: TierId) -> i64 {
    if tier == MINUTE { MINUTE_MS } else { HOUR_MS }
}

/// The buckets `spec` reads: every one its range touches.
fn buckets_of(spec: &QuerySpec) -> Range<i64> {
    let (start, end) = spec.range;
    let width = width(spec.tier);
    let first = start.div_euclid(width);
    let after = if start == end {
        first
    } else {
        (end - 1).div_euclid(width) + 1
    };
    first..after
}

/// The groups the model gives `spec`, and whether a folded combination is in its range,
/// computed from the writes and the snapshot's combinations alone.
fn expected(snapshot: &Snapshot<MemoryStorage>, model: &Model, spec: &QuerySpec) -> (Lists, bool) {
    let Range {
        start: first,
        end: after,
    } = buckets_of(spec);
    let mut lists: Lists = vec![HashMap::new(); spec.requests.len()];
    let mut folded = false;
    for (&(tier, bucket, combo), fields) in model {
        if tier != spec.tier || bucket < first || bucket >= after {
            continue;
        }
        let labels: Vec<u32> = snapshot.combination(ComboId::new(combo)).unwrap().collect();
        let of =
            |dimension: LabelId| labels[DIMENSIONS.iter().position(|&d| d == dimension).unwrap()];
        // Whatever the filters; a query of no requests scans nothing.
        folded |= labels.contains(&0) && !spec.requests.is_empty();
        let sums: Vec<u64> = (0..2)
            .filter(|field| spec.mask >> field & 1 == 1)
            .map(|field| fields[field])
            .collect();
        for (request, list) in spec.requests.iter().zip(&mut lists) {
            let admitted = request
                .filters
                .iter()
                .all(|(dimension, ids, exclude)| ids.contains(&of(*dimension)) != *exclude);
            if !admitted {
                continue;
            }
            let key: Vec<KeyPart> = match &request.key {
                KeySpec::Total => Vec::new(),
                KeySpec::Labels(dimensions) => dimensions
                    .iter()
                    .map(|&dimension| KeyPart::Label(of(dimension)))
                    .collect(),
                KeySpec::Mapped(maps) => maps
                    .iter()
                    .map(|(dimension, nodes, skip)| {
                        let label = of(*dimension);
                        if *skip == Some(label) {
                            KeyPart::Skipped
                        } else {
                            nodes
                                .iter()
                                .find(|(id, _)| *id == label)
                                .map_or(KeyPart::Other, |&(_, node)| KeyPart::Node(node))
                        }
                    })
                    .collect(),
            };
            let entry = list.entry(key).or_insert_with(|| vec![0; sums.len()]);
            for (sum, value) in entry.iter_mut().zip(&sums) {
                *sum = sum.saturating_add(*value);
            }
        }
    }
    (lists, folded)
}

fn run(
    snapshot: &Snapshot<MemoryStorage>,
    spec: &QuerySpec,
) -> Result<SeriesGroups, SeriesQueryError> {
    let maps: Vec<Vec<LabelMap<'_>>> = spec
        .requests
        .iter()
        .map(|request| match &request.key {
            KeySpec::Mapped(maps) => maps
                .iter()
                .map(|(dimension, nodes, skip)| LabelMap {
                    dimension: *dimension,
                    nodes,
                    skip: *skip,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    let filters: Vec<Vec<LabelFilter<'_>>> = spec
        .requests
        .iter()
        .map(|request| {
            request
                .filters
                .iter()
                .map(|(dimension, ids, exclude)| LabelFilter {
                    dimension: *dimension,
                    ids,
                    exclude: *exclude,
                })
                .collect()
        })
        .collect();
    let requests: Vec<GroupRequest<'_>> = spec
        .requests
        .iter()
        .zip(&filters)
        .zip(&maps)
        .map(|((request, filters), maps)| GroupRequest {
            filters,
            key: match &request.key {
                KeySpec::Total => GroupKey::Total,
                KeySpec::Labels(dimensions) => GroupKey::Labels(dimensions),
                KeySpec::Mapped(_) => GroupKey::Mapped(maps),
            },
        })
        .collect();
    snapshot.series(&SeriesQuery {
        range: range(spec.range.0, spec.range.1),
        tier: spec.tier,
        requests: &requests,
        fields: FieldMask(spec.mask),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    // Results are exact over the chunks and the head, whatever was chunked, folded, written
    // late or reopened (§4.3, §5.2, §7.2).
    #[test]
    fn queries_equal_a_model_of_the_writes(
        profile in knobs(),
        steps in prop::collection::vec(step(), 1..10),
        specs in prop::collection::vec(query_spec(), 1..8),
    ) {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, profile.clone());
        let mut model = Model::new();
        let mut now = 0;
        for (index, step) in steps.iter().enumerate() {
            now += step.advance;
            let points: Vec<SeriesPoint<'_>> = step
                .points
                .iter()
                .map(|&(tier, back, set, fields)| SeriesPoint {
                    tier,
                    at: Timestamp(now - back),
                    labels: &LABEL_SETS[set],
                    fields: Vec::leak(fields.to_vec()),
                })
                .collect();
            let combos = write_at(&mut tsdb, index as u64 + 1, now, &points);
            for (combo, point) in combos.iter().zip(&points) {
                let bucket = (point.at.0).div_euclid(width(point.tier));
                let sums = model.entry((point.tier, bucket, combo.get())).or_default();
                for (sum, value) in sums.iter_mut().zip(point.fields) {
                    *sum = sum.saturating_add(*value);
                }
            }
            if step.reopen {
                drop(tsdb.into_storage());
                tsdb = open_with(&disk, profile.clone());
            }
        }

        let snapshot = tsdb.reader().capture().unwrap();
        let before = snapshot.budget().reserved();
        for spec in &specs {
            let groups = run(&snapshot, spec).unwrap();
            let (expected, folded) = expected(&snapshot, &model, spec);
            prop_assert_eq!(lists(&groups), expected, "{:?}", spec);
            prop_assert_eq!(groups.folded(), folded, "{:?}", spec);
            let buckets = buckets_of(spec);
            let start = |bucket: i64, edge| {
                bucket.checked_mul(width(spec.tier)).unwrap_or(edge)
            };
            let covered = range(
                start(buckets.start, i64::MIN),
                start(buckets.end, i64::MAX),
            );
            prop_assert_eq!(groups.covered(), covered);
            prop_assert!(!groups.truncated_by_retention());
            // The result holds exactly the bytes of its groups; everything else returned.
            prop_assert_eq!(snapshot.budget().reserved(), before + held_by(&groups));
            drop(groups);
            prop_assert_eq!(snapshot.budget().reserved(), before);
        }
    }
}

/// Writes `points` into the hour tier's bucket 0 at time 0.
fn hour_points(tsdb: &mut Tsdb<MemoryStorage>, batch_id: u64, points: &[(&[&[u8]; 3], [u64; 2])]) {
    let points: Vec<SeriesPoint<'_>> = points
        .iter()
        .map(|(labels, fields)| SeriesPoint {
            tier: HOUR,
            at: Timestamp(0),
            labels: &labels[..],
            fields: Vec::leak(fields.to_vec()),
        })
        .collect();
    write(tsdb, batch_id, &points);
}

// Each ranking ignores the filter on its own dimension (#5807), and they all share one scan.
#[test]
fn rankings_share_one_scan_and_each_ignores_its_own_filter() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    hour_points(
        &mut tsdb,
        1,
        &[
            (&[b"match", b"curl", b"a.com"], [1, 10]),
            (&[b"match", b"curl", b"b.com"], [2, 20]),
            (&[b"direct", b"git", b"a.com"], [4, 40]),
            (&[b"match", b"git", b"c.com"], [8, 80]),
            (&[b"direct", b"curl", b"c.com"], [16, 160]),
        ],
    );
    let snapshot = tsdb.reader().capture().unwrap();
    let id = |dimension, value: &[u8]| label(&snapshot, dimension, value);
    let (rule, process) = ([id(RULE, b"match")], [id(PROCESS, b"curl")]);
    let rule_filter = LabelFilter {
        dimension: RULE,
        ids: &rule,
        exclude: false,
    };
    let process_filter = LabelFilter {
        dimension: PROCESS,
        ids: &process,
        exclude: false,
    };
    let requests = [
        GroupRequest {
            filters: &[rule_filter, process_filter],
            key: GroupKey::Total,
        },
        GroupRequest {
            filters: &[process_filter],
            key: GroupKey::Labels(&[RULE]),
        },
        GroupRequest {
            filters: &[rule_filter],
            key: GroupKey::Labels(&[PROCESS]),
        },
        GroupRequest {
            filters: &[rule_filter, process_filter],
            key: GroupKey::Labels(&[TARGET]),
        },
    ];
    let groups = snapshot
        .series(&query(hour(0..1), &requests, 0b10))
        .unwrap();
    let key = |dimension, value: &[u8]| vec![KeyPart::Label(id(dimension, value))];
    let expected: Lists = vec![
        HashMap::from([(vec![], vec![30])]),
        HashMap::from([
            (key(RULE, b"match"), vec![30]),
            (key(RULE, b"direct"), vec![160]),
        ]),
        HashMap::from([
            (key(PROCESS, b"curl"), vec![30]),
            (key(PROCESS, b"git"), vec![80]),
        ]),
        HashMap::from([
            (key(TARGET, b"a.com"), vec![10]),
            (key(TARGET, b"b.com"), vec![20]),
        ]),
    ];
    assert_eq!(lists(&groups), expected);
}

// The two passes of §8.1: per-level rankings that leave the skipped value out, then the
// mapped paths, where a skipped level joins its neighbours. Both read one snapshot, half of
// it in chunks and half in the head.
#[test]
fn a_topology_ranks_each_level_then_groups_mapped_paths() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let first = [
        at(point(&[b"match", b"curl", b"a.com"], &[0, 10]), 0),
        at(point(&[b"match", b"", b"a.com"], &[0, 50]), 0),
    ];
    write_at(&mut tsdb, 1, 0, &first);
    let second = [
        at(point(&[b"direct", b"git", b"b.com"], &[0, 3]), HOUR_MS),
        at(point(&[b"match", b"git", b"c.com"], &[0, 1]), HOUR_MS),
    ];
    // Bucket 0 has been quiet for a bucket: this commit chunks it.
    write_at(&mut tsdb, 2, 2 * HOUR_MS, &second);
    assert_eq!(chunked_buckets(&tsdb), [0]);
    let snapshot = tsdb.reader().capture().unwrap();
    let id = |dimension, value: &[u8]| label(&snapshot, dimension, value);

    // Pass 1. Without its filter, the empty chain would outrank curl.
    let empty = [id(PROCESS, b"")];
    let skip_empty = [LabelFilter {
        dimension: PROCESS,
        ids: &empty,
        exclude: true,
    }];
    let levels = [
        GroupRequest {
            filters: &[],
            key: GroupKey::Labels(&[RULE]),
        },
        GroupRequest {
            filters: &skip_empty,
            key: GroupKey::Labels(&[PROCESS]),
        },
        GroupRequest {
            filters: &[],
            key: GroupKey::Labels(&[TARGET]),
        },
    ];
    let ranked = snapshot.series(&query(hour(0..2), &levels, 0b10)).unwrap();
    let top = |list: &GroupList| {
        let best = list.iter().max_by_key(|group| group.sums[0]).unwrap();
        let [KeyPart::Label(id)] = best.key else {
            panic!("a label key: {:?}", best.key)
        };
        *id
    };
    let kept: Vec<u32> = ranked.lists().iter().map(top).collect();
    assert_eq!(
        kept,
        [
            id(RULE, b"match"),
            id(PROCESS, b"curl"),
            id(TARGET, b"a.com")
        ]
    );

    // Pass 2: each level keeps its node 0; the empty chain is skipped.
    let nodes: Vec<[(u32, u16); 1]> = kept.iter().map(|&id| [(id, 0)]).collect();
    let maps = [
        LabelMap {
            dimension: RULE,
            nodes: &nodes[0],
            skip: None,
        },
        LabelMap {
            dimension: PROCESS,
            nodes: &nodes[1],
            skip: Some(empty[0]),
        },
        LabelMap {
            dimension: TARGET,
            nodes: &nodes[2],
            skip: None,
        },
    ];
    let paths = [GroupRequest {
        filters: &[],
        key: GroupKey::Mapped(&maps),
    }];
    let mapped = snapshot.series(&query(hour(0..2), &paths, 0b10)).unwrap();
    use KeyPart::{Node, Other, Skipped};
    let expected: Lists = vec![HashMap::from([
        (vec![Node(0), Node(0), Node(0)], vec![10]),
        (vec![Node(0), Skipped, Node(0)], vec![50]),
        (vec![Other, Other, Other], vec![3]),
        (vec![Node(0), Other, Other], vec![1]),
    ])];
    assert_eq!(lists(&mapped), expected);
}

/// The buckets of the hour tier that have chunks.
fn chunked_buckets<S: Storage>(tsdb: &Tsdb<S>) -> Vec<i64> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    manifest::stream_chunks(&snapshot, StreamRef::Tier(HOUR))
        .unwrap()
        .iter()
        .map(|listed| listed.entry.key_range.0)
        .collect()
}

// A catalog with no room folds every combination into the whole overflow (§5.2). The overflow
// marker is a label like any other to filters, and the result says the scan met folded rows
// even when a filter turned them all away and left no group (§9.4). Unfolded, it says not.
#[test]
fn a_result_reports_folded_rows_its_filters_turned_away() {
    let unfolded_rules = [GroupRequest {
        filters: &[LabelFilter {
            dimension: RULE,
            ids: &[0],
            exclude: true,
        }],
        key: GroupKey::Total,
    }];
    for (catalog, folded) in [(1, true), (Profile::desktop().catalog, false)] {
        let disk = MemoryDisk::new();
        let profile = Profile {
            catalog,
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile);
        let combos = write(
            &mut tsdb,
            1,
            &[point(&[b"match", b"curl", b"a.com"], &[1, 2])],
        );
        assert_eq!(combos[0] == ComboId::new(0), folded);
        let snapshot = tsdb.reader().capture().unwrap();
        let groups = snapshot
            .series(&query(hour(0..1), &unfolded_rules, 0b11))
            .unwrap();
        let admitted = if folded {
            HashMap::new()
        } else {
            HashMap::from([(vec![], vec![1, 2])])
        };
        assert_eq!(lists(&groups), vec![admitted]);
        assert_eq!(groups.folded(), folded);
    }
}

// The head's combinations are checked against the catalog as they enter it, so a version
// whose head names one its catalog lacks is damage, reported as a chunk's is, not a panic.
#[test]
fn a_head_combination_the_catalog_lacks_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    write(
        &mut tsdb,
        1,
        &[point(&[b"match", b"curl", b"a.com"], &[1, 2])],
    );
    let current = tsdb.current();
    let schema = schema();
    let damaged = Version {
        publication: current.publication,
        catalog: Catalog::new(&schema),
        head: current.head.clone(),
    };
    let head = crate::budget::MemoryBudget::new(u64::MAX)
        .try_reserve("head", damaged.head.bytes())
        .unwrap();
    let registry = Registry::new(damaged, Arc::new(schema), Arc::new(OpenMarker), head);
    let shared = Arc::new(crate::version::Shared::new(
        tsdb.into_storage(),
        registry,
        MemoryBudget::new(u64::MAX),
        false,
    ));
    let snapshot = shared.capture().unwrap();
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let error = snapshot
        .series(&query(hour(0..1), &total, 0b11))
        .unwrap_err();
    assert!(
        matches!(
            error,
            SeriesQueryError::UnknownCombination {
                chunk: None,
                combo: 1
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

#[test]
fn a_query_the_schema_cannot_answer_is_local_and_reserves_nothing() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    write(
        &mut tsdb,
        1,
        &[point(&[b"match", b"curl", b"a.com"], &[1, 2])],
    );
    let snapshot = tsdb.reader().capture().unwrap();
    let before = snapshot.budget().reserved();
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let host = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&[RULE, HOST]),
    }];
    let cases = [
        SeriesQuery {
            tier: TierId(2),
            ..query(hour(0..1), &total, 1)
        },
        query(hour(0..1), &total, 0b100),
        query(hour(0..1), &host, 1),
    ];
    let errors: Vec<SeriesQueryError> = cases
        .iter()
        .map(|case| snapshot.series(case).unwrap_err())
        .collect();
    assert!(matches!(
        errors[0],
        SeriesQueryError::UnknownTier { tier: TierId(2) }
    ));
    assert!(matches!(
        errors[1],
        SeriesQueryError::UnknownField {
            mask: 0b100,
            fields: 2
        }
    ));
    assert!(matches!(
        errors[2],
        SeriesQueryError::NotADimension {
            request: 0,
            dimension: HOST
        }
    ));
    for error in &errors {
        assert_eq!(error.kind(), ErrorKind::Local, "{error}");
    }
    assert_eq!(snapshot.budget().reserved(), before);
}

/// A store whose hour bucket 0 holds `targets` combinations, all in the head.
fn many_targets(targets: usize) -> (MemoryDisk, Tsdb<MemoryStorage>) {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let names: Vec<&'static [u8]> = (0..targets)
        .map(|target| &*Vec::leak(format!("{target}.com").into_bytes()))
        .collect();
    let points: Vec<SeriesPoint<'_>> = names
        .iter()
        .map(|&name| point(Vec::leak(vec![b"match" as &[u8], b"curl", name]), &[1, 1]))
        .collect();
    write(&mut tsdb, 1, &points);
    (disk, tsdb)
}

// The groups grow within the budget, and too many fail the query as a `Local` error that
// returns everything it reserved (§8.2).
#[test]
fn groups_past_the_budget_fail_the_query_and_return_its_reservation() {
    let (_disk, tsdb) = many_targets(300);
    let snapshot = tsdb.reader().capture().unwrap();
    let budget = snapshot.budget();
    let by_target = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&[TARGET]),
    }];
    let ranking = query(hour(0..1), &by_target, 0b11);

    // With room to spare, the result holds exactly its groups.
    let groups = snapshot.series(&ranking).unwrap();
    assert_eq!(groups.lists()[0].len(), 300);
    let needed = held_by(&groups);
    drop(groups);

    // Leave room for the scan and the list, and less than the groups need.
    let before = budget.reserved();
    let scan = scan_memory(2, 1) + size_of::<GroupList>() as u64;
    let available = Profile::desktop().query - before;
    let _hog = budget
        .try_reserve("test", available - scan - needed / 2)
        .unwrap();
    let error = snapshot.series(&ranking).unwrap_err();
    assert!(
        matches!(error, SeriesQueryError::ReserveGroups { request: 0, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(budget.reserved(), before + available - scan - needed / 2);
}

// An entry that records a length no fragment 0 has is damage: the query fails `Corrupted`
// before it reserves the fragment it would be lent by it, where it would have failed for room.
#[test]
fn an_entry_recording_no_fragment_0_a_chunk_has_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let BigChunk { chunk, entry, .. } = big_chunk(&mut tsdb, false);
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let recorded = [
        0,
        MIN_FRAGMENT0_LEN as u32 - 1,
        MAX_FRAGMENT_LEN as u32 + 1,
        u32::MAX,
    ];
    for fragment0 in recorded {
        let damaged = ChunkEntry { fragment0, ..entry };
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        manifest::write_chunk(&mut batch, StreamRef::Tier(HOUR), 0, chunk, &damaged).unwrap();
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        let snapshot = tsdb.reader().capture().unwrap();
        let error = snapshot
            .series(&query(hour(0..24), &total, 0b11))
            .unwrap_err();
        assert!(
            matches!(
                error,
                SeriesQueryError::OpenChunk {
                    chunk: found,
                    source: ReadError::RecordedFragment0 { recorded }
                } if found == chunk.0 && recorded == fragment0
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
}

// The decoder is reserved before the first chunk is read, and only when one is: a query of the
// head alone does without it. The fragment the reader decodes from is reserved before each
// chunk is opened, as its manifest entry says, and the chunk's directory before it is fetched.
#[test]
fn reading_chunks_is_reserved_before_the_first_one_is_opened() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let labels: &[&[u8]] = &[b"match", b"curl", b"a.com"];
    write_at(&mut tsdb, 1, 0, &[at(point(labels, &[1, 2]), 0)]);
    write_at(
        &mut tsdb,
        2,
        5 * HOUR_MS,
        &[at(point(labels, &[3, 4]), 5 * HOUR_MS)],
    );
    assert_eq!(chunked_buckets(&tsdb), [0]);
    let snapshot = tsdb.reader().capture().unwrap();
    let chunks = manifest::stream_chunks(snapshot.storage(), StreamRef::Tier(HOUR)).unwrap();
    // A small chunk lends at most a data fragment at once.
    let lent = lent_memory(chunks[0].entry.fragment0);
    assert_eq!(lent, MAX_DATA_FRAGMENT_LEN as u64);
    let reading = SERIES_DECOMPRESSION_CONTEXT + lent;
    let budget = snapshot.budget();
    let before = budget.reserved();
    let available = Profile::desktop().query - before;
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let refused = |room: u64| {
        let hog = budget.try_reserve("test", available - room).unwrap();
        let error = snapshot
            .series(&query(hour(0..6), &total, 0b11))
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Local, "{error:?}");
        assert_eq!(budget.reserved(), before + hog.bytes());
        error
    };

    // Less than the decoder: the head alone is read, a chunk is not.
    let hog = budget
        .try_reserve("test", available - (SERIES_DECOMPRESSION_CONTEXT - 1))
        .unwrap();
    let head = snapshot.series(&query(hour(5..6), &total, 0b11)).unwrap();
    assert_eq!(lists(&head), vec![HashMap::from([(vec![], vec![3, 4])])]);
    drop((head, hog));
    let error = refused(SERIES_DECOMPRESSION_CONTEXT - 1);
    assert!(
        matches!(error, SeriesQueryError::ReserveDecoder { .. }),
        "{error:?}"
    );

    // The decoder, but not the fragment it reads from.
    for room in [SERIES_DECOMPRESSION_CONTEXT + 1024, reading - 1] {
        let error = refused(room);
        assert!(
            matches!(error, SeriesQueryError::ReserveFragment { chunk, .. } if chunk == chunks[0].chunk.0),
            "{room}: {error:?}"
        );
    }

    // The decoder and a fragment, but not the chunk's directory.
    let error = refused(reading + 1024);
    assert!(
        matches!(
            error,
            SeriesQueryError::OpenChunk {
                source: ReadError::ReserveDirectory { .. },
                ..
            }
        ),
        "{error:?}"
    );

    // Nothing at all fits: refused before the scan starts.
    let error = refused(scan_memory(2, 0) - 1);
    assert!(
        matches!(error, SeriesQueryError::ReserveScan { .. }),
        "{error:?}"
    );
}

// A bulk decompression needs no more than the context the decoder's reservation counts.
#[test]
fn the_decompression_context_fits_its_reservation() {
    use zstd::zstd_safe::DCtx;
    let body = vec![7u8; MAX_BLOCK_LEN];
    let frame = zstd::bulk::compress(&body, 1).unwrap();
    let mut context = DCtx::create();
    let mut out = vec![0u8; MAX_BLOCK_LEN];
    assert_eq!(
        context.decompress(&mut out[..], &frame).unwrap(),
        MAX_BLOCK_LEN
    );
    let size = context.sizeof() as u64;
    assert!(size <= SERIES_DECOMPRESSION_CONTEXT, "{size} bytes");
}

/// The fragments of a chunk, its directory first.
fn fragments(listed: &Listed) -> impl Iterator<Item = FragmentId> + '_ {
    (0..listed.entry.fragments).map(|index| FragmentId {
        chunk: listed.chunk,
        index,
    })
}

// A chunk outside the range is never read: damage to every fragment of it goes unnoticed,
// until a query reaches it.
#[test]
fn a_chunk_outside_the_range_is_never_read() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let labels: &[&[u8]] = &[b"match", b"curl", b"a.com"];
    write_at(&mut tsdb, 1, 0, &[at(point(labels, &[1, 2]), 0)]);
    write_at(
        &mut tsdb,
        2,
        5 * HOUR_MS,
        &[at(point(labels, &[3, 4]), 5 * HOUR_MS)],
    );
    write_at(&mut tsdb, 3, 7 * HOUR_MS, &[]);
    assert_eq!(chunked_buckets(&tsdb), [0, 5]);
    let snapshot = tsdb.storage().snapshot().unwrap();
    let chunks = manifest::stream_chunks(&snapshot, StreamRef::Tier(HOUR)).unwrap();
    for fragment in fragments(&chunks[0]) {
        tsdb.storage().corrupt_fragment(fragment);
    }

    let snapshot = tsdb.reader().capture().unwrap();
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let groups = snapshot.series(&query(hour(1..6), &total, 0b11)).unwrap();
    assert_eq!(lists(&groups), vec![HashMap::from([(vec![], vec![3, 4])])]);
    let error = snapshot
        .series(&query(hour(0..6), &total, 0b11))
        .unwrap_err();
    assert!(
        matches!(error, SeriesQueryError::OpenChunk { chunk, .. } if chunk == chunks[0].chunk.0),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

/// Rows per bucket of [`big_chunk`].
const ROWS: u32 = 20_000;

/// A chunk of hour buckets 10, 11 and 12, [`ROWS`] each, written behind the store's back:
/// its id and manifest entry, and each row's combination, upload and download.
struct BigChunk {
    chunk: ChunkId,
    entry: ChunkEntry,
    combos: Vec<u32>,
    uploads: Vec<u64>,
    downloads: Vec<u64>,
}

/// Writes a [`BigChunk`], its rows sorted by bucket and combination, among combination 0 and
/// three that a batch creates. Uploads are values nothing shrinks, so their column takes
/// several fragments; downloads repeat a few such values, which only zstd finds, so their
/// blocks are compressed when the chunk is, as a consolidated chunk is.
fn big_chunk(tsdb: &mut Tsdb<MemoryStorage>, compressed: bool) -> BigChunk {
    let points: Vec<SeriesPoint<'_>> = LABEL_SETS[..3]
        .iter()
        .map(|labels| point(labels, &[0, 0]))
        .collect();
    let created = write(tsdb, 1, &points);
    let ids = [0, created[0].get(), created[1].get(), created[2].get()];
    let rows = 3 * ROWS as usize;
    let buckets: Vec<i64> = (0..rows)
        .map(|row| 10 + (row / ROWS as usize) as i64)
        .collect();
    let combos: Vec<u32> = (0..rows)
        .map(|row| ids[row % ROWS as usize * 4 / ROWS as usize])
        .collect();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut noise = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let uploads: Vec<u64> = (0..rows).map(|_| noise()).collect();
    let pattern: Vec<u64> = (0..16).map(|_| noise()).collect();
    let downloads: Vec<u64> = (0..rows).map(|row| pattern[row % 16]).collect();

    let chunk = ChunkId(1 << 40);
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    let mut sink = BatchSink {
        batch: &mut batch,
        chunk,
    };
    let layout = series_columns(2);
    let kind = ChunkKind::Consolidated;
    let fingerprint = tsdb.shape().fingerprint;
    let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
    let compressor = compressed.then_some(&mut compressor);
    let mut builder = ChunkBuilder::new(
        &mut sink,
        kind,
        fingerprint,
        compressor,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let column = |values| BatchColumn {
        values,
        validity: None,
    };
    // Rows go in a few at a time, as a merge hands them over, so the blocks of all columns
    // fill fragments in row order.
    for start in (0..rows).step_by(1024) {
        let part = start..(start + 1024).min(rows);
        builder
            .append(&[
                column(Values::I64(&buckets[part.clone()])),
                column(Values::U32(&combos[part.clone()])),
                column(Values::U64(&uploads[part.clone()])),
                column(Values::U64(&downloads[part])),
            ])
            .unwrap();
    }
    let built = builder.finish().unwrap();
    let entry = ChunkEntry {
        kind: kind as u8,
        run: chunk.0,
        key_range: built.key_range,
        rows: built.rows,
        fragments: built.fragments,
        fragment0: built.fragment0,
        bytes: built.bytes,
    };
    manifest::write_chunk(&mut batch, StreamRef::Tier(HOUR), 0, chunk, &entry).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    BigChunk {
        chunk,
        entry,
        combos,
        uploads,
        downloads,
    }
}

// Within a chunk of several buckets, the blocks of rows outside the range are never read,
// and neither are the columns of fields not asked for: damage to every fragment that holds
// only such blocks goes unnoticed.
#[test]
fn blocks_outside_the_range_or_the_fields_are_never_read() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let BigChunk {
        chunk,
        entry,
        combos,
        uploads,
        ..
    } = big_chunk(&mut tsdb, false);
    let storage = tsdb.storage();

    // The fragments a query of bucket 12's uploads needs: those of the combination and
    // upload blocks with rows of bucket 12, and of the bucket blocks that straddle 11 and 12.
    let selected = 2 * ROWS..3 * ROWS;
    let snapshot = tsdb.storage().snapshot().unwrap();
    let source = StoredChunk {
        storage: &snapshot,
        chunk,
    };
    let reader = ChunkReader::open(
        &source,
        &crate::budget::MemoryAccount::new(
            &MemoryBudget::new(u64::MAX),
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        entry.fragment0,
    )
    .unwrap();
    let overlaps = |entry: &BlockEntry| {
        entry.first_row < selected.end && entry.first_row + entry.rows > selected.start
    };
    let mut needed: Vec<u32> = [COMBO, FIRST_FIELD]
        .into_iter()
        .flat_map(|column| reader.blocks(column).unwrap())
        .filter(|entry| overlaps(entry))
        .chain(
            reader
                .blocks(BUCKET)
                .unwrap()
                .iter()
                .filter(|entry| entry.first_row < selected.start && overlaps(entry)),
        )
        .map(|entry| entry.fragment)
        .collect();
    needed.extend([0]);
    let listed = Listed {
        extent: 0,
        chunk,
        entry,
    };
    let damaged: Vec<FragmentId> = fragments(&listed)
        .filter(|fragment| !needed.contains(&fragment.index))
        .collect();
    assert!(
        damaged.len() >= 2,
        "the test damages too little: {damaged:?}"
    );
    for &fragment in &damaged {
        storage.corrupt_fragment(fragment);
    }

    let snapshot = tsdb.reader().capture().unwrap();
    let by_combo = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&[RULE, PROCESS, TARGET]),
    }];
    let groups = snapshot
        .series(&query(hour(12..13), &by_combo, 0b01))
        .unwrap();
    let mut expected: HashMap<Vec<KeyPart>, Vec<u64>> = HashMap::new();
    for row in selected.clone() {
        let combo = combos[row as usize];
        let key: Vec<KeyPart> = snapshot
            .combination(ComboId::new(combo))
            .unwrap()
            .map(KeyPart::Label)
            .collect();
        let entry = expected.entry(key).or_insert(vec![0]);
        entry[0] = entry[0].saturating_add(uploads[row as usize]);
    }
    assert_eq!(lists(&groups), vec![expected]);
    // A quarter of each bucket's rows are of combination 0.
    assert!(groups.folded());
    // Reading what was damaged fails.
    let error = snapshot
        .series(&query(hour(10..13), &by_combo, 0b11))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted, "{error:?}");
}

/// The bytes the buffers of a decoded block hold.
fn output_held(block: &IntBlock<u64>) -> u64 {
    block.buffers().iter().map(|&(_, bytes)| bytes as u64).sum()
}

/// The bytes the decoder's scratch holds.
fn scratch_held(decoder: &BlockDecoder) -> u64 {
    decoder
        .buffers()
        .iter()
        .map(|&(_, bytes)| bytes as u64)
        .sum()
}

// Before a block is decoded, the column's output and the decoder's scratch are reserved as
// the larger of what they hold and what the block declares; after it, they are shrunk to the
// block's, which is what the buffers then hold. A block that does not fit is never decoded.
#[test]
fn a_column_reserves_each_block_before_decoding_it() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let BigChunk {
        chunk,
        entry,
        downloads,
        ..
    } = big_chunk(&mut tsdb, true);
    let snapshot = tsdb.storage().snapshot().unwrap();
    let source = StoredChunk {
        storage: &snapshot,
        chunk,
    };
    let reader = ChunkReader::open(
        &source,
        &crate::budget::MemoryAccount::new(
            &MemoryBudget::new(u64::MAX),
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        entry.fragment0,
    )
    .unwrap();
    let column = FIRST_FIELD + 1;
    let ty = reader.columns()[usize::from(column)].ty;
    let blocks = reader.blocks(column).unwrap();
    let (first, last) = (blocks[0], blocks[blocks.len() - 1]);
    let (large, small) = (
        decode_memory(first.shape(), ty),
        decode_memory(last.shape(), ty),
    );
    assert!(small.output < large.output && small.scratch < large.scratch);
    assert_ne!(small.scratch, 0, "the downloads' blocks are compressed");

    let budget = MemoryBudget::new(large.output + large.scratch);
    let account = crate::budget::MemoryAccount::new(
        &budget,
        "test decode",
        crate::budget::MemoryPlan::default(),
    )
    .unwrap();
    let mut decoder = Decoder {
        blocks: BlockDecoder::for_buffers(&account),
        account: account.clone(),
        _padding: account.lease(crate::budget::MemoryPlan::default()).unwrap(),
    };
    let revoked = std::sync::atomic::AtomicBool::new(false);
    let mut decode = Decode {
        revoked: QueryCheck::unarmed(&revoked),
        chunk: chunk.0,
        reader: &reader,
        source: &source,
        decoder: &mut decoder,
    };
    let mut downloads_column = Column::<u64>::new(column);
    let end = 3 * ROWS - 1;
    for (row, memory) in [(end, small), (1, large), (end, small)] {
        let value = downloads_column.value(&mut decode, row).unwrap();
        assert_eq!(value, downloads[row as usize]);
        assert_eq!(downloads_column.block.memory(), memory.output);
        assert_eq!(output_held(&downloads_column.block), memory.output);
        assert_eq!(account.live_bytes(), memory.output + memory.scratch);
        assert_eq!(scratch_held(&decode.decoder.blocks), memory.scratch);
    }

    // What is left holds the small block, so another column's large one does not fit.
    let mut other = Column::<u64>::new(column);
    let error = other.value(&mut decode, 0).unwrap_err();
    assert!(
        matches!(error, SeriesQueryError::ReserveBlock { chunk: id, .. } if id == chunk.0),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(
        (
            other.block.len(),
            other.rows.clone(),
            output_held(&other.block)
        ),
        (0, 0..0, 0)
    );
    drop(other);
    assert_eq!(
        account.live_bytes(),
        small.output + scratch_held(&decode.decoder.blocks)
    );
    assert_eq!(budget.reserved(), large.output + large.scratch);
}

// A query of an old snapshot sees neither later writes nor later chunking (§7.1).
#[test]
fn an_old_snapshot_ignores_later_writes_and_chunking() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let labels: &[&[u8]] = &[b"match", b"curl", b"a.com"];
    write_at(&mut tsdb, 1, 0, &[at(point(labels, &[1, 2]), 0)]);
    let old = tsdb.reader().capture().unwrap();
    let total = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&[TARGET]),
    }];
    let ask = |snapshot: &Snapshot<MemoryStorage>| {
        lists(&snapshot.series(&query(hour(0..3), &total, 0b11)).unwrap())
    };
    let before = ask(&old);

    // A new combination, a bucket chunked, then a late write chunked as a delta.
    let other: &[&[u8]] = &[b"match", b"curl", b"b.com"];
    write_at(
        &mut tsdb,
        2,
        2 * HOUR_MS,
        &[at(point(other, &[5, 5]), HOUR_MS)],
    );
    write_at(
        &mut tsdb,
        3,
        2 * HOUR_MS + 1,
        &[at(point(labels, &[7, 7]), 0)],
    );
    assert_eq!(chunked_buckets(&tsdb), [0, 0]);

    assert_eq!(ask(&old), before);
    let new = tsdb.reader().capture().unwrap();
    let id = |value: &[u8]| KeyPart::Label(label(&new, TARGET, value));
    assert_eq!(
        ask(&new),
        vec![HashMap::from([
            (vec![id(b"a.com")], vec![8, 9]),
            (vec![id(b"b.com")], vec![5, 5]),
        ])]
    );
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::storage::redb::RedbStorage;

    // redb reads every chunk inside the manifest scan that lists it, so nested reads of one
    // snapshot have to work there too; and a reopen reads the same.
    #[test]
    fn a_query_reads_chunks_and_the_head_on_redb_across_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("series.redb");
        let open = || {
            let profile = Profile::desktop();
            let storage = RedbStorage::open(&path, profile.backend_cache).unwrap();
            Tsdb::open(storage, schema(), profile).unwrap()
        };
        let mut tsdb = open();
        let labels: &[&[u8]] = &[b"match", b"curl", b"a.com"];
        let other: &[&[u8]] = &[b"direct", b"git", b"b.com"];
        write_at(&mut tsdb, 1, 0, &[at(point(labels, &[1, 2]), 0)]);
        write_at(&mut tsdb, 2, HOUR_MS, &[at(point(other, &[3, 4]), HOUR_MS)]);
        // Buckets 0 and 1 chunked, then a late delta of bucket 0, and bucket 3 in the head.
        write_at(&mut tsdb, 3, 3 * HOUR_MS, &[]);
        let late = [
            at(point(labels, &[5, 6]), 0),
            at(point(other, &[7, 8]), 3 * HOUR_MS),
        ];
        write_at(&mut tsdb, 4, 3 * HOUR_MS, &late);
        assert_eq!(chunked_buckets(&tsdb), [0, 1, 0]);
        let by_target = [GroupRequest {
            filters: &[],
            key: GroupKey::Labels(&[TARGET]),
        }];
        let ask = |tsdb: &Tsdb<RedbStorage>| {
            let snapshot = tsdb.reader().capture().unwrap();
            let groups = snapshot
                .series(&query(hour(0..4), &by_target, 0b11))
                .unwrap();
            let id = |value: &[u8]| KeyPart::Label(snapshot.find_label(TARGET, value).unwrap());
            (lists(&groups), id(b"a.com"), id(b"b.com"))
        };
        let (found, a, b) = ask(&tsdb);
        let expected = vec![HashMap::from([
            (vec![a], vec![6, 8]),
            (vec![b], vec![10, 12]),
        ])];
        assert_eq!(found, expected);
        drop(tsdb);
        let (found, ..) = ask(&open());
        assert_eq!(found, expected);
    }
}

// The first retained bucket is exact where the watermark's timestamp saturates: past the last
// bucket number there is none, and below the first every bucket is retained.
#[test]
fn the_first_retained_bucket_is_exact_at_the_ends_of_the_timeline() {
    let grid = |bucket, extent| {
        crate::time::Grid::new(
            crate::time::Span::new(bucket).unwrap(),
            crate::time::Span::new(extent).unwrap(),
        )
        .unwrap()
    };
    let hours = grid(HOUR_MS, 24 * HOUR_MS);
    assert_eq!(first_retained_bucket(hours, 0), Some(24));
    assert_eq!(first_retained_bucket(hours, -2), Some(-24));
    // The last extent: its saturated end would name its own last bucket.
    let last = hours.extent_of_bucket(hours.bucket().index_of(Timestamp(i64::MAX)));
    assert_eq!(first_retained_bucket(hours, last), Some((last + 1) * 24));
    let millis = grid(1, 1);
    assert_eq!(first_retained_bucket(millis, i64::MAX - 1), Some(i64::MAX));
    assert_eq!(first_retained_bucket(millis, i64::MAX), None);
    let pairs = grid(1, 2);
    assert_eq!(first_retained_bucket(pairs, i64::MAX / 2), None);
    assert_eq!(first_retained_bucket(pairs, i64::MIN), Some(i64::MIN));
}

#[test]
fn groups_outlive_the_scan_snapshot_and_store() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<SeriesGroups>();
    let (_disk, tsdb) = many_targets(30);
    let snapshot = tsdb.reader().capture().unwrap();
    let budget = snapshot.budget().clone();
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&[TARGET]),
    }];
    let groups = snapshot
        .series(&query(hour(0..1), &requests, 0b11))
        .unwrap();
    let expected = lists(&groups);
    let held = held_by(&groups);
    drop(snapshot);
    drop(tsdb);
    assert_eq!(budget.reserved(), held);
    assert_eq!(lists(&groups), expected);
    assert_eq!(groups.lists()[0].len(), 30);
    drop(groups);
    assert_eq!(budget.reserved(), 0);
}

#[test]
fn finishing_groups_moves_buffers_and_admits_the_list_before_transfer() {
    for fail in [false, true] {
        let budget = MemoryBudget::new(4096);
        let account = MemoryAccount::new(&budget, "test groups", MemoryPlan::default()).unwrap();
        let mut list = GroupBuffer::new(&account, 1, 1).unwrap();
        list.add(&[KeyPart::Label(7)], &[11], &account).unwrap();
        let keys = list.keys.as_ptr();
        let sums = list.sums.as_ptr();
        let mut lists = account.buffer(1).unwrap();
        lists.push(list);
        if fail {
            account.fail_after(0);
        }
        let result = account.finish(GroupsBuffer {
            account: &account,
            lists,
        });
        if fail {
            assert!(result.is_err());
            drop(account);
            assert_eq!(budget.reserved(), 0);
        } else {
            let result = result.unwrap();
            assert_eq!(result.get()[0].keys.as_ptr(), keys);
            assert_eq!(result.get()[0].sums.as_ptr(), sums);
            drop(account);
            let list = &result.get()[0];
            assert_eq!(
                budget.reserved(),
                size_of::<GroupList>() as u64 + list.bytes()
            );
            assert_eq!(list.iter().next().unwrap().sums, &[11]);
            drop(result);
            assert_eq!(budget.reserved(), 0);
        }
    }
}
