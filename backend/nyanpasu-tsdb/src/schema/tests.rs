use super::*;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

const RULE: LabelId = LabelId(0);
const CHAIN: LabelId = LabelId(1);
const TARGET: LabelId = LabelId(2);
const SUBSCRIPTION: LabelId = LabelId(3);

fn column(name: &'static str, ty: ColumnType) -> ColumnSpec {
    ColumnSpec {
        name,
        ty,
        nullable: false,
    }
}

fn nullable(name: &'static str, ty: ColumnType) -> ColumnSpec {
    ColumnSpec {
        nullable: true,
        ..column(name, ty)
    }
}

fn field(name: &'static str) -> FieldSpec {
    FieldSpec {
        name,
        aggregate: Aggregate::SaturatingSum,
    }
}

fn tier(name: &'static str, bucket: Duration, extent: Duration) -> TierSpec {
    TierSpec {
        name,
        bucket,
        extent,
    }
}

fn labels(names: &[&'static str]) -> Vec<LabelSpec> {
    names.iter().map(|&name| LabelSpec { name }).collect()
}

/// The shape of the traffic dataset (§11.1) next to a core-log stream (§11.2): every
/// kind of column, a fold ladder, and both kinds of extent.
fn schema() -> Schema {
    Schema {
        version: 1,
        labels: labels(&["rule", "chain", "target", "subscription"]),
        series: Some(SeriesSpec {
            dimensions: vec![RULE, CHAIN, TARGET],
            fields: vec![field("upload"), field("download"), field("connections")],
            tiers: vec![tier("minute", MINUTE, HOUR), tier("hour", HOUR, DAY)],
            fold: vec![vec![TARGET]],
        }),
        events: vec![
            EventStreamSpec {
                name: "closed",
                key: EventKey::Timestamp,
                extent: ExtentSpec::Window(DAY),
                columns: vec![
                    column("closed_at", ColumnType::Timestamp),
                    column("started_at", ColumnType::Timestamp),
                    column("upload", ColumnType::U64),
                    column("download", ColumnType::U64),
                    column("series", ColumnType::Series),
                    column("connection", ColumnType::Identity),
                    nullable("folded_target", ColumnType::FoldedLabel(TARGET)),
                    nullable("subscription", ColumnType::Label(SUBSCRIPTION)),
                ],
            },
            EventStreamSpec {
                name: "core_log",
                key: EventKey::Sequence,
                extent: ExtentSpec::Epoch,
                columns: vec![
                    column("seq", ColumnType::U64),
                    column("received_at", ColumnType::Timestamp),
                    nullable("raw_time", ColumnType::Text),
                    column("log_type", ColumnType::Dict),
                    column("level", ColumnType::Fixed(1)),
                    column("delta", ColumnType::I64),
                    column("body", ColumnType::Text),
                ],
            },
        ],
    }
}

fn rejected(mutate: impl FnOnce(&mut Schema)) -> SchemaError {
    let mut schema = schema();
    mutate(&mut schema);
    schema.validate().unwrap_err()
}

fn closed(schema: &mut Schema) -> &mut EventStreamSpec {
    &mut schema.events[0]
}

fn series(schema: &mut Schema) -> &mut SeriesSpec {
    schema.series.as_mut().unwrap()
}

/// `count` distinct names. They are leaked, which a test can afford.
fn names(count: usize) -> Vec<&'static str> {
    (0..count)
        .map(|index| &*Box::leak(format!("n{index}").into_boxed_str()))
        .collect()
}

// --- validation: what is accepted

#[test]
fn the_example_schema_is_valid() {
    assert_eq!(schema().validate(), Ok(()));
}

#[test]
fn a_schema_may_be_only_events_or_only_a_series_or_empty() {
    let mut only_events = schema();
    only_events.series = None;
    only_events.events.remove(0);
    only_events.labels.clear();
    assert_eq!(only_events.validate(), Ok(()));

    let mut only_series = schema();
    only_series.events.clear();
    assert_eq!(only_series.validate(), Ok(()));

    let nothing = Schema {
        version: 0,
        labels: Vec::new(),
        series: None,
        events: Vec::new(),
    };
    assert_eq!(nothing.validate(), Ok(()));
}

#[test]
fn a_name_is_unique_only_within_its_scope() {
    let mut schema = schema();
    // A label, a field, a tier and a stream share a name; the streams share a column
    // name.
    schema.labels[0].name = "x";
    series(&mut schema).fields[0].name = "x";
    series(&mut schema).tiers[0].name = "x";
    schema.events[0].name = "x";
    schema.events[0].columns[0].name = "same";
    schema.events[1].columns[0].name = "same";
    assert_eq!(schema.validate(), Ok(()));
}

// --- validation: names

#[test]
fn names_may_not_be_empty() {
    assert_eq!(
        rejected(|s| s.labels[1].name = ""),
        SchemaError::EmptyName {
            scope: NameScope::Label,
            index: 1
        }
    );
    assert_eq!(
        rejected(|s| series(s).fields[2].name = ""),
        SchemaError::EmptyName {
            scope: NameScope::Field,
            index: 2
        }
    );
    assert_eq!(
        rejected(|s| series(s).tiers[1].name = ""),
        SchemaError::EmptyName {
            scope: NameScope::Tier,
            index: 1
        }
    );
    assert_eq!(
        rejected(|s| s.events[1].name = ""),
        SchemaError::EmptyName {
            scope: NameScope::Stream,
            index: 1
        }
    );
    assert_eq!(
        rejected(|s| closed(s).columns[3].name = ""),
        SchemaError::EmptyName {
            scope: NameScope::Column { stream: "closed" },
            index: 3
        }
    );
}

#[test]
fn names_may_not_repeat_within_their_scope() {
    assert_eq!(
        rejected(|s| s.labels[3].name = "rule"),
        SchemaError::DuplicateName {
            scope: NameScope::Label,
            name: "rule",
            first: 0,
            second: 3
        }
    );
    assert_eq!(
        rejected(|s| series(s).fields[2].name = "upload"),
        SchemaError::DuplicateName {
            scope: NameScope::Field,
            name: "upload",
            first: 0,
            second: 2
        }
    );
    assert_eq!(
        rejected(|s| series(s).tiers[1].name = "minute"),
        SchemaError::DuplicateName {
            scope: NameScope::Tier,
            name: "minute",
            first: 0,
            second: 1
        }
    );
    assert_eq!(
        rejected(|s| s.events[1].name = "closed"),
        SchemaError::DuplicateName {
            scope: NameScope::Stream,
            name: "closed",
            first: 0,
            second: 1
        }
    );
    assert_eq!(
        rejected(|s| closed(s).columns[5].name = "upload"),
        SchemaError::DuplicateName {
            scope: NameScope::Column { stream: "closed" },
            name: "upload",
            first: 2,
            second: 5
        }
    );
}

// --- validation: labels and dimensions

#[test]
fn labels_are_limited_to_what_a_label_id_can_number() {
    let with_labels = |count| Schema {
        version: 0,
        labels: names(count)
            .into_iter()
            .map(|name| LabelSpec { name })
            .collect(),
        series: None,
        events: Vec::new(),
    };
    assert_eq!(with_labels(MAX_LABELS).validate(), Ok(()));
    assert_eq!(
        with_labels(MAX_LABELS + 1).validate(),
        Err(SchemaError::TooManyLabels {
            count: MAX_LABELS + 1
        })
    );
}

#[test]
fn dimensions_are_non_empty_unique_and_in_range() {
    assert_eq!(
        rejected(|s| series(s).dimensions.clear()),
        SchemaError::NoDimensions
    );
    assert_eq!(
        rejected(|s| series(s).dimensions = vec![RULE, TARGET, RULE]),
        SchemaError::DuplicateDimension {
            label: RULE,
            first: 0,
            second: 2
        }
    );
    assert_eq!(
        rejected(|s| series(s).dimensions = vec![RULE, LabelId(4)]),
        SchemaError::DimensionOutOfRange {
            index: 1,
            label: LabelId(4),
            labels: 4
        }
    );
}

#[test]
fn a_series_has_at_most_sixteen_dimensions() {
    let dimensions = |count: usize| {
        let mut schema = schema();
        schema.labels = labels(&names(count));
        series(&mut schema).dimensions = (0..count as u16).map(LabelId).collect();
        schema.validate()
    };
    assert_eq!(dimensions(MAX_DIMENSIONS), Ok(()));
    assert_eq!(
        dimensions(MAX_DIMENSIONS + 1),
        Err(SchemaError::TooManyDimensions {
            count: MAX_DIMENSIONS + 1
        })
    );
}

// --- validation: the series

#[test]
fn a_series_has_one_to_sixteen_fields() {
    let fields = |count| {
        let mut schema = schema();
        series(&mut schema).fields = names(count).into_iter().map(field).collect();
        schema.validate()
    };
    assert_eq!(fields(0), Err(SchemaError::FieldCount { count: 0 }));
    assert_eq!(fields(1), Ok(()));
    assert_eq!(fields(MAX_FIELDS), Ok(()));
    assert_eq!(
        fields(MAX_FIELDS + 1),
        Err(SchemaError::FieldCount {
            count: MAX_FIELDS + 1
        })
    );
}

#[test]
fn a_series_has_one_to_255_tiers() {
    let tiers = |count| {
        let mut schema = schema();
        series(&mut schema).tiers = names(count)
            .into_iter()
            .map(|name| tier(name, MINUTE, HOUR))
            .collect();
        schema.validate()
    };
    assert_eq!(tiers(0), Err(SchemaError::TierCount { count: 0 }));
    assert_eq!(tiers(1), Ok(()));
    assert_eq!(tiers(MAX_TIERS), Ok(()));
    assert_eq!(
        tiers(MAX_TIERS + 1),
        Err(SchemaError::TierCount {
            count: MAX_TIERS + 1
        })
    );
}

#[test]
fn a_bucket_is_a_positive_whole_number_of_milliseconds() {
    let bucket = |bucket| {
        rejected(|s| {
            series(s).tiers[0] = tier("minute", bucket, HOUR);
        })
    };
    assert_eq!(
        bucket(Duration::ZERO),
        SchemaError::TierBucket {
            tier: "minute",
            source: DurationError::Zero
        }
    );
    let fine = Duration::from_micros(60_500);
    assert_eq!(
        bucket(fine),
        SchemaError::TierBucket {
            tier: "minute",
            source: DurationError::SubMillisecond { duration: fine }
        }
    );
    assert_eq!(bucket(fine).kind(), ErrorKind::Local);
}

#[test]
fn an_extent_is_a_positive_whole_multiple_of_its_bucket() {
    let extent = |extent| {
        rejected(|s| {
            series(s).tiers[0] = tier("minute", MINUTE, extent);
        })
    };
    assert_eq!(
        extent(Duration::ZERO),
        SchemaError::TierExtent {
            tier: "minute",
            source: DurationError::Zero
        }
    );
    let fine = Duration::from_nanos(1_000_000_001);
    assert_eq!(
        extent(fine),
        SchemaError::TierExtent {
            tier: "minute",
            source: DurationError::SubMillisecond { duration: fine }
        }
    );
    // Not a multiple: longer, and shorter, than the bucket.
    for not_a_multiple in [Duration::from_secs(90), Duration::from_secs(30)] {
        assert_eq!(
            extent(not_a_multiple),
            SchemaError::ExtentNotMultiple {
                tier: "minute",
                bucket_ms: 60_000,
                extent_ms: not_a_multiple.as_millis() as i64
            }
        );
    }
    // An extent of exactly one bucket is a whole number of buckets.
    let mut one_bucket = schema();
    series(&mut one_bucket).tiers[0] = tier("minute", MINUTE, MINUTE);
    assert_eq!(one_bucket.validate(), Ok(()));
}

// --- validation: the fold ladder

#[test]
fn a_fold_ladder_grows_without_reaching_the_whole_combination() {
    let ladder = |fold: Vec<Vec<LabelId>>| {
        let mut schema = schema();
        series(&mut schema).fold = fold;
        // The folded-label column needs `TARGET` folded; this test is about the ladder.
        closed(&mut schema).columns.remove(6);
        schema.validate()
    };
    assert_eq!(ladder(vec![]), Ok(()));
    assert_eq!(ladder(vec![vec![TARGET]]), Ok(()));
    assert_eq!(ladder(vec![vec![TARGET], vec![CHAIN, TARGET]]), Ok(()));
    // The order within a step does not matter.
    assert_eq!(ladder(vec![vec![TARGET], vec![TARGET, CHAIN]]), Ok(()));
    assert_eq!(ladder(vec![vec![RULE, CHAIN]]), Ok(()));
}

#[test]
fn a_fold_step_is_a_non_empty_set_of_dimensions() {
    let step = |fold: Vec<Vec<LabelId>>| {
        let mut schema = schema();
        series(&mut schema).fold = fold;
        schema.validate().unwrap_err()
    };
    assert_eq!(
        step(vec![vec![TARGET], vec![]]),
        SchemaError::FoldStepEmpty { step: 1 }
    );
    assert_eq!(
        step(vec![vec![TARGET, CHAIN, TARGET]]),
        SchemaError::FoldStepDuplicate {
            step: 0,
            label: TARGET
        }
    );
    // The label exists, but the series does not use it as a dimension.
    assert_eq!(
        step(vec![vec![TARGET], vec![TARGET, SUBSCRIPTION]]),
        SchemaError::FoldStepNotDimension {
            step: 1,
            label: SUBSCRIPTION
        }
    );
    assert_eq!(
        step(vec![vec![LabelId(99)]]),
        SchemaError::FoldStepNotDimension {
            step: 0,
            label: LabelId(99)
        }
    );
}

#[test]
fn each_fold_step_folds_strictly_more_than_the_one_before() {
    let step = |fold: Vec<Vec<LabelId>>| {
        let mut schema = schema();
        series(&mut schema).fold = fold;
        schema.validate().unwrap_err()
    };
    // The same set again, a smaller one, and sets that do not contain the one before.
    for fold in [
        vec![vec![TARGET], vec![TARGET]],
        vec![vec![TARGET], vec![CHAIN, TARGET], vec![TARGET]],
        vec![vec![TARGET], vec![CHAIN]],
        vec![vec![RULE, CHAIN], vec![CHAIN, TARGET]],
        // Larger, but not containing the step before it.
        vec![vec![TARGET], vec![RULE, CHAIN]],
    ] {
        let last = fold.len() - 1;
        assert_eq!(
            step(fold.clone()),
            SchemaError::FoldStepNotSuperset { step: last },
            "{fold:?}"
        );
    }
}

#[test]
fn no_fold_step_folds_every_dimension() {
    let step = |fold: Vec<Vec<LabelId>>| {
        let mut schema = schema();
        series(&mut schema).fold = fold;
        schema.validate().unwrap_err()
    };
    assert_eq!(
        step(vec![vec![RULE, CHAIN, TARGET]]),
        SchemaError::FoldStepFoldsAll { step: 0 }
    );
    assert_eq!(
        step(vec![vec![TARGET], vec![TARGET, RULE, CHAIN]]),
        SchemaError::FoldStepFoldsAll { step: 1 }
    );
}

// --- validation: event streams

#[test]
fn there_are_at_most_255_streams() {
    let streams = |count| {
        let mut schema = schema();
        schema.events = names(count)
            .into_iter()
            .map(|name| EventStreamSpec {
                name,
                key: EventKey::Sequence,
                extent: ExtentSpec::Epoch,
                columns: vec![column("seq", ColumnType::U64)],
            })
            .collect();
        schema.validate()
    };
    assert_eq!(streams(MAX_STREAMS), Ok(()));
    assert_eq!(
        streams(MAX_STREAMS + 1),
        Err(SchemaError::TooManyStreams {
            count: MAX_STREAMS + 1
        })
    );
}

#[test]
fn a_stream_has_one_to_61_columns() {
    let columns = |count| {
        let mut schema = schema();
        closed(&mut schema).columns = names(count)
            .into_iter()
            .map(|name| column(name, ColumnType::U64))
            .collect();
        schema.validate()
    };
    assert_eq!(
        columns(0),
        Err(SchemaError::ColumnCount {
            stream: "closed",
            count: 0
        })
    );
    assert_eq!(columns(1), Ok(()));
    assert_eq!(columns(MAX_COLUMNS), Ok(()));
    assert_eq!(
        columns(MAX_COLUMNS + 1),
        Err(SchemaError::ColumnCount {
            stream: "closed",
            count: MAX_COLUMNS + 1
        })
    );
}

#[test]
fn a_stream_has_at_most_one_series_column() {
    assert_eq!(
        rejected(|s| closed(s).columns.push(column("again", ColumnType::Series))),
        SchemaError::SecondSeriesColumn {
            stream: "closed",
            first: "series",
            second: "again"
        }
    );
    // One in each of two streams is fine.
    let mut two_streams = schema();
    two_streams.events[1]
        .columns
        .push(column("series", ColumnType::Series));
    assert_eq!(two_streams.validate(), Ok(()));
}

#[test]
fn a_series_column_needs_a_series() {
    let mut schema = schema();
    schema.series = None;
    schema.events.remove(1);
    assert_eq!(
        schema.validate(),
        Err(SchemaError::SeriesColumnWithoutSeries {
            stream: "closed",
            column: "series"
        })
    );
}

#[test]
fn a_folded_label_column_is_nullable_and_keeps_a_dimension_of_a_series_column() {
    // No series column in the stream.
    assert_eq!(
        rejected(|s| {
            closed(s).columns.remove(4);
        }),
        SchemaError::FoldedLabelWithoutSeriesColumn {
            stream: "closed",
            column: "folded_target"
        }
    );
    // The column is null unless the dimension was folded.
    assert_eq!(
        rejected(|s| closed(s).columns[6].nullable = false),
        SchemaError::FoldedLabelNotNullable {
            stream: "closed",
            column: "folded_target"
        }
    );
    // A label that is not a dimension of the series.
    assert_eq!(
        rejected(|s| closed(s).columns[6].ty = ColumnType::FoldedLabel(SUBSCRIPTION)),
        SchemaError::FoldedLabelNotDimension {
            stream: "closed",
            column: "folded_target",
            label: SUBSCRIPTION
        }
    );
    // A label that is not even a label.
    assert_eq!(
        rejected(|s| closed(s).columns[6].ty = ColumnType::FoldedLabel(LabelId(9))),
        SchemaError::ColumnLabelOutOfRange {
            stream: "closed",
            column: "folded_target",
            label: LabelId(9),
            labels: 4
        }
    );
    // The series column may come after the folded-label column.
    let mut reordered = schema();
    closed(&mut reordered).columns.swap(4, 6);
    assert_eq!(reordered.validate(), Ok(()));
    // A label folded by a later step counts.
    let mut later = schema();
    series(&mut later).fold = vec![vec![TARGET], vec![CHAIN, TARGET]];
    closed(&mut later).columns[6].ty = ColumnType::FoldedLabel(CHAIN);
    assert_eq!(later.validate(), Ok(()));
    // The implied last step folds every dimension, so a dimension that no explicit step
    // lists counts too, with a ladder or without one.
    let mut implied = schema();
    closed(&mut implied).columns[6].ty = ColumnType::FoldedLabel(RULE);
    assert_eq!(implied.validate(), Ok(()));
    series(&mut implied).fold = Vec::new();
    assert_eq!(implied.validate(), Ok(()));
    closed(&mut implied).columns[6].ty = ColumnType::FoldedLabel(TARGET);
    assert_eq!(implied.validate(), Ok(()));
}

#[test]
fn a_label_column_names_a_label() {
    assert_eq!(
        rejected(|s| closed(s).columns[7].ty = ColumnType::Label(LabelId(4))),
        SchemaError::ColumnLabelOutOfRange {
            stream: "closed",
            column: "subscription",
            label: LabelId(4),
            labels: 4
        }
    );
    let mut last = schema();
    closed(&mut last).columns[7].ty = ColumnType::Label(LabelId(3));
    assert_eq!(last.validate(), Ok(()));
}

#[test]
fn a_fixed_column_is_one_to_1024_bytes() {
    let fixed = |width| {
        let mut schema = schema();
        schema.events[1].columns[4].ty = ColumnType::Fixed(width);
        schema.validate()
    };
    assert_eq!(fixed(1), Ok(()));
    assert_eq!(fixed(MAX_FIXED_WIDTH), Ok(()));
    for width in [0, MAX_FIXED_WIDTH + 1, u16::MAX] {
        assert_eq!(
            fixed(width),
            Err(SchemaError::FixedWidth {
                stream: "core_log",
                column: "level",
                width
            })
        );
    }
}

#[test]
fn a_time_window_needs_a_timestamp_key_and_a_whole_millisecond_width() {
    assert_eq!(
        rejected(|s| closed(s).key = EventKey::Sequence),
        SchemaError::WindowNeedsTimestampKey { stream: "closed" }
    );
    assert_eq!(
        rejected(|s| closed(s).extent = ExtentSpec::Window(Duration::ZERO)),
        SchemaError::StreamWindow {
            stream: "closed",
            source: DurationError::Zero
        }
    );
    let fine = Duration::from_micros(1);
    let error = rejected(|s| closed(s).extent = ExtentSpec::Window(fine));
    assert_eq!(
        error,
        SchemaError::StreamWindow {
            stream: "closed",
            source: DurationError::SubMillisecond { duration: fine }
        }
    );
    assert_eq!(error.kind(), ErrorKind::Local);
}

#[test]
fn an_epoch_needs_a_sequence_key() {
    assert_eq!(
        rejected(|s| s.events[1].key = EventKey::Timestamp),
        SchemaError::EpochNeedsSequenceKey { stream: "core_log" }
    );
}

#[test]
fn errors_say_what_is_wrong_and_where() {
    let error = rejected(|s| series(s).tiers[0] = tier("minute", MINUTE, Duration::from_secs(90)));
    assert_eq!(
        error.to_string(),
        "extent of tier `minute` (90000 ms) is not a whole number of its buckets (60000 ms)"
    );
    let error = rejected(|s| closed(s).columns[5].name = "upload");
    assert_eq!(
        error.to_string(),
        "column of event stream `closed` name `upload` is used at index 2 and again at \
         index 5"
    );
    let error = rejected(|s| series(s).tiers[0] = tier("minute", Duration::ZERO, HOUR));
    assert_eq!(
        error.to_string(),
        "bucket of tier `minute`: duration is zero"
    );
}

// --- fingerprint

#[test]
fn fnv1a_matches_the_published_test_vectors() {
    // From the FNV reference test suite (isthe.com/chongo/src/fnv/test_fnv.c).
    let hash = |text: &str| {
        let mut hash = Fnv1a::new();
        hash.bytes(text.as_bytes());
        hash.finish()
    };
    assert_eq!(hash(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(hash("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(hash("b"), 0xaf63_df4c_8601_f1a5);
    assert_eq!(hash("foobar"), 0x8594_4171_f739_67e8);
}

/// A valid schema that takes every step of the encoding once: a multi-byte varint, an
/// `Option`, a `bool`, a nested sequence, an enum variant with a payload, and durations.
fn tiny() -> Schema {
    Schema {
        version: 300,
        labels: labels(&["ab", "c"]),
        series: Some(SeriesSpec {
            dimensions: vec![LabelId(0), LabelId(1)],
            fields: vec![field("n")],
            tiers: vec![tier(
                "t",
                Duration::from_millis(1500),
                Duration::from_secs(3),
            )],
            fold: vec![vec![LabelId(1)]],
        }),
        events: vec![EventStreamSpec {
            name: "e",
            key: EventKey::Sequence,
            extent: ExtentSpec::Epoch,
            columns: vec![nullable("c", ColumnType::Fixed(300))],
        }],
    }
}

/// The postcard encoding of [`tiny`], written out by hand from the wire specification
/// (<https://postcard.jamesmunns.com/wire-format>), not produced by running postcard. In it,
/// an integer is a varint (7 bits per byte, least significant first, the high bit set while
/// more follow), a string or a sequence is its length and then its content, an `Option` is
/// 0 or 1 and then the value, a `bool` is 0 or 1, a struct is its fields in declaration
/// order, an enum variant is its position in the declaration and then its payload, and a
/// `Duration` is serde's struct of whole seconds (`u64`) and subsecond nanoseconds (`u32`).
fn tiny_encoding() -> Vec<u8> {
    [
        // version: 300 = 0b10_0101100
        &[0xac, 0x02][..],
        // labels: 2 of them, "ab" and "c"
        &[0x02, 0x02, b'a', b'b', 0x01, b'c'],
        // series: Some
        &[0x01],
        // dimensions: [0, 1]
        &[0x02, 0x00, 0x01],
        // fields: 1, named "n", aggregate SaturatingSum (variant 0)
        &[0x01, 0x01, b'n', 0x00],
        // tiers: 1, named "t"
        &[0x01, 0x01, b't'],
        // bucket 1500 ms = 1 s and 500_000_000 ns; 500_000_000 = 0b1_1101110_0110101_1001010_0000000
        &[0x01, 0x80, 0xca, 0xb5, 0xee, 0x01],
        // extent 3 s and 0 ns
        &[0x03, 0x00],
        // fold: 1 step of 1 label, label 1
        &[0x01, 0x01, 0x01],
        // events: 1, named "e", key Sequence (variant 1)
        &[0x01, 0x01, b'e', 0x01],
        // columns: 1, named "c", type Fixed (variant 6) of 300 bytes, nullable
        &[0x01, 0x01, b'c', 0x06, 0xac, 0x02, 0x01],
        // extent Epoch (variant 1)
        &[0x01],
    ]
    .concat()
}

#[test]
fn the_derived_encoding_is_postcards_wire_format() {
    tiny().validate().unwrap();
    assert_eq!(postcard::to_stdvec(&tiny()).unwrap(), tiny_encoding());
}

// Both literals were computed by an independent implementation of the postcard wire
// format and of FNV-1a (a short script written from the two specifications), not by
// running this code or the postcard crate.
#[test]
fn the_fingerprint_hashes_the_version_byte_and_the_encoding() {
    let mut expected = Fnv1a::new();
    expected.bytes(&[FINGERPRINT_ENCODING]);
    expected.bytes(&tiny_encoding());
    assert_eq!(tiny().fingerprint(), expected.finish());
    assert_eq!(tiny().fingerprint(), 0x7ac2_0af9_7591_e31f);
}

// The value is stored in datasets and chunk headers, so it must never change: if this
// test fails, the encoding changed and every stored dataset would be refused.
#[test]
fn the_fingerprint_of_the_example_schema_is_pinned() {
    assert_eq!(schema().fingerprint(), GOLDEN_FINGERPRINT);
}

const GOLDEN_FINGERPRINT: u64 = 0xaa8c_5d42_097c_dce4;

#[test]
fn the_fingerprint_depends_on_the_content_of_names_not_their_addresses() {
    let mut other = schema();
    let copy: &'static str = Box::leak(String::from("rule").into_boxed_str());
    assert!(!std::ptr::eq(copy, other.labels[0].name));
    other.labels[0].name = copy;
    assert_eq!(other.fingerprint(), schema().fingerprint());
    assert_eq!(schema().fingerprint(), schema().fingerprint());
}

/// Every case changes exactly one structural thing, and every case has to produce its
/// own fingerprint: a field left out of the encoding, or two encodings that run
/// together, would make two of them collide.
#[test]
fn changing_any_structural_field_changes_the_fingerprint() {
    type Mutation = fn(&mut Schema);
    let cases: &[(&str, Mutation)] = &[
        ("version", |s| s.version = 2),
        ("label renamed", |s| s.labels[0].name = "rule2"),
        ("labels swapped", |s| s.labels.swap(0, 1)),
        ("label added", |s| s.labels.push(LabelSpec { name: "more" })),
        ("label removed", |s| {
            s.labels.pop();
        }),
        // The same letters in the same order, with the boundary between names moved.
        ("label boundary", |s| {
            s.labels = labels(&["rulec", "hain", "target", "subscription"])
        }),
        ("no series", |s| s.series = None),
        ("dimension order", |s| series(s).dimensions.swap(0, 1)),
        ("dimension removed", |s| {
            series(s).dimensions.pop();
        }),
        ("dimension replaced", |s| {
            series(s).dimensions[2] = SUBSCRIPTION
        }),
        ("field renamed", |s| series(s).fields[0].name = "up"),
        ("fields swapped", |s| series(s).fields.swap(0, 1)),
        ("field added", |s| series(s).fields.push(field("errors"))),
        ("field removed", |s| {
            series(s).fields.pop();
        }),
        ("tier renamed", |s| series(s).tiers[0].name = "min"),
        ("tiers swapped", |s| series(s).tiers.swap(0, 1)),
        ("tier added", |s| {
            series(s).tiers.push(tier("day", DAY, DAY))
        }),
        ("tier removed", |s| {
            series(s).tiers.pop();
        }),
        ("bucket", |s| {
            series(s).tiers[0].bucket = Duration::from_secs(30)
        }),
        ("bucket by a millisecond", |s| {
            series(s).tiers[0].bucket = Duration::from_millis(60_001)
        }),
        ("extent", |s| series(s).tiers[0].extent = DAY),
        ("fold step label", |s| series(s).fold[0] = vec![CHAIN]),
        ("fold step added", |s| {
            series(s).fold.push(vec![CHAIN, TARGET])
        }),
        ("fold step grown", |s| {
            series(s).fold[0] = vec![TARGET, CHAIN]
        }),
        ("fold step grown the other way", |s| {
            series(s).fold[0] = vec![CHAIN, TARGET]
        }),
        ("fold removed", |s| series(s).fold.clear()),
        ("fold step split", |s| {
            series(s).fold = vec![vec![TARGET], vec![]]
        }),
        ("stream removed", |s| {
            s.events.pop();
        }),
        ("streams swapped", |s| s.events.swap(0, 1)),
        ("stream added", |s| {
            let mut extra = s.events[1].clone();
            extra.name = "extra";
            s.events.push(extra);
        }),
        ("stream renamed", |s| s.events[0].name = "shut"),
        ("key", |s| s.events[0].key = EventKey::Sequence),
        ("window", |s| {
            s.events[0].extent = ExtentSpec::Window(Duration::from_secs(2 * 24 * 60 * 60))
        }),
        ("window by a millisecond", |s| {
            s.events[0].extent = ExtentSpec::Window(DAY + Duration::from_millis(1))
        }),
        ("epoch", |s| s.events[0].extent = ExtentSpec::Epoch),
        ("window after epoch", |s| {
            s.events[1].extent = ExtentSpec::Window(DAY)
        }),
        ("column renamed", |s| {
            s.events[0].columns[0].name = "shut_at"
        }),
        ("columns swapped", |s| s.events[0].columns.swap(0, 1)),
        ("column removed", |s| {
            s.events[0].columns.pop();
        }),
        ("column added", |s| {
            s.events[0].columns.push(column("extra", ColumnType::U64))
        }),
        ("nullable", |s| s.events[0].columns[0].nullable = true),
        ("not nullable", |s| s.events[0].columns[6].nullable = false),
        ("type I64", |s| s.events[0].columns[0].ty = ColumnType::I64),
        ("type U64", |s| s.events[0].columns[0].ty = ColumnType::U64),
        ("type Series", |s| {
            s.events[0].columns[0].ty = ColumnType::Series
        }),
        ("type Label 0", |s| {
            s.events[0].columns[0].ty = ColumnType::Label(LabelId(0))
        }),
        ("type Label 1", |s| {
            s.events[0].columns[0].ty = ColumnType::Label(LabelId(1))
        }),
        ("type Dict", |s| {
            s.events[0].columns[0].ty = ColumnType::Dict
        }),
        ("type Fixed 4", |s| {
            s.events[0].columns[0].ty = ColumnType::Fixed(4)
        }),
        ("type Fixed 8", |s| {
            s.events[0].columns[0].ty = ColumnType::Fixed(8)
        }),
        ("type Identity", |s| {
            s.events[0].columns[0].ty = ColumnType::Identity
        }),
        ("type Text", |s| {
            s.events[0].columns[0].ty = ColumnType::Text
        }),
        ("type FoldedLabel 0", |s| {
            s.events[0].columns[0].ty = ColumnType::FoldedLabel(LabelId(0))
        }),
        ("type FoldedLabel 1", |s| {
            s.events[0].columns[0].ty = ColumnType::FoldedLabel(LabelId(1))
        }),
    ];

    let mut seen = HashMap::from([(schema().fingerprint(), "unchanged")]);
    for &(name, mutate) in cases {
        let mut changed = schema();
        mutate(&mut changed);
        let fingerprint = changed.fingerprint();
        if let Some(other) = seen.insert(fingerprint, name) {
            panic!("`{name}` and `{other}` share the fingerprint {fingerprint:#018x}");
        }
    }
}
