//! Closed-file redb accounting for synthetic consolidated chunks; no format experiment.

use super::{
    disk::{Counting, Counts},
    tsdb::{
        storage::{Direction, Durability, redb::RedbStorage},
        *,
    },
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle};
use std::{
    collections::BTreeMap,
    fmt::Write,
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

/// Cohorts use separate extents; retention removes exactly the older half.
const WINDOW: i64 = 1_000_000;
/// Separate schema from the timing harness; all integer byte columns are explicit.
const HEADER: &str = "source,seed,shape,rows_per_extent,input_payload_bytes,repeat,state,table,\
     live_chunks,fragment0_bytes,data_bytes,fragments,table_stored_bytes,table_metadata_bytes,\
     table_fragmented_bytes,table_pages,file_bytes,page_size,allocated_pages,stored_bytes,\
     metadata_bytes,fragmented_bytes,unattributed_allocated_bytes,free_pages,header_bytes,\
     phase_write_bytes,phase_syncs,total_write_bytes,total_syncs,write_bytes_per_chunk,\
     syncs_per_chunk,phase_write_to_live_payload,inline_upper_bytes,\
     inline_boundary_penalty_bytes\n";

/// One parameterized shape: fixed-width random bytes, or a summed series with random values.
#[derive(Clone, Copy)]
struct Shape {
    /// Stable CSV label, including the boundary side or full-chunk control.
    name: &'static str,
    /// Representation rows after consolidation, per extent.
    rows: usize,
    /// Zero selects series; otherwise one event column has this fixed width.
    width: usize,
}
impl Shape {
    /// The full matrix and a quick matrix that still crosses a page boundary.
    fn matrix(smoke: bool) -> Vec<Self> {
        let mut shapes = vec![];
        for (name, rows) in [
            ("rows-1", 1),
            ("rows-8", 8),
            ("rows-64", 64),
            ("rows-512", 512),
        ] {
            if !smoke || rows == 1 || rows == 8 {
                shapes.push(Self {
                    name,
                    rows,
                    width: 0,
                });
            }
        }
        for (below, above, kib) in [
            ("payload-4k-minus", "payload-4k-plus", 4),
            ("payload-8k-minus", "payload-8k-plus", 8),
            ("payload-16k-minus", "payload-16k-plus", 16),
            ("payload-32k-minus", "payload-32k-plus", 32),
            ("payload-64k-minus", "payload-64k-plus", 64),
        ] {
            if !smoke || kib == 4 {
                let rows = kib * 2;
                shapes.push(Self {
                    name: below,
                    rows,
                    width: 512 - 512 / rows,
                });
                shapes.push(Self {
                    name: above,
                    rows,
                    width: 512 + 512 / rows,
                });
            }
        }
        if !smoke {
            shapes.push(Self {
                name: "full-rows",
                rows: 65_536,
                width: 0,
            });
            shapes.push(Self {
                name: "full-bytes",
                rows: 3_800,
                width: 1_024,
            });
        }
        shapes
    }
    /// Minimal independent series/event schemas; fixed bytes avoid optimistic text compression.
    fn schema(self) -> Schema {
        Schema {
            version: 1,
            labels: if self.width == 0 {
                vec![LabelSpec { name: "label" }]
            } else {
                vec![]
            },
            series: (self.width == 0).then(|| SeriesSpec {
                dimensions: vec![LabelId(0)],
                fields: vec![FieldSpec {
                    name: "sum",
                    aggregate: Aggregate::SaturatingSum,
                }],
                tiers: vec![TierSpec {
                    name: "tier",
                    bucket: Duration::from_millis(1),
                    extent: Duration::from_millis(WINDOW as u64),
                }],
                fold: vec![],
            }),
            events: if self.width == 0 {
                vec![]
            } else {
                vec![EventStreamSpec {
                    name: "events",
                    key: EventKey::Timestamp,
                    columns: vec![ColumnSpec {
                        name: "bytes",
                        ty: ColumnType::Fixed(self.width as u16),
                        nullable: false,
                    }],
                    extent: ExtentSpec::Window(Duration::from_millis(WINDOW as u64)),
                }]
            },
        }
    }
}

/// Generated once, before any counted backend is opened, reused across independent repetitions.
struct Fixture {
    /// Row counts and type.
    shape: Shape,
    /// Caller-owned canonical labels, reused in each extent.
    labels: Vec<Vec<u8>>,
    /// Caller-owned sums; two equal writes sum to twice this value.
    fields: Vec<[u64; 1]>,
    /// Incompressible, deterministic fixed-width event values.
    payload: Vec<Vec<u8>>,
    /// Cohort count; the full controls use fewer extents, never smaller chunks.
    extents: usize,
}
impl Fixture {
    /// Local xorshift only; no global RNG, codec assumptions or production ids.
    fn generate(shape: Shape, seed: u64, smoke: bool) -> Self {
        let mut random = seed.max(1);
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        Self {
            shape,
            labels: if shape.width == 0 {
                (0..shape.rows)
                    .map(|i| format!("label-{i:06}").into_bytes())
                    .collect()
            } else {
                vec![]
            },
            fields: (0..shape.rows).map(|_| [next() & 0xffff_ffff]).collect(),
            payload: (0..shape.rows)
                .map(|_| (0..shape.width).map(|_| next() as u8).collect())
                .collect(),
            extents: if smoke || shape.name.starts_with("full") {
                2
            } else {
                8
            },
        }
    }
    /// Two series passes guarantee even the one-row extent is actually consolidated from L0.
    fn write(&self, db: &mut Tsdb<RedbStorage>) {
        let mut batch_id = 0;
        let passes = if self.shape.width == 0 { 2 } else { 1 };
        let batch = if self.shape.width == 0 {
            4_096
        } else {
            (self.shape.rows / 2).clamp(1, 256)
        };
        for extent in 0..self.extents {
            for _ in 0..passes {
                for start in (0..self.shape.rows).step_by(batch) {
                    let end = (start + batch).min(self.shape.rows);
                    let labels: Vec<_> = (start..end)
                        .map(|i| [self.labels.get(i).map_or(&[][..], Vec::as_slice)])
                        .collect();
                    let points: Vec<_> = if self.shape.width == 0 {
                        (start..end)
                            .map(|i| SeriesPoint {
                                tier: TierId(0),
                                at: Timestamp(extent as i64 * WINDOW),
                                labels: &labels[i - start],
                                fields: &self.fields[i],
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                    let values: Vec<_> = (start..end)
                        .map(|i| [Value::Bytes(&self.payload[i])])
                        .collect();
                    let events: Vec<_> = if self.shape.width != 0 {
                        (start..end)
                            .map(|i| EventRow {
                                stream: StreamId(0),
                                key: extent as i64 * WINDOW + i as i64,
                                seq: db.next_event_seq(),
                                values: &values[i - start],
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                    batch_id += 1;
                    db.write(
                        &Input {
                            batch_id,
                            now: Timestamp((self.extents as i64 + 1) * WINDOW),
                            series: &points,
                            events: &events,
                            aux: &[],
                            pins: &[],
                        },
                        Durability::Durable,
                    )
                    .unwrap();
                }
            }
        }
    }
    /// Public sums/pages and metadata verify the exact remaining cohort, before every close.
    fn verify(&self, db: &Tsdb<RedbStorage>, retained: bool) {
        let first = if retained { self.extents / 2 } else { 0 };
        let status = db.status().unwrap();
        let data = status.data().unwrap();
        assert_eq!(
            data.streams()[0].rows,
            ((self.extents - first) * self.shape.rows) as u64
        );
        assert_eq!(data.streams()[0].pending.cleanup_chunks, 0);
        assert_eq!(data.streams()[0].pending.l0_chunks, 0);
        assert_eq!(data.journal_bytes, 0);
        assert!(!data.maintenance.behind);
        drop(status);
        let snap = db.reader().capture().unwrap();
        let range = TimeRange::new(Timestamp(0), Timestamp(self.extents as i64 * WINDOW)).unwrap();
        if self.shape.width == 0 {
            let result = snap
                .series(&SeriesQuery {
                    range,
                    tier: TierId(0),
                    fields: FieldMask(1),
                    requests: &[GroupRequest {
                        filters: &[],
                        key: GroupKey::Total,
                    }],
                })
                .unwrap();
            let sum: u64 = result.lists()[0].iter().map(|g| g.sums[0]).sum();
            assert_eq!(
                sum,
                self.fields.iter().map(|v| 2 * v[0]).sum::<u64>() * (self.extents - first) as u64
            );
        } else {
            for direction in [Direction::Forward, Direction::Backward] {
                let mut seen = 0;
                let mut cursor = None;
                loop {
                    let page = snap
                        .scan_events(&EventQuery {
                            range,
                            stream: StreamId(0),
                            direction,
                            cursor,
                            predicate: &EventPredicate::default(),
                            limit: 128,
                        })
                        .unwrap();
                    for event in page.events() {
                        let index = if direction == Direction::Forward {
                            first * self.shape.rows + seen
                        } else {
                            self.extents * self.shape.rows - seen - 1
                        };
                        assert_eq!(
                            event.key,
                            (index / self.shape.rows) as i64 * WINDOW
                                + (index % self.shape.rows) as i64
                        );
                        let EventValue::Bytes(bytes) = &event.values[0] else {
                            panic!("the payload column holds bytes");
                        };
                        assert!(bytes.as_ref() == self.payload[index % self.shape.rows]);
                        seen += 1;
                    }
                    cursor = page.cursor();
                    if cursor.is_none() {
                        break;
                    }
                }
                assert_eq!(seen, (self.extents - first) * self.shape.rows);
            }
        }
    }
}

/// Experiment overrides force bounded L0 flushes and quiet consolidation, never format/floor edits.
fn profile() -> Profile {
    Profile {
        journal_bytes: 1,
        event_buffer_rows: 1,
        event_buffer_age: Duration::from_millis(1),
        series_quiet_time: Some(Duration::from_millis(1)),
        ..Profile::desktop()
    }
}
/// Converges before inspecting a closed file; counts prove retention and cleanup really ran.
fn converge(db: &mut Tsdb<RedbStorage>, fixture: &Fixture, retained: bool) -> Maintained {
    let retention = Retention {
        max_age: retained.then_some(Duration::from_millis(
            (fixture.extents as u64 / 2 + 1) * WINDOW as u64,
        )),
        max_bytes: None,
    };
    let policy = RetentionPolicy {
        tiers: if fixture.shape.width == 0 {
            vec![retention]
        } else {
            vec![]
        },
        streams: if fixture.shape.width == 0 {
            vec![]
        } else {
            vec![retention]
        },
    };
    let mut total = Maintained::default();
    for _ in 0..10_000 {
        let result = db
            .maintain(&Maintain {
                now: Timestamp((fixture.extents as i64 + 1) * WINDOW),
                retention: &policy,
                rounds: 1,
            })
            .unwrap();
        total.expired += result.expired;
        total.chunks_deleted += result.chunks_deleted;
        total.chunks_built += result.chunks_built;
        total.consolidations += result.consolidations;
        if !result.more {
            return total;
        }
    }
    panic!("space fixture did not converge");
}

/// One table's own redb accounting, including byte-string keys in stored bytes.
#[derive(Default)]
struct Table {
    /// Raw keys plus values.
    stored: u64,
    /// Btree metadata.
    metadata: u64,
    /// Unused bytes inside allocated btree pages.
    fragmented: u64,
    /// Allocated base pages, including multi-page leaves (node counts are not page counts).
    pages: u64,
}
/// Closed-file observation. System bytes and free pages are distinct, never called payload.
struct Stats {
    /// Per-table measurements from the untyped table API.
    tables: BTreeMap<String, Table>,
    /// Physical length of the original closed file, excluding inspection I/O.
    file: u64,
    /// DatabaseStats page size, allocated pages, stored, metadata, then slack excluding free pages.
    db: [u64; 5],
    /// Allocated bytes outside the live stats footprint, e.g. deferred freed pages.
    system: u64,
    /// Unallocated pages, excluding the one-page v3 file header.
    free: u64,
    /// Actual chunks, directory bytes, other fragment bytes and fragment count.
    fragments: [u64; 4],
    /// Optimistic fragment-table-only ceiling, not a measured alternative format.
    inline_upper: u64,
    /// Dedicated allocation growth if two small fragments were concatenated across a boundary.
    inline_penalty: u64,
}
impl Stats {
    /// Inspects a copy so opening/aborting/dropping redb cannot alter workload counters or layout.
    fn read(path: &Path, copy: &Path) -> Self {
        let file = fs::metadata(path).unwrap().len();
        fs::copy(path, copy).unwrap();
        let db = redb::Database::open(copy).unwrap();
        let write = db.begin_write().unwrap();
        let stats = write.stats().unwrap();
        let page = stats.page_size() as u64;
        let allocated = stats.allocated_pages();
        // redb 4.4 creates v3 files with no region headers (page_manager.rs:39-41).
        assert_eq!(file % page, 0);
        let free = file / page - 1 - allocated;
        // WriteTransaction::stats includes free pages in fragmented_bytes (transactions.rs:2666).
        let fragmented = stats.fragmented_bytes() - free * page;
        let footprint = stats.stored_bytes() + stats.metadata_bytes() + fragmented;
        assert_eq!(footprint % page, 0);
        assert!(allocated * page >= footprint);
        let numbers = [
            page,
            allocated,
            stats.stored_bytes(),
            stats.metadata_bytes(),
            fragmented,
        ];
        let read = db.begin_read().unwrap();
        let mut tables = BTreeMap::new();
        for handle in read.list_tables().unwrap() {
            let name = handle.name().to_owned();
            let table = read.open_untyped_table(handle).unwrap();
            let s = table.stats().unwrap();
            tables.insert(
                name,
                Table {
                    stored: s.stored_bytes(),
                    metadata: s.metadata_bytes(),
                    fragmented: s.fragmented_bytes(),
                    pages: (s.stored_bytes() + s.metadata_bytes() + s.fragmented_bytes()) / page,
                },
            );
        }
        assert_eq!(
            tables.values().map(|t| t.stored).sum::<u64>(),
            stats.stored_bytes()
        );
        for table in tables.values() {
            assert_eq!(
                table.pages * page,
                table.stored + table.metadata + table.fragmented
            );
        }
        let table = read
            .open_table(TableDefinition::<(u64, u32), &[u8]>::new("fragments"))
            .unwrap();
        let mut chunks: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let mut fragments = [0; 4];
        for entry in table.iter().unwrap() {
            let (key, value) = entry.unwrap();
            let (chunk, index) = key.value();
            let len = value.value().len() as u64;
            let values = chunks.entry(chunk).or_default();
            assert_eq!(index as usize, values.len());
            values.push(len);
            fragments[if index == 0 { 1 } else { 2 }] += len;
            fragments[3] += 1;
        }
        fragments[0] = chunks.len() as u64;
        assert!(fragments[0] > 0);
        assert_eq!(
            tables["fragments"].stored,
            fragments[1] + fragments[2] + 12 * fragments[3]
        );
        // A ceiling assumes perfect packing, no branch/record overhead and free key/CRC removal.
        // Large leaves retain redb's power-of-two allocation. It is deliberately optimistic.
        let mut small = 0;
        let mut dedicated = 0;
        let mut penalty = 0;
        let mut eligible_chunks = 0;
        for values in chunks.values() {
            let eligible = values.len() == 2 && values[1] < 64 * 1024;
            eligible_chunks += u64::from(eligible);
            let packed: Vec<u64> = if eligible {
                vec![values.iter().sum::<u64>() - 4]
            } else {
                values.clone()
            };
            if eligible && values[1] >= page {
                let old = (values[1] + 20).div_ceil(page).next_power_of_two() * page;
                let new = (values[0] + values[1] - 4 + 20)
                    .div_ceil(page)
                    .next_power_of_two()
                    * page;
                penalty += new.saturating_sub(old);
            }
            for len in packed {
                if len >= page {
                    dedicated += (len + 20).div_ceil(page).next_power_of_two() * page;
                } else {
                    small += len + 12;
                }
            }
        }
        let ideal = dedicated + small.div_ceil(page) * page;
        let inline_upper = if eligible_chunks == 0 {
            0
        } else {
            (tables["fragments"].pages * page).saturating_sub(ideal)
        };
        Self {
            tables,
            file,
            db: numbers,
            system: allocated * page - footprint,
            free,
            fragments,
            inline_upper,
            inline_penalty: penalty,
        }
    }
    /// Emits one row per real table plus a total row; global columns repeat and must not be summed.
    fn csv(
        &self,
        seed: u64,
        fixture: &Fixture,
        repeat: usize,
        state: &str,
        phase: (u64, u64),
        total: (u64, u64),
    ) -> String {
        let mut csv = String::new();
        for name in self
            .tables
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("TOTAL"))
        {
            let aggregate = Table {
                stored: self.db[2],
                metadata: self.db[3],
                fragmented: self.db[4],
                pages: (self.db[2] + self.db[3] + self.db[4]) / self.db[0],
            };
            let t = self.tables.get(name).unwrap_or(&aggregate);
            writeln!(
                csv,
                "synthetic-approximation,{seed},{},{},{},{repeat},{state},{name},{},{},\
                 {},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{:.6},\
                 {:.6},{},{}",
                fixture.shape.name,
                fixture.shape.rows,
                fixture.shape.rows * fixture.shape.width,
                self.fragments[0],
                self.fragments[1],
                self.fragments[2],
                self.fragments[3],
                t.stored,
                t.metadata,
                t.fragmented,
                t.pages,
                self.file,
                self.db[0],
                self.db[1],
                self.db[2],
                self.db[3],
                self.db[4],
                self.system,
                self.free,
                self.db[0],
                phase.0,
                phase.1,
                total.0,
                total.1,
                phase.0 as f64 / self.fragments[0] as f64,
                phase.1 as f64 / self.fragments[0] as f64,
                phase.0 as f64 / (self.fragments[1] + self.fragments[2]) as f64,
                self.inline_upper,
                self.inline_penalty
            )
            .unwrap();
        }
        csv
    }
}

/// Full matrix: three independent identical-seed runs; smoke: one run of four small shapes.
pub fn run(seed: u64, smoke: bool) -> String {
    let elapsed = Instant::now();
    let mut csv = HEADER.to_owned();
    for shape in Shape::matrix(smoke) {
        let fixture = Fixture::generate(shape, seed, smoke);
        for repeat in 0..if smoke { 1 } else { 3 } {
            eprintln!(
                "space synthetic approximation {} repeat={repeat}",
                shape.name
            );
            let temp = super::tempdir();
            let path = temp.path().join("space.redb");
            let copy = temp.path().join("stats.redb");
            let counts = Arc::new(Counts::default());
            let open = || {
                Tsdb::open(
                    RedbStorage::open_with_backend(
                        Counting::open(&path, counts.clone()),
                        profile().backend_cache,
                    )
                    .unwrap(),
                    shape.schema(),
                    profile(),
                )
                .unwrap()
            };
            let mut db = open();
            fixture.write(&mut db);
            let done = converge(&mut db, &fixture, false);
            assert!(done.consolidations > 0 && done.chunks_built > 0);
            fixture.verify(&db, false);
            db.close(Timestamp(fixture.extents as i64 * WINDOW))
                .unwrap();
            drop(db);
            assert_eq!(counts.closes(), 1);
            let fresh = Stats::read(&path, &copy);
            assert_eq!(
                fresh.fragments[0], fixture.extents as u64,
                "expected one consolidated chunk per extent"
            );
            csv += &fresh.csv(
                seed,
                &fixture,
                repeat,
                "fresh",
                counts.read(),
                counts.read(),
            );
            if shape.name.starts_with("payload-") {
                let center = (shape.rows * 512) as u64;
                let bytes = fresh.fragments[2] / fresh.fragments[0];
                assert!(if shape.name.ends_with("minus") {
                    bytes < center
                } else {
                    bytes > center
                });
            }
            let before = counts.read();
            let mut db = open();
            let done = converge(&mut db, &fixture, true);
            assert_eq!(done.expired, (fixture.extents / 2) as u64);
            assert_eq!(done.chunks_deleted, (fixture.extents / 2) as u64);
            fixture.verify(&db, true);
            db.close(Timestamp(fixture.extents as i64 * WINDOW))
                .unwrap();
            drop(db);
            let retained = Stats::read(&path, &copy);
            assert_eq!(retained.fragments[0], fresh.fragments[0] / 2);
            let after = counts.read();
            csv += &retained.csv(
                seed,
                &fixture,
                repeat,
                "retained",
                (after.0 - before.0, after.1 - before.1),
                after,
            );
            let before = counts.read();
            let mut db = open();
            assert!(matches!(db.compact().unwrap(), Compaction::Executed { .. }));
            fixture.verify(&db, true);
            db.close(Timestamp(fixture.extents as i64 * WINDOW))
                .unwrap();
            drop(db);
            let compacted = Stats::read(&path, &copy);
            assert_eq!(compacted.fragments, retained.fragments);
            let after = counts.read();
            csv += &compacted.csv(
                seed,
                &fixture,
                repeat,
                "compacted",
                (after.0 - before.0, after.1 - before.1),
                after,
            );
        }
    }
    validate(&csv);
    eprintln!("space validated in {:.3}s", elapsed.elapsed().as_secs_f64());
    csv
}

/// Smoke checks the whole schema, every state and closed-file byte conservation, not just parsing.
fn validate(csv: &str) {
    let columns = HEADER.trim_end().split(',').count();
    let mut states = [0; 3];
    for row in csv.lines().skip(1) {
        let cells: Vec<_> = row.split(',').collect();
        assert_eq!(cells.len(), columns);
        assert_eq!(cells[0], "synthetic-approximation");
        for index in [1, 3, 4, 5, 32, 33] {
            cells[index].parse::<u64>().unwrap();
        }
        for value in &cells[8..29] {
            value.parse::<u64>().unwrap();
        }
        assert!(cells[29].parse::<f64>().unwrap().is_finite());
        assert!(cells[30].parse::<f64>().unwrap().is_finite());
        assert!(cells[31].parse::<f64>().unwrap().is_finite());
        if cells[7] == "TOTAL" {
            let n = |i: usize| cells[i].parse::<u64>().unwrap();
            assert_eq!(n(16), n(19) + n(20) + n(21) + n(22) + n(23) * n(17) + n(24));
            states[match cells[6] {
                "fresh" => 0,
                "retained" => 1,
                "compacted" => 2,
                _ => panic!("state"),
            }] += 1;
        }
    }
    assert!(states[0] > 0 && states[0] == states[1] && states[1] == states[2]);
}
