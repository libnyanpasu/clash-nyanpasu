//! Seeded caller-owned inputs shared by the binary and ignored charged-memory measurements.

use super::tsdb::{
    self,
    storage::{Durability, Storage},
    *,
};
use std::time::Duration;

/// All timestamps are milliseconds; the workload represents one simulated day.
pub const DAY: i64 = 86_400_000;
/// Reproducible default, printed in every CSV row.
pub const SEED: u64 = 0x4e59_414e_5041_5355;

/// A scale changes cardinality, never profile presets.
#[derive(Clone, Copy, Debug)]
pub enum Scale {
    /// Tiny self-check, with every scenario still exercised.
    Smoke,
    /// 90k combinations and 160k hourly inputs.
    S,
    /// 240k combinations and 590k hourly inputs.
    M,
    /// 600k combinations and 2.54m hourly inputs.
    L,
}
impl Scale {
    /// Returns the declared combinations and hourly input rows.
    pub fn sizes(self) -> (usize, usize) {
        match self {
            Self::Smoke => (24, 96),
            Self::S => (90_000, 160_000),
            Self::M => (240_000, 590_000),
            Self::L => (600_000, 2_540_000),
        }
    }
    /// Stable CSV spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
        }
    }
}

/// Parameters of one generator; shapes use the same schema and generation path.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Synthetic approximation's CSV label.
    pub name: &'static str,
    /// Distinct input series labels, independent of the catalog's admission decision.
    pub combinations: usize,
    /// Input points across one day.
    pub points: usize,
    /// Input event rows across one day.
    pub events: usize,
    /// Series bucket width: minute or hour.
    pub minute: bool,
    /// Bytes of each non-null text, including the large-value case.
    pub text_bytes: usize,
    /// Arrival delay, without changing the row's event time.
    pub late_ms: i64,
    /// Force overflow with a tiny, explicitly reported catalog budget.
    pub full_catalog: bool,
    /// Pin the first combination while subsequent input introduces new labels.
    pub churn: bool,
    /// Keep a snapshot during subsequent writes and maintenance.
    pub retained: bool,
    /// Delay maintenance until the day's inputs finish.
    pub stalled: bool,
}
impl Shape {
    /// Minimal shape for answer and CSV checks on both engines.
    pub fn tiny() -> Self {
        Self {
            name: "hourly",
            combinations: 24,
            points: 96,
            events: 32,
            minute: false,
            text_bytes: 24,
            late_ms: 0,
            full_catalog: false,
            churn: false,
            retained: false,
            stalled: false,
        }
    }
    /// Every requested shape, parameterized rather than separate fixture implementations.
    pub fn all(scale: Scale) -> Vec<Self> {
        let (combinations, points) = scale.sizes();
        let core = Self {
            combinations,
            points,
            events: 0,
            ..Self::tiny()
        };
        let small = matches!(scale, Scale::Smoke);
        let event = Self {
            combinations: if small { 24 } else { 1297 },
            points: 0,
            events: if small { 32 } else { 20_000 },
            ..core
        };
        vec![
            core,
            Self {
                name: "sparse-desktop",
                combinations: if small { 24 } else { 1297 },
                points: if small { 96 } else { 24 * 58 },
                ..core
            },
            Self {
                name: "minute",
                minute: true,
                ..core
            },
            Self {
                name: "closed-20k",
                ..event
            },
            Self {
                name: "closed-50k",
                events: if small { 32 } else { 50_000 },
                ..event
            },
            Self {
                name: "gateway-200k",
                events: if small { 32 } else { 200_000 },
                ..event
            },
            Self {
                name: "text-nullable",
                text_bytes: 512,
                ..event
            },
            Self {
                name: "large-value",
                events: if small { 4 } else { 64 },
                text_bytes: 1024 * 1024,
                ..event
            },
            Self {
                name: "late-30s",
                late_ms: 30_000,
                ..core
            },
            Self {
                name: "late-7d",
                late_ms: 7 * DAY,
                ..core
            },
            Self {
                name: "churn-pinned",
                churn: true,
                ..event
            },
            Self {
                name: "full-catalog",
                combinations: 4096,
                events: if small { 4096 } else { 20_000 },
                full_catalog: true,
                ..event
            },
            Self {
                name: "stalled-maintenance",
                stalled: true,
                ..core
            },
            Self {
                name: "retained-reader",
                points: if small { 16 * 256 } else { core.points },
                retained: true,
                ..core
            },
        ]
    }
    /// Immutable synthetic schema; text, null, identity and mirrored series share one event.
    pub fn schema(self) -> Schema {
        Schema {
            version: 1,
            labels: vec![LabelSpec {
                name: "combination",
            }],
            series: Some(SeriesSpec {
                dimensions: vec![LabelId(0)],
                fields: vec![FieldSpec {
                    name: "sum",
                    aggregate: Aggregate::SaturatingSum,
                }],
                tiers: vec![TierSpec {
                    name: if self.minute { "minute" } else { "hour" },
                    bucket: Duration::from_secs(if self.minute { 60 } else { 3600 }),
                    extent: Duration::from_secs(86400),
                }],
                fold: vec![],
            }),
            events: vec![EventStreamSpec {
                name: "events",
                key: EventKey::Timestamp,
                columns: vec![
                    ColumnSpec {
                        name: "series",
                        ty: ColumnType::Series,
                        nullable: false,
                    },
                    ColumnSpec {
                        name: "text",
                        ty: ColumnType::Text,
                        nullable: true,
                    },
                    ColumnSpec {
                        name: "identity",
                        ty: ColumnType::Identity,
                        nullable: false,
                    },
                ],
                extent: ExtentSpec::Window(Duration::from_secs(86400)),
            }],
        }
    }
    /// Explicit experiment override; no preset is changed.
    pub fn profile(self) -> Profile {
        Profile {
            catalog: if self.full_catalog {
                64 * 1024
            } else {
                Profile::desktop().catalog
            },
            durable_interval: Some(Duration::from_secs(86400)),
            ..Profile::desktop()
        }
    }
}

/// Compact generated point, outside store memory accounting.
#[derive(Debug)]
pub struct Point {
    /// Index into the generated canonical label bytes.
    pub label: usize,
    /// Event time in milliseconds within the synthetic day.
    pub at: i64,
    /// Input value, including possible zero.
    pub value: u64,
}
/// Compact event including its predetermined null pattern.
#[derive(Debug)]
pub struct Row {
    /// Index into the generated canonical label bytes.
    pub label: usize,
    /// Event time in milliseconds within the synthetic day.
    pub at: i64,
    /// Whether this row carries a null text cell.
    pub null: bool,
}
/// Caller-owned generated inputs; no RNG, formatting or payload generation in timed work.
#[derive(Debug)]
pub struct Fixture {
    /// Generator parameters recorded with the measurement.
    pub shape: Shape,
    /// Canonical input labels, allocated before timing.
    pub labels: Vec<Vec<u8>>,
    /// Generated series inputs, outside store budgets.
    pub points: Vec<Point>,
    /// Generated events, outside store budgets.
    pub rows: Vec<Row>,
    /// Shared caller-owned text; repeated bytes intentionally compress well.
    pub payload: Vec<u8>,
    /// Independent sum of generated point values.
    pub expected_sum: u64,
}
impl Fixture {
    /// Local xorshift generator; seed zero is mapped to one rather than stuck at zero.
    pub fn generate(shape: Shape, seed: u64) -> Self {
        let mut state = seed.max(1);
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let labels = (0..shape.combinations)
            .map(|i| format!("label-{i:07}").into_bytes())
            .collect();
        let points: Vec<_> = (0..shape.points)
            .map(|i| Point {
                label: i % shape.combinations,
                at: (i as i64 * DAY / shape.points.max(1) as i64).min(DAY - 1),
                value: next() % 1000,
            })
            .collect();
        let rows = (0..shape.events)
            .map(|i| Row {
                label: i % shape.combinations,
                at: i as i64 * DAY / shape.events.max(1) as i64,
                null: i % 7 == 0,
            })
            .collect();
        let expected_sum = points.iter().map(|point| point.value).sum();
        Self {
            shape,
            labels,
            points,
            rows,
            payload: vec![b'x'; shape.text_bytes],
            expected_sum,
        }
    }
    /// Entire day's range; a delay affects arrival time, not the requested keys.
    pub fn range(&self) -> TimeRange {
        TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap()
    }
    /// Materializes one bounded borrowed batch before starting its write timer.
    pub fn batch<'a, S: Storage>(&'a self, db: &mut Tsdb<S>, index: usize) -> Batch<'a> {
        let count = self.batch_rows();
        let start = index * count;
        let points: Vec<_> = self.points.iter().skip(start).take(count).collect();
        let rows: Vec<_> = self.rows.iter().skip(start).take(count).collect();
        let labels = points
            .iter()
            .map(|p| [self.labels[p.label].as_slice()])
            .collect();
        let event_labels = rows
            .iter()
            .map(|p| [self.labels[p.label].as_slice()])
            .collect();
        let fields = points.iter().map(|p| [p.value]).collect();
        let seqs = rows.iter().map(|_| db.next_event_seq()).collect();
        Batch {
            fixture: self,
            index,
            points,
            rows,
            labels,
            event_labels,
            fields,
            seqs,
        }
    }
    /// Payload size bounds borrowed input batches below the experiment's batch cap.
    fn batch_rows(&self) -> usize {
        (2 * 1024 * 1024 / self.payload.len().max(1)).clamp(1, 256)
    }
    /// Number of bounded input batches.
    pub fn batches(&self) -> usize {
        self.points
            .len()
            .max(self.rows.len())
            .div_ceil(self.batch_rows())
    }
    /// Independently checks sums, event ordering and every decoded payload, without stored ids.
    pub fn verify<S: Storage>(&self, db: &Tsdb<S>) {
        if self.shape.full_catalog {
            let status = db.status().unwrap();
            let data = status.data().unwrap();
            assert!(
                data.catalog.combinations > 0,
                "full catalog must first admit real combinations"
            );
            assert!(
                data.streams()
                    .iter()
                    .any(|stream| stream.cumulative.folded_rows > 0),
                "full catalog never overflowed"
            );
        }
        let snap = db.reader().capture().unwrap();
        let groups = snap
            .series(&SeriesQuery {
                range: self.range(),
                tier: TierId(0),
                requests: &[GroupRequest {
                    filters: &[],
                    key: GroupKey::Total,
                }],
                fields: FieldMask(1),
            })
            .unwrap();
        let sum: u64 = groups.lists()[0].iter().map(|g| g.sums[0]).sum();
        assert_eq!(sum, self.expected_sum);
        for direction in [
            tsdb::storage::Direction::Forward,
            tsdb::storage::Direction::Backward,
        ] {
            let mut cursor = None;
            let mut seen = 0;
            loop {
                let page = snap
                    .scan_events(&EventQuery {
                        stream: StreamId(0),
                        range: self.range(),
                        direction,
                        cursor,
                        predicate: &EventPredicate::default(),
                        limit: 16,
                    })
                    .unwrap();
                for event in page.events() {
                    let i = if direction == tsdb::storage::Direction::Forward {
                        seen
                    } else {
                        self.rows.len() - 1 - seen
                    };
                    assert_eq!(event.key, self.rows[i].at);
                    if let EventValue::Combo(combo) = event.values[0] {
                        let label = snap.combination(combo).unwrap().next().unwrap();
                        if self.shape.full_catalog {
                            assert!(
                                label == 0
                                    || snap.label(LabelId(0), label).unwrap()
                                        == self.labels[self.rows[i].label]
                            );
                        } else {
                            assert_eq!(
                                snap.label(LabelId(0), label).unwrap(),
                                self.labels[self.rows[i].label]
                            );
                        }
                    } else {
                        panic!("missing series value");
                    }
                    assert!(matches!(&event.values[2], EventValue::Bytes(bytes)
                        if bytes.as_ref()==b"67e55044-10b1-426f-9247-bb680e5fe0c8"));
                    match &event.values[1] {
                        EventValue::Null => assert!(self.rows[i].null),
                        EventValue::Bytes(bytes) => {
                            assert!(!self.rows[i].null);
                            assert_eq!(bytes.as_ref(), self.payload);
                        }
                        value => panic!("unexpected text {value:?}"),
                    }
                    seen += 1;
                }
                cursor = page.cursor();
                if cursor.is_none() {
                    break;
                }
            }
            assert_eq!(seen, self.rows.len());
        }
    }
}

/// Prebuilt batch storage; borrowed API slices are formed outside the measured call.
pub struct Batch<'a> {
    /// Generated immutable inputs.
    fixture: &'a Fixture,
    /// Zero-based batch number.
    index: usize,
    /// Borrowed generated point descriptors.
    points: Vec<&'a Point>,
    /// Borrowed generated event descriptors.
    rows: Vec<&'a Row>,
    /// Prebuilt one-dimensional series keys.
    labels: Vec<[&'a [u8]; 1]>,
    /// Prebuilt event series keys.
    event_labels: Vec<[&'a [u8]; 1]>,
    /// Prebuilt point field values.
    fields: Vec<[u64; 1]>,
    /// Positions allocated before timing the write.
    seqs: Vec<EventSeq>,
}
impl Batch<'_> {
    /// Calls the observer around only the public write, after borrowed arrays are ready.
    pub fn write<S: Storage, T>(
        &self,
        db: &mut Tsdb<S>,
        durability: Durability,
        measure: impl FnOnce(&mut Tsdb<S>, &Input<'_>, Durability) -> T,
    ) -> T {
        let points: Vec<_> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| SeriesPoint {
                tier: TierId(0),
                at: Timestamp(p.at),
                labels: &self.labels[i],
                fields: &self.fields[i],
            })
            .collect();
        let values: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, p)| {
                [
                    Value::Series(&self.event_labels[i]),
                    if p.null {
                        Value::Null
                    } else {
                        Value::Bytes(&self.fixture.payload)
                    },
                    Value::Bytes(b"67e55044-10b1-426f-9247-bb680e5fe0c8"),
                ]
            })
            .collect();
        let events: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, p)| EventRow {
                stream: StreamId(0),
                key: p.at,
                seq: self.seqs[i],
                values: &values[i],
            })
            .collect();
        let now = self
            .points
            .last()
            .map(|p| p.at)
            .unwrap_or(0)
            .max(self.rows.last().map(|p| p.at).unwrap_or(0))
            + self.fixture.shape.late_ms;
        let pin = [PinOp::Pin {
            key: b"synthetic-pin",
            labels: &[self.fixture.labels[0].as_slice()],
        }];
        let pins: &[PinOp<'_>] = if self.fixture.shape.churn && self.index == 0 {
            &pin
        } else {
            &[]
        };
        let batch_id = self.index as u64 + 1;
        let aux_bytes = batch_id.to_le_bytes();
        let input = Input {
            batch_id,
            now: Timestamp(now),
            series: &points,
            events: &events,
            aux: &[AuxOp::Put {
                key: b"synthetic-host",
                value: &aux_bytes,
            }],
            pins,
        };
        measure(db, &input, durability)
    }
}
