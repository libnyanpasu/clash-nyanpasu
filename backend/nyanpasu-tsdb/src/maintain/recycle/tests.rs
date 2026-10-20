//! Recycling end to end: what keeps an id, what releases it, the catalog's reserve, failures
//! and a crash, the bounds of a round and of planning, the write path's references and their
//! cost on traffic's schema, and a model of writes, pins, retention and snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use proptest::prelude::*;

use super::*;
use crate::{
    EventPage, EventPredicate, EventQuery, EventValue, FieldMask, GroupKey, GroupRequest, KeyPart,
    Maintain, MaintainError, Profile, Retention, RetentionPolicy, SeriesGroups, SeriesQuery,
    Snapshot, Timestamp, Tsdb,
    catalog::raw,
    db::tests::{
        CLOSED, HOST, HOUR, HOUR_MS, LABELS, MINUTE, MINUTE_MS, PROCESS, RULE, TARGET, UUID,
        dump_storage, open, open_with, schema,
    },
    layout::{META, REFERENCE_INDEX, REFERENCES, references::tests as stored},
    maintain::{
        self,
        tests::{Tally, all, check_accounting, check_settled, finish, policy, run, work},
    },
    storage::{
        self, ChunkId, CommitFailure, Durability, FragmentId, KeyRange, Ns, Storage,
        conformance::{CommitOutcome, OpClass},
        memory::{MemoryDisk, MemoryStorage},
    },
    time::Grid,
    write::{EventRow, Input, PinOp, SeriesPoint, Value, Written},
};

const DAY_MS: i64 = 24 * HOUR_MS;
const MINUTES: StreamRef = StreamRef::Tier(MINUTE);
const HOURS: StreamRef = StreamRef::Tier(HOUR);
const EVENTS: StreamRef = StreamRef::Events(CLOSED);
const DIMENSIONS: [LabelId; 3] = [RULE, PROCESS, TARGET];

fn age(hours: u64) -> Retention {
    Retention {
        max_age: Some(Duration::from_secs(hours * 3600)),
        max_bytes: None,
    }
}

fn keep() -> Retention {
    Retention::default()
}

/// Chunks every source in the commit that writes it, so that an expired extent keeps chunks
/// until cleanup deletes them.
fn chunking() -> Profile {
    Profile {
        journal_bytes: 1,
        ..Profile::desktop()
    }
}

/// The labels `values`, kept for the rest of the test.
fn leak(values: [&str; 3]) -> &'static [&'static [u8]] {
    Vec::leak(
        values
            .iter()
            .map(|value| String::leak(value.to_string()).as_bytes())
            .collect(),
    )
}

/// A point of `tier` at `at`, of `labels`, worth 1.
fn point(tier: TierId, at: i64, labels: &'static [&'static [u8]]) -> SeriesPoint<'static> {
    SeriesPoint {
        tier,
        at: Timestamp(at),
        labels,
        fields: &[1, 0],
    }
}

/// The values of a `closed` row: its series value and its host, if any, and its upload.
fn closed(
    series: Option<&'static [&'static [u8]]>,
    host: Option<&'static [u8]>,
    upload: u64,
) -> Vec<Value<'static>> {
    vec![
        Value::I64(0),
        Value::U64(upload),
        series.map_or(Value::Null, Value::Series),
        host.map_or(Value::Null, Value::Bytes),
        Value::Null,
        Value::Bytes(UUID),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::Null,
    ]
}

/// Commits batch `batch_id` at `now`: `points`, a `closed` row of each of `rows` (key,
/// values), and `pins`.
fn commit<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    now: i64,
    points: &[SeriesPoint<'_>],
    rows: &[(i64, Vec<Value<'static>>)],
    pins: &[PinOp<'_>],
) -> Written {
    let events: Vec<EventRow<'_>> = rows
        .iter()
        .map(|(key, values)| EventRow {
            stream: CLOSED,
            key: *key,
            seq: tsdb.next_event_seq(),
            values,
        })
        .collect();
    let input = Input {
        batch_id,
        now: Timestamp(now),
        series: points,
        events: &events,
        aux: &[],
        pins,
    };
    tsdb.write(&input, Durability::Durable).unwrap()
}

/// The id of the live combination of `labels`, if any.
fn combo<S: Storage>(tsdb: &Tsdb<S>, labels: &[&[u8]]) -> Option<u32> {
    let current = tsdb.current();
    let ids: Vec<u32> = DIMENSIONS
        .iter()
        .zip(labels)
        .map(|(&dimension, value)| current.catalog.find_label(dimension, value))
        .collect::<Option<_>>()?;
    current
        .catalog
        .live_combos()
        .into_iter()
        .find(|(_, labels)| *labels == ids)
        .map(|(id, _)| id)
}

fn label<S: Storage>(tsdb: &Tsdb<S>, dimension: LabelId, value: &[u8]) -> Option<u32> {
    tsdb.current().catalog.find_label(dimension, value)
}

fn candidates<S: Storage>(tsdb: &Tsdb<S>) -> Vec<Entity> {
    stored::candidates(&tsdb.storage().snapshot().unwrap())
}

fn newest<S: Storage>(tsdb: &Tsdb<S>, entity: Entity, stream: StreamRef) -> Option<i64> {
    references::newest(&tsdb.storage().snapshot().unwrap(), entity, stream).unwrap()
}

/// The round `maintain` would run next at `now` under `retention`.
fn next_round<S: Storage>(tsdb: &Tsdb<S>, now: i64, retention: &RetentionPolicy) -> Option<Round> {
    let request = Maintain {
        now: Timestamp(now),
        retention,
        rounds: 1,
    };
    let snapshot = tsdb.storage().snapshot().unwrap();
    maintain::plan(&snapshot, &tsdb.round_context(&tsdb.current()), &request).unwrap()
}

/// Runs a round per call until the next one recycles, within the work there is.
fn until_recycling<S: Storage>(tsdb: &mut Tsdb<S>, now: i64, retention: &RetentionPolicy) {
    for _ in 0..=work(tsdb) {
        if next_round(tsdb, now, retention) == Some(Round::Recycle) {
            return;
        }
        run(tsdb, now, retention, 1).unwrap();
    }
    panic!("no recycling round came");
}

/// Every entry but the metadata, whose state record counts publications and incarnations.
fn without_meta(entries: Vec<(u8, Vec<u8>, Vec<u8>)>) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    entries
        .into_iter()
        .filter(|(ns, ..)| *ns != META.0)
        .collect()
}

// An expired extent keeps its ids until cleanup deletes its chunks: a commit's round, which has
// no policy to clean up with, forgets the reference of the stream that is cleaned and keeps the
// one of the stream that is not. A combination in two streams is kept until both are cleaned,
// and a label value a live combination uses is kept.
#[test]
fn an_expired_extent_keeps_its_ids_until_cleanup_reaches_it() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let old = leak(["match", "old-process", "old.com"]);
    let host: &[u8] = b"old-host";
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, old)],
        &[(5, closed(Some(old), Some(host), 1))],
        &[],
    );
    let day = DAY_MS + HOUR_MS;
    commit(
        &mut tsdb,
        2,
        day,
        &[point(HOUR, day, LABELS)],
        &[(day, closed(Some(LABELS), Some(b"example.com"), 2))],
        &[],
    );
    let id = combo(&tsdb, old).unwrap();
    let retention = policy(keep(), age(24), age(24));
    let now = 2 * DAY_MS + HOUR_MS;
    let done = run(&mut tsdb, now, &retention, 2).unwrap();
    assert_eq!((done.expired, done.more), (2, true));
    let done = run(&mut tsdb, now, &retention, 1).unwrap();
    assert_eq!((done.cleanups, done.more), (1, true));
    assert!(matches!(
        next_round(&tsdb, now, &retention),
        Some(Round::Cleanup {
            stream: EVENTS,
            expired: 0
        })
    ));

    let written = commit(&mut tsdb, 3, now, &[], &[], &[]);
    assert_eq!(
        written.maintained,
        Maintained {
            recycles: 1,
            references_forgotten: 1,
            ..Maintained::default()
        }
    );
    assert_eq!(combo(&tsdb, old), Some(id));
    assert_eq!(newest(&tsdb, Entity::Combo(id), HOURS), None);
    assert_eq!(newest(&tsdb, Entity::Combo(id), EVENTS), Some(0));
    assert_eq!(candidates(&tsdb), []);

    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(
        (done.cleanups, done.combos_released, done.labels_released),
        (1, 1, 3)
    );
    assert_eq!(combo(&tsdb, old), None);
    for (dimension, value) in [
        (PROCESS, &b"old-process"[..]),
        (TARGET, b"old.com"),
        (HOST, host),
    ] {
        assert_eq!(label(&tsdb, dimension, value), None);
    }
    assert!(combo(&tsdb, LABELS).is_some());
    assert!(label(&tsdb, RULE, b"match").is_some());
    assert!(label(&tsdb, HOST, b"example.com").is_some());
    assert_eq!(candidates(&tsdb), []);
    assert_eq!(next_round(&tsdb, now, &retention), None);
    check_settled(&tsdb);
    check_accounting(&tsdb);
}

#[test]
fn a_combination_in_two_tiers_is_kept_until_both_are_cleaned() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let both = leak(["match", "both", "both.com"]);
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(MINUTE, HOUR_MS, both), point(HOUR, HOUR_MS, both)],
        &[],
        &[],
    );
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let id = combo(&tsdb, both).unwrap();
    let now = 2 * DAY_MS;
    let done = finish(&mut tsdb, now, &policy(age(2), keep(), keep()));
    assert_eq!((done.references_forgotten, done.combos_released), (1, 0));
    assert_eq!(newest(&tsdb, Entity::Combo(id), MINUTES), None);
    assert_eq!(newest(&tsdb, Entity::Combo(id), HOURS), Some(0));
    assert_eq!(combo(&tsdb, both), Some(id));
    let done = finish(&mut tsdb, now, &policy(age(2), age(24), keep()));
    assert_eq!((done.combos_released, done.labels_released), (1, 2));
    assert_eq!(combo(&tsdb, both), None);
    assert!(combo(&tsdb, LABELS).is_some());
}

// A pin keeps a combination its data no longer references; the unpin that takes its last pin
// makes it a candidate in the batch's own transaction, and the next round releases it.
#[test]
fn a_pinned_combination_is_kept_then_released_after_its_unpin() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let pinned = leak(["match", "pinned", "pinned.com"]);
    let pin = [PinOp::Pin {
        key: b"conn",
        labels: pinned,
    }];
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, pinned)],
        &[],
        &pin,
    );
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let id = combo(&tsdb, pinned).unwrap();
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), keep()));
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(
        (
            done.references_forgotten,
            done.candidates_checked,
            done.combos_released
        ),
        (1, 1, 0)
    );
    assert_eq!(combo(&tsdb, pinned), Some(id));
    assert_eq!(candidates(&tsdb), []);

    commit(
        &mut tsdb,
        3,
        now,
        &[],
        &[],
        &[PinOp::Unpin { key: b"conn" }],
    );
    assert_eq!(candidates(&tsdb), [Entity::Combo(id)]);
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!((done.combos_released, done.labels_released), (1, 2));
    assert_eq!(combo(&tsdb, pinned), None);
    assert!(label(&tsdb, RULE, b"match").is_some());
}

// A combination pinned again after its last unpin queued it stays a candidate until a round
// finds it pinned and drops the candidate; its next last unpin queues it again. An unpin in a
// batch whose data references the combination queues nothing.
#[test]
fn repinning_a_queued_candidate_keeps_it() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let lonely = leak(["lonely", "lonely", "lonely.com"]);
    let pin = |key| PinOp::Pin {
        key,
        labels: lonely,
    };
    commit(&mut tsdb, 1, 0, &[], &[], &[pin(b"a")]);
    let id = combo(&tsdb, lonely).unwrap();
    commit(&mut tsdb, 2, 0, &[], &[], &[PinOp::Unpin { key: b"a" }]);
    assert_eq!(candidates(&tsdb), [Entity::Combo(id)]);
    commit(&mut tsdb, 3, 0, &[], &[], &[pin(b"b")]);
    assert_eq!(candidates(&tsdb), [Entity::Combo(id)]);
    let retention = policy(keep(), keep(), keep());
    let done = finish(&mut tsdb, 0, &retention);
    assert_eq!(
        (done.recycles, done.candidates_checked, done.combos_released),
        (1, 1, 0)
    );
    assert_eq!(combo(&tsdb, lonely), Some(id));
    assert_eq!(candidates(&tsdb), []);

    commit(&mut tsdb, 4, 0, &[], &[], &[PinOp::Unpin { key: b"b" }]);
    let done = finish(&mut tsdb, 0, &retention);
    // One round: the values the combination left unused are candidates the round takes too.
    assert_eq!(
        (done.recycles, done.combos_released, done.labels_released),
        (1, 1, 3)
    );
    assert_eq!(combo(&tsdb, lonely), None);

    let pins = [pin(b"c"), PinOp::Unpin { key: b"c" }];
    commit(&mut tsdb, 5, 0, &[point(HOUR, 0, lonely)], &[], &pins);
    assert_eq!(candidates(&tsdb), []);
}

/// The hour tier's groups by every series dimension, and the first page of `closed`, read from
/// `snapshot`.
fn results(snapshot: &Snapshot<MemoryStorage>) -> (SeriesGroups, EventPage) {
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&DIMENSIONS),
    }];
    let groups = snapshot
        .series(&SeriesQuery {
            range: all(),
            tier: HOUR,
            requests: &requests,
            fields: FieldMask(1),
        })
        .unwrap();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: all(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 10,
        })
        .unwrap();
    (groups, page)
}

/// What `snapshot` gives the ids of `groups` and `page`: the labels of each group, then of
/// each event's combination and its host.
fn named(
    snapshot: &Snapshot<MemoryStorage>,
    groups: &SeriesGroups,
    page: &EventPage,
) -> Vec<Option<Vec<u8>>> {
    let label = |dimension, id| snapshot.label(dimension, id).map(<[u8]>::to_vec);
    let mut names = Vec::new();
    for group in groups.lists()[0].iter() {
        for (&dimension, part) in DIMENSIONS.iter().zip(group.key) {
            let KeyPart::Label(id) = *part else {
                panic!("a key part of {part:?}");
            };
            names.push(label(dimension, id));
        }
    }
    for event in page.events() {
        let EventValue::Combo(combo) = event.values[2] else {
            panic!("a series value of {:?}", event.values[2]);
        };
        match snapshot.combination(combo) {
            Some(ids) => names.extend(
                DIMENSIONS
                    .iter()
                    .zip(ids)
                    .map(|(&dimension, id)| label(dimension, id)),
            ),
            None => names.push(None),
        }
        let EventValue::Label(host) = event.values[3] else {
            panic!("a host of {:?}", event.values[3]);
        };
        names.push(label(HOST, host));
    }
    names
}

fn values(values: &[&[u8]]) -> Vec<Option<Vec<u8>>> {
    values.iter().map(|value| Some(value.to_vec())).collect()
}

// A result outlives its snapshot, and its ids are bare numbers. Once a round releases them and
// a write gives them to other values, the result's token differs from a later snapshot's,
// whose catalog names the new values, while the snapshot it was read from still names the old
// ones (§5.3 rule 5).
#[test]
fn a_result_retained_across_a_release_and_a_reuse_names_another_catalog() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let old = leak(["old-rule", "old-process", "old.com"]);
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, old)],
        &[(5, closed(Some(old), Some(b"old-host"), 1))],
        &[],
    );
    let id = combo(&tsdb, old).unwrap();
    let before = tsdb.reader().capture().unwrap();
    let (groups, page) = results(&before);
    assert_eq!(groups.catalog(), before.catalog());
    assert_eq!(page.catalog(), before.catalog());
    let old_values = values(&[
        b"old-rule",
        b"old-process",
        b"old.com",
        b"old-rule",
        b"old-process",
        b"old.com",
        b"old-host",
    ]);
    assert_eq!(named(&before, &groups, &page), old_values);

    let now = 2 * DAY_MS + HOUR_MS;
    let done = finish(&mut tsdb, now, &policy(keep(), age(24), age(24)));
    assert_eq!((done.combos_released, done.labels_released), (1, 4));
    let new = leak(["new-rule", "new-process", "new.com"]);
    commit(
        &mut tsdb,
        2,
        now,
        &[point(HOUR, now, new)],
        &[(now, closed(Some(new), Some(b"new-host"), 2))],
        &[],
    );
    assert_eq!(combo(&tsdb, new), Some(id));

    let after = tsdb.reader().capture().unwrap();
    assert_ne!(groups.catalog(), after.catalog());
    assert_ne!(page.catalog(), after.catalog());
    assert_eq!(named(&before, &groups, &page), old_values);
    assert_eq!(
        named(&after, &groups, &page),
        values(&[
            b"new-rule",
            b"new-process",
            b"new.com",
            b"new-rule",
            b"new-process",
            b"new.com",
            b"new-host",
        ])
    );
}

// Writes that only add ids, and a recycling round that releases none, keep the token: a
// retained result's ids name the same values through a later snapshot.
#[test]
fn growth_and_a_round_that_releases_nothing_keep_the_catalog_token() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    commit(
        &mut tsdb,
        1,
        0,
        &[point(HOUR, 0, LABELS)],
        &[(5, closed(Some(LABELS), Some(b"h"), 1))],
        &[],
    );
    let before = tsdb.reader().capture().unwrap();
    let (groups, page) = results(&before);

    // A candidate pinned again before the round checks it, which the round drops.
    let lonely = leak(["lonely", "lonely", "lonely.com"]);
    let pin = |key| PinOp::Pin {
        key,
        labels: lonely,
    };
    let other = leak(["other", "other", "other.com"]);
    commit(&mut tsdb, 2, 0, &[point(HOUR, 0, other)], &[], &[pin(b"a")]);
    commit(&mut tsdb, 3, 0, &[], &[], &[PinOp::Unpin { key: b"a" }]);
    commit(&mut tsdb, 4, 0, &[], &[], &[pin(b"b")]);
    let done = finish(&mut tsdb, 0, &policy(keep(), keep(), keep()));
    assert_eq!(
        (
            done.recycles,
            done.candidates_checked,
            done.combos_released + done.labels_released
        ),
        (1, 1, 0)
    );

    let after = tsdb.reader().capture().unwrap();
    assert_eq!(before.find_label(PROCESS, b"other"), None);
    assert!(after.find_label(PROCESS, b"other").is_some());
    assert_eq!(groups.catalog(), after.catalog());
    assert_eq!(page.catalog(), after.catalog());
    assert_eq!(
        named(&after, &groups, &page),
        named(&before, &groups, &page)
    );
}

// A batch records the newest extent of each (stream, entity) it holds, whatever the order of
// its points and rows, and a row's label value as its own entity.
#[test]
fn several_extents_of_one_entity_in_one_batch_record_the_newest() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let points = [
        point(HOUR, 0, LABELS),
        point(HOUR, 2 * DAY_MS, LABELS),
        point(HOUR, DAY_MS, LABELS),
        point(MINUTE, 3 * HOUR_MS, LABELS),
    ];
    let rows = [
        (3 * DAY_MS, closed(Some(LABELS), Some(b"h"), 1)),
        (5, closed(Some(LABELS), Some(b"h"), 2)),
    ];
    let written = commit(&mut tsdb, 1, 3 * DAY_MS, &points, &rows, &[]);
    assert_eq!(written.late, 0);
    let (id, host) = (
        Entity::Combo(combo(&tsdb, LABELS).unwrap()),
        Entity::Label(HOST, label(&tsdb, HOST, b"h").unwrap()),
    );
    let (records, index) = stored::all(&tsdb.storage().snapshot().unwrap());
    assert_eq!(
        records,
        [
            (id, MINUTES, 3),
            (id, HOURS, 2),
            (id, EVENTS, 3),
            (host, EVENTS, 3)
        ]
    );
    assert_eq!(
        index,
        [
            (MINUTES, 3, id),
            (HOURS, 2, id),
            (EVENTS, 3, id),
            (EVENTS, 3, host)
        ]
    );
}

// A write into an older extent that no watermark drops leaves a newer record as it is, and
// records a combination the stream had no record of.
#[test]
fn an_accepted_late_write_creates_an_absent_record() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let late = leak(["late", "late", "late.com"]);
    commit(
        &mut tsdb,
        1,
        5 * DAY_MS,
        &[point(HOUR, 5 * DAY_MS, LABELS)],
        &[],
        &[],
    );
    let points = [
        point(HOUR, 2 * DAY_MS, LABELS),
        point(HOUR, 2 * DAY_MS, late),
    ];
    let written = commit(&mut tsdb, 2, 5 * DAY_MS, &points, &[], &[]);
    assert_eq!(written.late, 0);
    let (kept, late) = (
        Entity::Combo(combo(&tsdb, LABELS).unwrap()),
        Entity::Combo(combo(&tsdb, late).unwrap()),
    );
    let (records, index) = stored::all(&tsdb.storage().snapshot().unwrap());
    assert_eq!(records, [(kept, HOURS, 5), (late, HOURS, 2)]);
    assert_eq!(index, [(HOURS, 2, late), (HOURS, 5, kept)]);
}

/// What one namespace saw a batch do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Seen {
    gets: u64,
    /// Entries scans visited.
    visits: u64,
    puts: u64,
    deletes: u64,
}

/// What batches did, per namespace.
#[derive(Clone, Default)]
struct Ops(Arc<Mutex<BTreeMap<u8, Seen>>>);

impl Ops {
    fn count(&self, ns: Ns, f: impl FnOnce(&mut Seen)) {
        f(self.0.lock().unwrap().entry(ns.0).or_default());
    }

    fn of(&self, ns: Ns) -> Seen {
        self.0
            .lock()
            .unwrap()
            .get(&ns.0)
            .copied()
            .unwrap_or_default()
    }

    /// The reads and the writes of every namespace: values got and entries visited, and
    /// entries put and deleted.
    fn totals(&self) -> (u64, u64) {
        let ops = self.0.lock().unwrap();
        let reads = ops.values().map(|seen| seen.gets + seen.visits).sum();
        let writes = ops.values().map(|seen| seen.puts + seen.deletes).sum();
        (reads, writes)
    }
}

/// A storage whose batches count what they read and write per namespace.
struct Probe<S> {
    storage: S,
    ops: Ops,
}

struct ProbeBatch<'s, B> {
    batch: B,
    ops: &'s Ops,
}

impl<S: Storage> Storage for Probe<S> {
    type Snapshot = S::Snapshot;
    type Batch<'s>
        = ProbeBatch<'s, S::Batch<'s>>
    where
        Self: 's;

    fn snapshot(&self) -> storage::Result<S::Snapshot> {
        self.storage.snapshot()
    }

    fn batch(&self) -> storage::Result<Self::Batch<'_>> {
        Ok(ProbeBatch {
            batch: self.storage.batch()?,
            ops: &self.ops,
        })
    }
}

impl<B: StorageRead> StorageRead for ProbeBatch<'_, B> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> storage::Result<T> {
        self.ops.count(ns, |seen| seen.gets += 1);
        self.batch.get(ns, key, f)
    }

    fn scan<T>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        mut f: impl FnMut(&[u8], &[u8]) -> ControlFlow<T>,
    ) -> storage::Result<Option<T>> {
        let mut visits = 0;
        let found = self.batch.scan(ns, range, direction, |key, value| {
            visits += 1;
            f(key, value)
        });
        self.ops.count(ns, |seen| seen.visits += visits);
        found
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> storage::Result<T> {
        self.batch.fragment(id, limit, f)
    }
}

impl<B: StorageWrite> StorageWrite for ProbeBatch<'_, B> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> storage::Result<()> {
        self.ops.count(ns, |seen| seen.puts += 1);
        self.batch.put(ns, key, value)
    }

    fn delete(&mut self, ns: Ns, key: &[u8]) -> storage::Result<()> {
        self.ops.count(ns, |seen| seen.deletes += 1);
        self.batch.delete(ns, key)
    }

    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> storage::Result<()> {
        self.batch.put_fragment(id, bytes)
    }

    fn delete_chunk(&mut self, chunk: ChunkId) -> storage::Result<()> {
        self.batch.delete_chunk(chunk)
    }

    fn commit(self, durability: Durability) -> Result<(), CommitFailure> {
        self.batch.commit(durability)
    }
}

fn probed(
    disk: &MemoryDisk,
    schema: crate::Schema,
    profile: Profile,
) -> (Tsdb<Probe<MemoryStorage>>, Ops) {
    let ops = Ops::default();
    let probe = Probe {
        storage: disk.open().unwrap(),
        ops: ops.clone(),
    };
    (Tsdb::open(probe, schema, profile).unwrap(), ops)
}

// A combination the head holds in a point's bucket costs no read: its journal record was
// recorded when it was written. Any other costs one read per (stream, entity) and batch, and a
// write only when it is new to the stream or reaches a newer extent. Rows always read.
#[test]
fn the_head_spares_the_read_of_a_combination_it_holds() {
    let disk = MemoryDisk::new();
    let (mut tsdb, ops) = probed(&disk, schema(), Profile::desktop());
    let mut last = ops.of(REFERENCES);
    let mut step = |tsdb: &mut Tsdb<_>,
                    batch,
                    points: &[SeriesPoint<'_>],
                    rows: &[(i64, Vec<Value<'static>>)]| {
        commit(tsdb, batch, 0, points, rows, &[]);
        let now = ops.of(REFERENCES);
        let seen = (now.gets - last.gets, now.puts - last.puts);
        last = now;
        seen
    };
    let twice = [point(HOUR, 0, LABELS), point(HOUR, 1, LABELS)];
    assert_eq!(step(&mut tsdb, 1, &twice, &[]), (1, 1));
    assert_eq!(step(&mut tsdb, 2, &[point(HOUR, 2, LABELS)], &[]), (0, 0));
    // Another bucket of the same extent: a read, no write.
    assert_eq!(
        step(&mut tsdb, 3, &[point(HOUR, HOUR_MS, LABELS)], &[]),
        (1, 0)
    );
    // A later extent: a read and a write.
    assert_eq!(
        step(&mut tsdb, 4, &[point(HOUR, DAY_MS, LABELS)], &[]),
        (1, 1)
    );
    let row = || (5, closed(Some(LABELS), Some(b"h"), 1));
    assert_eq!(step(&mut tsdb, 5, &[], &[row(), row()]), (2, 2));
    assert_eq!(step(&mut tsdb, 6, &[], &[row()]), (2, 0));
    // Another combination in a bucket the head holds: only the combinations it holds there are
    // spared.
    let other = leak(["other", "other", "other.com"]);
    assert_eq!(step(&mut tsdb, 7, &[point(HOUR, 3, other)], &[]), (1, 1));
    // The index moved with the record of the later extent.
    let (_, index) = stored::all(&tsdb.storage().snapshot().unwrap());
    let id = Entity::Combo(combo(&tsdb, LABELS).unwrap());
    assert!(index.contains(&(HOURS, 1, id)), "{index:?}");
    assert!(!index.contains(&(HOURS, 0, id)), "{index:?}");
}

// The head keeps a bucket's sums in a page per range of 256 combination ids. A batch whose
// combinations interleave with three pages reads only those the head does not hold: it spares
// the first and last of each page, and reads one inside a page, one between two pages and one
// past the last.
#[test]
fn the_head_spares_the_reads_of_the_combinations_of_its_pages() {
    let disk = MemoryDisk::new();
    let (mut tsdb, ops) = probed(&disk, schema(), Profile::desktop());
    let minutes: Vec<SeriesPoint<'static>> = (0..640)
        .map(|n| point(MINUTE, 0, leak(["rule", "process", &format!("{n}.com")])))
        .collect();
    commit(&mut tsdb, 1, 0, &minutes, &[], &[]);
    let labels: BTreeMap<u32, &'static [&'static [u8]]> = minutes
        .iter()
        .map(|point| (combo(&tsdb, point.labels).unwrap(), point.labels))
        .collect();
    assert!(labels.keys().copied().eq(1..=640));
    let hour = |ids: &[u32]| -> Vec<SeriesPoint<'static>> {
        ids.iter().map(|id| point(HOUR, 0, labels[id])).collect()
    };
    // Pages of ids 0..256, 256..512 and 512..768.
    let held = [10, 200, 300, 400, 520, 560];
    commit(&mut tsdb, 2, 0, &hour(&held), &[], &[]);

    let before = ops.of(REFERENCES);
    let ids = [10, 200, 250, 300, 350, 400, 450, 520, 560, 600];
    commit(&mut tsdb, 3, 0, &hour(&ids), &[], &[]);
    let after = ops.of(REFERENCES);
    assert_eq!((after.gets - before.gets, after.puts - before.puts), (4, 4));
}

/// Skewed draws from `0..n`, the cube of a uniform draw: a few values take most draws.
fn skewed(next: &mut impl FnMut() -> u64, n: u64) -> u64 {
    let unit = (next() % 1_000_000) as f64 / 1_000_000.0;
    (unit * unit * unit * n as f64) as u64
}

/// Writes traffic's schema (§11.1) six hours at the rate of 50,000 closed connections a day,
/// every 10 s, each with a point of its combination in the hour tier, drawn as the event
/// consolidation tests draw them but from `targets` targets. Checks what recording the
/// references costs: exactly one read per distinct combination of a batch's rows, and one per
/// combination of its points that the head does not hold in the hour yet; a record and an
/// index entry per stream the first time a combination appears there. Prints it against what
/// the batches read and write in all.
fn traffic_references(targets: u64) {
    use crate::write::chunking::tests::traffic_schema;

    let disk = MemoryDisk::new();
    let (mut tsdb, ops) = probed(&disk, traffic_schema(), Profile::desktop());
    let (count, every) = (50_000 / 4, 10_000i64);
    // Within day 0, so that every reference is in its first extent.
    let batches = (DAY_MS / 4 / every - 1) as u64;
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // The combinations each hour bucket's head holds.
    let mut held: BTreeMap<i64, BTreeSet<[u64; 3]>> = BTreeMap::new();
    let (mut expected, mut reads, mut writes, mut most) = (0, 0, 0, 0);
    let mut totals = (0, 0);
    let mut rows_written = 0;
    for batch in 0..batches {
        let now = (batch as i64 + 1) * every;
        let due = count * (batch + 1) / batches - count * batch / batches;
        let drawn: Vec<[u64; 3]> = (0..due)
            .map(|_| {
                [
                    skewed(&mut next, 12),
                    skewed(&mut next, 40),
                    skewed(&mut next, targets),
                ]
            })
            .collect();
        let labels: Vec<&'static [&'static [u8]]> = drawn
            .iter()
            .map(|&[rule, process, target]| {
                leak([
                    &format!("rule-{rule}"),
                    &format!("process-{process}.exe"),
                    &format!("target-{target}.example.com"),
                ])
            })
            .collect();
        let points: Vec<SeriesPoint<'_>> = labels
            .iter()
            .map(|labels| SeriesPoint {
                tier: TierId(0),
                at: Timestamp(now),
                labels,
                fields: &[1_000, 10_000, 1],
            })
            .collect();
        let values: Vec<[Value<'_>; 7]> = labels
            .iter()
            .map(|labels| {
                [
                    Value::I64(now - 5_000),
                    Value::I64(now - 5_000),
                    Value::U64(1_000),
                    Value::U64(10_000),
                    Value::Series(labels),
                    Value::Bytes(UUID),
                    Value::Null,
                ]
            })
            .collect();
        let events: Vec<EventRow<'_>> = values
            .iter()
            .enumerate()
            .map(|(index, values)| EventRow {
                stream: CLOSED,
                key: now - index as i64,
                seq: tsdb.next_event_seq(),
                values,
            })
            .collect();
        let distinct: BTreeSet<[u64; 3]> = drawn.iter().copied().collect();
        let bucket = held.entry(now.div_euclid(HOUR_MS)).or_default();
        expected += distinct.len() + distinct.difference(bucket).count();
        bucket.extend(distinct);
        let before = (ops.of(REFERENCES), ops.of(REFERENCE_INDEX), ops.totals());
        let input = Input {
            batch_id: batch + 1,
            now: Timestamp(now),
            series: &points,
            events: &events,
            aux: &[],
            pins: &[],
        };
        tsdb.write(&input, Durability::Durable).unwrap();
        let (records, index) = (ops.of(REFERENCES), ops.of(REFERENCE_INDEX));
        let read = records.gets - before.0.gets;
        reads += read;
        most = most.max(read);
        writes += (records.puts - before.0.puts)
            + (index.puts - before.1.puts)
            + (index.deletes - before.1.deletes);
        let after = ops.totals();
        totals = (
            totals.0 + after.0 - before.2.0,
            totals.1 + after.1 - before.2.1,
        );
        rows_written += due;
    }
    assert_eq!(reads, expected as u64);
    let per = |total: u64| total as f64 / batches as f64;
    let combos = tsdb.current().catalog.live_combos().len() as u64;
    println!(
        "traffic, {targets} targets: {batches} batches of {:.2} rows and points, {combos} \
         combinations; the \
         references read {:.2} records a batch (at most {most}) and wrote {:.2} entries, of \
         {:.2} reads and {:.2} writes a batch in all",
        per(rows_written),
        per(reads),
        per(writes),
        per(totals.0),
        per(totals.1),
    );
    // A pair is new once per stream: two writes for each (record and index entry).
    assert_eq!(writes, 2 * 2 * combos);
}

// The cost of the references on traffic, with the targets of the event consolidation tests,
// which make almost every connection a new combination, and with ten, which leave thousands,
// about what a real library holds (§5.1).
#[test]
fn the_references_of_traffic_cost_a_read_per_new_pair() {
    traffic_references(2_000);
    traffic_references(10);
}

// A full catalog folds new combinations (§5.2). A folded combination is an ordinary one: its
// data references it, and it is recycled with the rest of its day, which frees the room a new
// combination then takes whole.
#[test]
fn folded_combinations_are_recycled_and_their_room_reused() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        catalog: 64 * 1024,
        ..chunking()
    };
    let mut tsdb = open_with(&disk, profile);
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 1, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let mut batch = 1;
    let mut folded = 0;
    while folded == 0 {
        batch += 1;
        assert!(batch < 100, "the catalog never filled");
        let points: Vec<SeriesPoint<'_>> = (0..50)
            .map(|n| {
                let labels = leak(["match", &format!("p{batch}-{n}"), &format!("t{batch}-{n}")]);
                point(HOUR, HOUR_MS, labels)
            })
            .collect();
        let rows = [(5, closed(Some(points[49].labels), None, 1))];
        folded = commit(&mut tsdb, batch, HOUR_MS, &points, &rows, &[]).folded;
    }
    let live = tsdb.current().catalog.live_combos().len();
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), age(24)));
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(done.combos_released as usize, live - 1);
    assert_eq!(tsdb.current().catalog.live_combos().len(), 1);
    assert!(combo(&tsdb, LABELS).is_some());
    let fresh = leak(["match", "fresh", "fresh.com"]);
    let written = commit(
        &mut tsdb,
        batch + 1,
        now,
        &[point(HOUR, now, fresh)],
        &[],
        &[],
    );
    assert_eq!(written.folded, 0);
    assert!(combo(&tsdb, fresh).is_some());
}

// A recycling round that fails changes nothing: one whose commit is not applied is done again
// by the next call, and one whose backend fails part-way needs recovery. A crash and a reopen
// then find the same work, and maintenance finishes it.
#[test]
fn a_failed_recycling_round_changes_nothing_and_a_reopen_finishes_it() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let old = leak(["old", "old", "old.com"]);
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, old)],
        &[(5, closed(Some(old), Some(b"old-host"), 1))],
        &[],
    );
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), age(24)));
    until_recycling(&mut tsdb, now, &retention);
    let (version, storage) = (tsdb.current(), dump_storage(&tsdb));

    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    let error = run(&mut tsdb, now, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Commit {
                source: CommitFailure::NotApplied { .. }
            }
        ),
        "{error:?}"
    );
    assert!(Arc::ptr_eq(&version, &tsdb.current()));
    assert_eq!(dump_storage(&tsdb), storage);
    check_settled(&tsdb);

    tsdb.storage().fail_next(OpClass::Write);
    let error = run(&mut tsdb, now, &retention, 1).unwrap_err();
    assert!(matches!(error, MaintainError::Recycle { .. }), "{error:?}");
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(Arc::ptr_eq(&version, &tsdb.current()));
    assert!(tsdb.registry().needs_recovery());
    tsdb.into_storage().crash();

    let mut tsdb = open_with(&disk, chunking());
    assert_eq!(without_meta(dump_storage(&tsdb)), without_meta(storage));
    assert_eq!(next_round(&tsdb, now, &retention), Some(Round::Recycle));
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!((done.combos_released, done.labels_released), (1, 4));
    assert_eq!(combo(&tsdb, old), None);
    assert_eq!(candidates(&tsdb), []);
}

// A catalog at its limit still recycles: a fresh draft has room for one release (the reserve),
// so each round releases an id or more, and stops at the first the draft refuses, which waits
// for the next round. Here a combination of values it alone uses goes, then its values, and
// 1,200 hosts, whose table spans pages.
#[test]
fn a_catalog_at_its_limit_releases_an_id_or_more_each_round() {
    let disk = MemoryDisk::new();
    let limit = 4096;
    let profile = Profile {
        cleanup_chunks: limit,
        ..chunking()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let alone = leak(["alone", "alone", "alone.com"]);
    let rows: Vec<(i64, Vec<Value<'static>>)> = (0..1_200)
        .map(|n| {
            let host = String::leak(format!("h{n}")).as_bytes();
            (5, closed(Some(alone), Some(host), 1))
        })
        .collect();
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, alone)],
        &rows,
        &[],
    );
    let day = DAY_MS + HOUR_MS;
    let kept = [(day, closed(Some(LABELS), Some(b"kept"), 1))];
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &kept, &[]);
    drop(tsdb.into_storage());
    // The budget of exactly what the catalog holds once loaded.
    let full = open_with(&disk, profile.clone()).current().catalog.bytes();
    let mut tsdb = open_with(
        &disk,
        Profile {
            catalog: full,
            ..profile
        },
    );

    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), age(24)));
    let (mut released, mut rounds, mut refused) = (0, 0, false);
    for _ in 0..=work(&tsdb) {
        let done = run(&mut tsdb, now, &retention, 1).unwrap();
        if done.recycles == 1 {
            let ids = done.combos_released + done.labels_released;
            assert!(ids > 0, "{done:?}");
            (released, rounds) = (released + ids, rounds + 1);
            let examined = done.references_forgotten + done.candidates_checked;
            refused |= examined < limit as u64 && !candidates(&tsdb).is_empty();
        }
        if !done.more {
            break;
        }
    }
    assert_eq!(released, 1 + 3 + 1_200);
    assert!(refused, "no release was refused in {rounds} rounds");
    assert_eq!(combo(&tsdb, alone), None);
    assert!(combo(&tsdb, LABELS).is_some());
    assert!(label(&tsdb, HOST, b"kept").is_some());
    assert!(tsdb.current().catalog.bytes() <= full);
    check_settled(&tsdb);
}

// A round examines at most D index entries and candidates, and the rounds together examine each
// once: here ten combinations of a day, each in a tier and in a row with a host of its own.
#[test]
fn a_round_examines_at_most_d_entries_and_candidates() {
    let disk = MemoryDisk::new();
    let limit = 3;
    let profile = Profile {
        cleanup_chunks: limit,
        ..chunking()
    };
    let mut tsdb = open_with(&disk, profile);
    let combos: Vec<_> = (0..10)
        .map(|n| leak(["match", &format!("p{n}"), &format!("t{n}")]))
        .collect();
    let points: Vec<_> = combos
        .iter()
        .map(|&labels| point(HOUR, HOUR_MS, labels))
        .collect();
    let rows: Vec<_> = combos
        .iter()
        .enumerate()
        .map(|(n, &labels)| {
            let host = String::leak(format!("h{n}")).as_bytes();
            (5, closed(Some(labels), Some(host), 1))
        })
        .collect();
    commit(&mut tsdb, 1, HOUR_MS, &points, &rows, &[]);
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), age(24)));
    let (mut forgotten, mut checked, mut rounds) = (0, 0, 0);
    for _ in 0..=work(&tsdb) {
        let done = run(&mut tsdb, now, &retention, 1).unwrap();
        assert!(
            done.references_forgotten + done.candidates_checked <= limit as u64,
            "{done:?}"
        );
        forgotten += done.references_forgotten;
        checked += done.candidates_checked;
        rounds += done.recycles;
        if !done.more {
            break;
        }
    }
    // The hour tier's ten, and the ten combinations and ten hosts of `closed`.
    assert_eq!(forgotten, 30);
    // Each combination once its last reference went, each host, and the two values of its own
    // each combination left unused: none stale, none twice.
    assert_eq!(checked, 10 + 10 + 20);
    assert_eq!(rounds, (30 + 40u64).div_ceil(limit as u64));
    assert_eq!(tsdb.current().catalog.live_combos().len(), 1);
}

/// What planning the next round reads once it recycles the references of a day of `combos`
/// combinations.
fn planning_reads(combos: usize) -> usize {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, chunking());
    let points: Vec<_> = (0..combos)
        .map(|n| point(HOUR, HOUR_MS, leak(["match", &format!("p{n}"), "t"])))
        .collect();
    commit(&mut tsdb, 1, HOUR_MS, &points, &[], &[]);
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), keep()));
    until_recycling(&mut tsdb, now, &retention);
    let snapshot = tsdb.storage().snapshot().unwrap();
    let tally = Tally {
        read: &snapshot,
        reads: std::cell::Cell::new(0),
    };
    let request = Maintain {
        now: Timestamp(now),
        retention: &retention,
        rounds: 1,
    };
    let round = maintain::plan(&tally, &tsdb.round_context(&tsdb.current()), &request).unwrap();
    assert_eq!(round, Some(Round::Recycle));
    tally.reads.get()
}

// Planning a recycling round reads a candidate, then per stream its record, its first chunk up
// to the watermark and its first reference there, however many references wait.
#[test]
fn planning_reads_do_not_grow_with_the_references() {
    assert_eq!(planning_reads(4), planning_reads(64));
}

/// The values the model draws from: 24 combinations and 3 hosts.
const RULES: [&str; 2] = ["r0", "r1"];
const PROCESSES: [&str; 3] = ["p0", "p1", "p2"];
const TARGETS: [&str; 4] = ["t0", "t1", "t2", "t3"];
const HOSTS: [&str; 3] = ["h0", "h1", "h2"];
const COMBOS: usize = 24;

/// The values of combination `n` of the model.
fn pooled(n: usize) -> [&'static str; 3] {
    [RULES[n % 2], PROCESSES[n / 2 % 3], TARGETS[n / 6 % 4]]
}

type Labels = Vec<Vec<u8>>;
/// A `closed` row of the model: its combination, its host and its upload.
type Row = (Option<usize>, Option<usize>, u64);
/// A `closed` row as a snapshot shows it: key, incarnation, sequence, labels, host, upload.
type Shown = (i64, u64, u64, Option<Labels>, Option<Vec<u8>>, u64);

fn labels_of(n: usize) -> Labels {
    pooled(n)
        .iter()
        .map(|value| value.as_bytes().to_vec())
        .collect()
}

#[derive(Clone, Debug)]
enum Step {
    /// Moves the clock `advance` minutes on, then writes points (tier, minutes back,
    /// combination) and `closed` rows (minutes back, combination, host).
    Write {
        advance: i64,
        points: Vec<(TierId, i64, usize)>,
        rows: Vec<(i64, Option<usize>, Option<usize>)>,
    },
    /// Pins a combination under a key of its own.
    Pin(usize),
    /// Takes off the pin of one of the keys in use, if any.
    Unpin(usize),
    /// Moves the clock on, then maintains with the ages, in hours, of the minute tier, the
    /// hour tier and `closed`, and a budget of rounds.
    Maintain {
        advance: i64,
        ages: [Option<u64>; 3],
        rounds: usize,
    },
    /// Maintains until nothing is left.
    Converge {
        ages: [Option<u64>; 3],
    },
    /// Captures a snapshot, which the later steps check again.
    Capture,
    Reopen,
}

fn ages() -> impl Strategy<Value = [Option<u64>; 3]> {
    (
        prop_oneof![Just(None), Just(Some(2)), Just(Some(24))],
        prop_oneof![Just(None), Just(Some(24)), Just(Some(48))],
        prop_oneof![Just(None), Just(Some(24)), Just(Some(48))],
    )
        .prop_map(|(minutes, hours, closed)| [minutes, hours, closed])
}

fn step() -> impl Strategy<Value = Step> {
    let tier = prop_oneof![Just(MINUTE), Just(HOUR)];
    let back = 0..72 * 60i64;
    let write = (
        0..720i64,
        prop::collection::vec((tier, back.clone(), 0..COMBOS), 0..5),
        prop::collection::vec(
            (
                back,
                prop::option::of(0..COMBOS),
                prop::option::of(0..HOSTS.len()),
            ),
            0..4,
        ),
    )
        .prop_map(|(advance, points, rows)| Step::Write {
            advance,
            points,
            rows,
        });
    let maintain =
        (0..720i64, ages(), 0..6usize).prop_map(|(advance, ages, rounds)| Step::Maintain {
            advance,
            ages,
            rounds,
        });
    prop_oneof![
        4 => write,
        1 => (0..COMBOS).prop_map(Step::Pin),
        1 => any::<usize>().prop_map(Step::Unpin),
        2 => maintain,
        1 => ages().prop_map(|ages| Step::Converge { ages }),
        1 => Just(Step::Capture),
        1 => Just(Step::Reopen),
    ]
}

fn ages_policy(ages: [Option<u64>; 3]) -> RetentionPolicy {
    let [minutes, hours, closed] = ages.map(|hours| hours.map_or_else(keep, age));
    policy(minutes, hours, closed)
}

/// What the writes and pins left, before retention.
#[derive(Default)]
struct Model {
    /// (tier, bucket, combination) → the first field's sum.
    series: BTreeMap<(TierId, i64, usize), u64>,
    /// `closed` rows: (key, incarnation, sequence) → combination, host and upload.
    rows: BTreeMap<(i64, u64, u64), Row>,
    /// The pin keys in use, with the combination each pins.
    pins: BTreeMap<u64, usize>,
}

/// What a snapshot shows of the model's data.
#[derive(Debug, PartialEq, Eq)]
struct View {
    /// Per tier, the first field's sum by the labels of its combinations.
    series: Vec<BTreeMap<Labels, u64>>,
    /// The `closed` rows in key order.
    rows: Vec<Shown>,
}

impl Model {
    /// The points left by the watermarks `marks` of the minute tier, the hour tier and
    /// `closed`, by tier, bucket and combination.
    fn points<'m>(
        &'m self,
        grids: &'m [Grid],
        marks: &'m [Option<i64>; 3],
    ) -> impl Iterator<Item = (TierId, i64, usize, u64)> + 'm {
        self.series
            .iter()
            .map(|(&(tier, bucket, combo), &sum)| (tier, bucket, combo, sum))
            .filter(|&(tier, bucket, ..)| {
                let extent = grids[usize::from(tier.0)].extent_of_bucket(bucket);
                marks[usize::from(tier.0)].is_none_or(|expired| extent > expired)
            })
    }

    /// The rows the watermark of `closed` leaves: key, incarnation, sequence, combination,
    /// host and upload.
    fn kept_rows<'m>(
        &'m self,
        marks: &'m [Option<i64>; 3],
    ) -> impl Iterator<Item = (i64, u64, u64, Option<usize>, Option<usize>, u64)> + 'm {
        self.rows
            .iter()
            .filter(|&(&(key, ..), _)| {
                marks[2].is_none_or(|expired| key.div_euclid(DAY_MS) > expired)
            })
            .map(|(&(key, incarnation, seq), &(combo, host, upload))| {
                (key, incarnation, seq, combo, host, upload)
            })
    }

    fn view(&self, grids: &[Grid], marks: &[Option<i64>; 3]) -> View {
        let mut series = vec![BTreeMap::new(); 2];
        for (tier, _, combo, sum) in self.points(grids, marks) {
            *series[usize::from(tier.0)]
                .entry(labels_of(combo))
                .or_default() += sum;
        }
        let rows = self
            .kept_rows(marks)
            .map(|(key, incarnation, seq, combo, host, upload)| {
                let host = host.map(|host| HOSTS[host].as_bytes().to_vec());
                (key, incarnation, seq, combo.map(labels_of), host, upload)
            })
            .collect();
        View { series, rows }
    }
}

/// What `snapshot` shows, every id resolved through its own catalog.
fn observe(snapshot: &Snapshot<MemoryStorage>) -> View {
    let resolve = |ids: Vec<u32>| -> Labels {
        DIMENSIONS
            .iter()
            .zip(ids)
            .map(|(&dimension, id)| {
                snapshot
                    .label(dimension, id)
                    .expect("a live value")
                    .to_vec()
            })
            .collect()
    };
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&DIMENSIONS),
    }];
    let series = [MINUTE, HOUR]
        .into_iter()
        .map(|tier| {
            let groups = snapshot
                .series(&SeriesQuery {
                    range: all(),
                    tier,
                    requests: &requests,
                    fields: FieldMask(1),
                })
                .unwrap();
            groups.lists()[0]
                .iter()
                .map(|group| {
                    let ids = group
                        .key
                        .iter()
                        .map(|part| match *part {
                            KeyPart::Label(id) => id,
                            other => panic!("a key part of {other:?}"),
                        })
                        .collect();
                    (resolve(ids), group.sums[0])
                })
                .collect()
        })
        .collect();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: all(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 100_000,
        })
        .unwrap();
    let rows = page
        .events()
        .iter()
        .map(|event| {
            let labels = match &event.values[2] {
                EventValue::Combo(combo) => Some(resolve(
                    snapshot
                        .combination(*combo)
                        .expect("a live combination")
                        .collect(),
                )),
                EventValue::Null => None,
                other => panic!("a series value of {other:?}"),
            };
            let host = match &event.values[3] {
                EventValue::Label(id) => {
                    Some(snapshot.label(HOST, *id).expect("a live host").to_vec())
                }
                EventValue::Null => None,
                other => panic!("a host of {other:?}"),
            };
            let EventValue::U64(upload) = event.values[1] else {
                panic!("an upload of {:?}", event.values[1]);
            };
            (
                event.key,
                event.seq.incarnation,
                event.seq.seq,
                labels,
                host,
                upload,
            )
        })
        .collect();
    View { series, rows }
}

fn watermarks(tsdb: &Tsdb<MemoryStorage>) -> [Option<i64>; 3] {
    let snapshot = tsdb.storage().snapshot().unwrap();
    [MINUTES, HOURS, EVENTS].map(|stream| manifest::read_watermark(&snapshot, stream).unwrap())
}

/// Checks that each combination and host the model keeps has a reference of its stream at
/// its extent or a newer one.
fn check_references(
    tsdb: &Tsdb<MemoryStorage>,
    model: &Model,
    marks: &[Option<i64>; 3],
) -> Result<(), TestCaseError> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let grids = tsdb.shape().grids.clone();
    let combo_of = |n: usize| {
        let labels: Vec<&[u8]> = pooled(n).iter().map(|value| value.as_bytes()).collect();
        Entity::Combo(combo(tsdb, &labels).expect("a live combination"))
    };
    for (tier, bucket, n, _) in model.points(&grids, marks) {
        let extent = grids[usize::from(tier.0)].extent_of_bucket(bucket);
        let newest = references::newest(&snapshot, combo_of(n), StreamRef::Tier(tier)).unwrap();
        prop_assert!(
            newest >= Some(extent),
            "{n} in {tier:?} at {extent}: {newest:?}"
        );
    }
    for (key, _, _, combo, host, _) in model.kept_rows(marks) {
        let extent = key.div_euclid(DAY_MS);
        let host = host.map(|host| {
            let id = label(tsdb, HOST, HOSTS[host].as_bytes()).expect("a live host");
            Entity::Label(HOST, id)
        });
        for entity in combo.map(combo_of).into_iter().chain(host) {
            let newest = references::newest(&snapshot, entity, EVENTS).unwrap();
            prop_assert!(newest >= Some(extent), "{entity:?} at {extent}: {newest:?}");
        }
    }
    Ok(())
}

/// Checks that, maintenance having converged, the catalog holds exactly what the kept data
/// and the pins reference, and that each value counts the live combinations that use it.
fn check_converged(
    tsdb: &Tsdb<MemoryStorage>,
    model: &Model,
    marks: &[Option<i64>; 3],
) -> Result<(), TestCaseError> {
    let grids = tsdb.shape().grids.clone();
    let referenced: BTreeSet<usize> = model
        .points(&grids, marks)
        .map(|(_, _, combo, _)| combo)
        .chain(model.kept_rows(marks).filter_map(|row| row.3))
        .chain(model.pins.values().copied())
        .collect();
    let catalog = &tsdb.current().catalog;
    let live: BTreeMap<Labels, Vec<u32>> = catalog
        .live_combos()
        .into_iter()
        .map(|(_, ids)| {
            let labels = DIMENSIONS
                .iter()
                .zip(&ids)
                .map(|(&dimension, &id)| catalog.label(dimension, id).unwrap().to_vec())
                .collect();
            (labels, ids)
        })
        .collect();
    let expected: BTreeSet<Labels> = referenced.iter().map(|&n| labels_of(n)).collect();
    prop_assert_eq!(live.keys().cloned().collect::<BTreeSet<_>>(), expected);
    let snapshot = tsdb.storage().snapshot().unwrap();
    for (position, &dimension) in DIMENSIONS.iter().enumerate() {
        let values: BTreeSet<Vec<u8>> =
            live.keys().map(|labels| labels[position].clone()).collect();
        let found: Vec<(u32, Vec<u8>)> = catalog.live_labels(dimension);
        prop_assert_eq!(
            found
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<BTreeSet<_>>(),
            values,
            "{:?}",
            dimension
        );
        for (id, _) in found {
            let users = live.values().filter(|ids| ids[position] == id).count() as u64;
            prop_assert_eq!(label_uses(&snapshot, dimension, id).unwrap(), users);
        }
    }
    let hosts: BTreeSet<Vec<u8>> = model
        .kept_rows(marks)
        .filter_map(|row| row.4)
        .map(|host| HOSTS[host].as_bytes().to_vec())
        .collect();
    let found: BTreeSet<Vec<u8>> = catalog
        .live_labels(HOST)
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    prop_assert_eq!(found, hosts);
    prop_assert_eq!(stored::candidates(&snapshot), []);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    // After every step, the queries of the current version and of every snapshot captured
    // before equal the model, every id their data holds resolving to its own values, and the
    // write path has recorded every reference the kept data holds. Once maintenance converges,
    // the catalog holds exactly the combinations the kept data and the pins reference and the
    // values they and the kept rows use, each counting its combinations.
    #[test]
    fn recycling_matches_a_model(
        cleanup_chunks in 1..4usize,
        chunk_all in any::<bool>(),
        steps in prop::collection::vec(step(), 1..24),
    ) {
        let disk = MemoryDisk::new();
        let profile = Profile {
            cleanup_chunks,
            journal_bytes: if chunk_all { 1 } else { Profile::desktop().journal_bytes },
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile.clone());
        let grids = tsdb.shape().grids.clone();
        let pools: Vec<&'static [&'static [u8]]> = (0..COMBOS)
            .map(|n| -> &'static [&'static [u8]] {
                Vec::leak(pooled(n).iter().map(|value| value.as_bytes()).collect())
            })
            .collect();
        let mut model = Model::default();
        let mut held: Vec<(Snapshot<MemoryStorage>, View)> = Vec::new();
        let (mut now, mut batch, mut next_key) = (0i64, 0u64, 0u64);

        for step in &steps {
            let marks = watermarks(&tsdb);
            match step {
                Step::Write { advance, points, rows } => {
                    now += advance * MINUTE_MS;
                    batch += 1;
                    let mut late = 0;
                    let series: Vec<SeriesPoint<'_>> = points
                        .iter()
                        .map(|&(tier, back, n)| SeriesPoint {
                            tier,
                            at: Timestamp(now - back * MINUTE_MS),
                            labels: pools[n],
                            fields: &[1, 0],
                        })
                        .collect();
                    for (point, &(_, _, n)) in series.iter().zip(points) {
                        let grid = grids[usize::from(point.tier.0)];
                        let bucket = grid.bucket().index_of(point.at);
                        let expired = marks[usize::from(point.tier.0)];
                        if expired.is_some_and(|expired| grid.extent_of_bucket(bucket) <= expired) {
                            late += 1;
                        } else {
                            *model.series.entry((point.tier, bucket, n)).or_default() += 1;
                        }
                    }
                    let values: Vec<Vec<Value<'static>>> = rows
                        .iter()
                        .map(|&(_, n, host)| {
                            let host = host.map(|host| HOSTS[host].as_bytes());
                            closed(n.map(|n| pools[n]), host, batch)
                        })
                        .collect();
                    let events: Vec<EventRow<'_>> = rows
                        .iter()
                        .zip(&values)
                        .map(|(&(back, ..), values)| EventRow {
                            stream: CLOSED,
                            key: now - back * MINUTE_MS,
                            seq: tsdb.next_event_seq(),
                            values,
                        })
                        .collect();
                    for (event, &(_, n, host)) in events.iter().zip(rows) {
                        if marks[2].is_some_and(|expired| event.key.div_euclid(DAY_MS) <= expired) {
                            late += 1;
                        } else {
                            let order = (event.key, event.seq.incarnation, event.seq.seq);
                            model.rows.insert(order, (n, host, batch));
                        }
                    }
                    let input = Input {
                        batch_id: batch,
                        now: Timestamp(now),
                        series: &series,
                        events: &events,
                        aux: &[],
                        pins: &[],
                    };
                    let written = tsdb.write(&input, Durability::Durable).unwrap();
                    prop_assert_eq!(written.late, late);
                }
                Step::Pin(n) => {
                    batch += 1;
                    next_key += 1;
                    let key = next_key.to_be_bytes();
                    let pins = [PinOp::Pin { key: &key, labels: pools[*n] }];
                    commit(&mut tsdb, batch, now, &[], &[], &pins);
                    model.pins.insert(next_key, *n);
                }
                Step::Unpin(pick) => {
                    let Some(&key) = model.pins.keys().nth(pick % model.pins.len().max(1)) else {
                        continue;
                    };
                    batch += 1;
                    let bytes = key.to_be_bytes();
                    commit(&mut tsdb, batch, now, &[], &[], &[PinOp::Unpin { key: &bytes }]);
                    model.pins.remove(&key);
                }
                Step::Maintain { advance, ages, rounds } => {
                    now += advance * MINUTE_MS;
                    run(&mut tsdb, now, &ages_policy(*ages), *rounds).unwrap();
                }
                Step::Converge { ages } => {
                    finish(&mut tsdb, now, &ages_policy(*ages));
                    check_converged(&tsdb, &model, &watermarks(&tsdb))?;
                }
                Step::Capture => {
                    let snapshot = tsdb.reader().capture().unwrap();
                    held.push((snapshot, model.view(&grids, &marks)));
                }
                Step::Reopen => {
                    held.clear();
                    drop(tsdb.into_storage());
                    tsdb = open_with(&disk, profile.clone());
                }
            }
            let marks = watermarks(&tsdb);
            let snapshot = tsdb.reader().capture().unwrap();
            prop_assert_eq!(observe(&snapshot), model.view(&grids, &marks));
            drop(snapshot);
            check_references(&tsdb, &model, &marks)?;
            for (snapshot, view) in &held {
                prop_assert_eq!(&observe(snapshot), view);
            }
            check_settled(&tsdb);
        }
        held.clear();
        let marks = watermarks(&tsdb);
        finish(&mut tsdb, now, &ages_policy([None; 3]));
        check_converged(&tsdb, &model, &marks)?;
    }
}

// A value a released combination left without uses is kept when a new combination uses it
// before a round checks it: the check reads its uses again.
#[test]
fn a_value_a_new_combination_uses_before_its_check_is_kept() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        cleanup_chunks: 1,
        ..chunking()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let old = leak(["match", "shared", "old.com"]);
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[point(HOUR, HOUR_MS, old)],
        &[],
        &[],
    );
    let day = DAY_MS + HOUR_MS;
    commit(&mut tsdb, 2, day, &[point(HOUR, day, LABELS)], &[], &[]);
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), age(24), keep()));
    until_recycling(&mut tsdb, now, &retention);
    // A round each: the reference goes, then the combination, which leaves its own process and
    // target candidates.
    for _ in 0..2 {
        run(&mut tsdb, now, &retention, 1).unwrap();
    }
    assert_eq!(combo(&tsdb, old), None);
    let process = label(&tsdb, PROCESS, b"shared").unwrap();
    let target = label(&tsdb, TARGET, b"old.com").unwrap();
    assert_eq!(
        candidates(&tsdb),
        [
            Entity::Label(PROCESS, process),
            Entity::Label(TARGET, target)
        ]
    );
    // A reopen clears the backlog, so the next commit runs no round before its batch.
    drop(tsdb.into_storage());
    let mut tsdb = open_with(&disk, profile);
    let new = leak(["match", "shared", "new.com"]);
    commit(&mut tsdb, 3, now, &[point(HOUR, now, new)], &[], &[]);
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!((done.candidates_checked, done.labels_released), (2, 1));
    assert_eq!(label(&tsdb, PROCESS, b"shared"), Some(process));
    assert_eq!(label(&tsdb, TARGET, b"old.com"), None);
    assert!(combo(&tsdb, new).is_some());
}

// A recycling round reserves the catalog pages its draft copied, which stay allocated beside
// the current catalog until publication: without room, it fails and changes nothing.
#[test]
fn a_recycling_round_reserves_its_catalog_copies() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let lonely = leak(["lonely", "lonely", "lonely.com"]);
    let pin = [PinOp::Pin {
        key: b"a",
        labels: lonely,
    }];
    commit(&mut tsdb, 1, 0, &[], &[], &pin);
    commit(&mut tsdb, 2, 0, &[], &[], &[PinOp::Unpin { key: b"a" }]);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));
    let budget = tsdb.staging_budget();
    let full = budget
        .try_reserve("test", Profile::desktop().staging - budget.reserved())
        .unwrap();
    let retention = policy(keep(), keep(), keep());
    let error = run(&mut tsdb, 0, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Recycle {
                source: RecycleError::ReserveStaging { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(dump_storage(&tsdb), storage);
    drop(full);
    let done = finish(&mut tsdb, 0, &retention);
    assert_eq!((done.combos_released, done.labels_released), (1, 3));
}

// A candidate naming a label dimension the schema does not have is damage: the round fails as
// corrupted instead of reading a dictionary that is not there.
#[test]
fn a_candidate_of_an_unknown_dimension_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    references::queue(&mut batch, Entity::Label(LabelId(99), 1)).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let error = run(&mut tsdb, 0, &policy(keep(), keep(), keep()), 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Recycle {
                source: RecycleError::UnknownDimension {
                    dimension: LabelId(99)
                }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

// A count of 0 uses deletes its record, so a stored one is damage: a round that checks the
// value fails as corrupted instead of releasing it while a live combination uses it, and
// changes nothing.
#[test]
fn a_stored_count_of_zero_uses_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    commit(&mut tsdb, 1, 0, &[point(HOUR, 0, LABELS)], &[], &[]);
    let curl = label(&tsdb, PROCESS, b"curl").unwrap();
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    references::queue(&mut batch, Entity::Label(PROCESS, curl)).unwrap();
    raw::put_uses_record(&mut batch, PROCESS, curl, 0).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let stored = dump_storage(&tsdb);

    let error = run(&mut tsdb, 0, &policy(keep(), keep(), keep()), 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Recycle {
                source: RecycleError::CheckUses {
                    dimension,
                    id,
                    source: UsesError::ZeroUses { .. },
                },
            } if dimension == PROCESS && id == curl
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert_eq!(dump_storage(&tsdb), stored);
    assert_eq!(label(&tsdb, PROCESS, b"curl"), Some(curl));
    assert!(combo(&tsdb, LABELS).is_some());
}

// A pin count of 0 deletes its record too, so a stored one is damage: a round that checks the
// pinned combination fails as corrupted instead of releasing it, and changes nothing.
#[test]
fn a_stored_count_of_zero_pins_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let lonely = leak(["lonely", "lonely", "lonely.com"]);
    let pin = [PinOp::Pin {
        key: b"conn",
        labels: lonely,
    }];
    commit(&mut tsdb, 1, 0, &[], &[], &pin);
    let id = combo(&tsdb, lonely).unwrap();
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    references::queue(&mut batch, Entity::Combo(id)).unwrap();
    raw::put_pin_record(&mut batch, id, 0).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let stored = dump_storage(&tsdb);

    let error = run(&mut tsdb, 0, &policy(keep(), keep(), keep()), 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Recycle {
                source: RecycleError::CheckPins {
                    id: checked,
                    source: PinError::ZeroPins { .. },
                },
            } if checked == id
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert_eq!(dump_storage(&tsdb), stored);
    assert_eq!(combo(&tsdb, lonely), Some(id));
}

// A candidate that new data references before a round checks it is kept: the check reads its
// references again. Here a combination its last unpin queued, and a host queued once the row
// that held it was cleaned up, each written again.
#[test]
fn a_candidate_referenced_again_before_its_check_is_kept() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        cleanup_chunks: 1,
        ..chunking()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let lonely = leak(["lonely", "lonely", "lonely.com"]);
    let host: &'static [u8] = b"old-host";
    let pin = [PinOp::Pin {
        key: b"a",
        labels: lonely,
    }];
    commit(
        &mut tsdb,
        1,
        HOUR_MS,
        &[],
        &[(5, closed(None, Some(host), 1))],
        &pin,
    );
    commit(
        &mut tsdb,
        2,
        HOUR_MS,
        &[],
        &[],
        &[PinOp::Unpin { key: b"a" }],
    );
    let day = DAY_MS + HOUR_MS;
    let kept = [(day, closed(Some(LABELS), Some(b"kept"), 1))];
    commit(&mut tsdb, 3, day, &[point(HOUR, day, LABELS)], &kept, &[]);
    let (id, host_id) = (
        combo(&tsdb, lonely).unwrap(),
        label(&tsdb, HOST, host).unwrap(),
    );
    let (now, retention) = (2 * DAY_MS + HOUR_MS, policy(keep(), keep(), age(24)));
    until_recycling(&mut tsdb, now, &retention);
    // A round of one: it forgets the row's host, which leaves it a candidate too.
    run(&mut tsdb, now, &retention, 1).unwrap();
    assert_eq!(
        candidates(&tsdb),
        [Entity::Combo(id), Entity::Label(HOST, host_id)]
    );
    // A reopen clears the backlog, so the next commit runs no round before its batch.
    drop(tsdb.into_storage());
    let mut tsdb = open_with(&disk, profile);
    let again = [(now, closed(None, Some(host), 2))];
    commit(&mut tsdb, 4, now, &[point(HOUR, now, lonely)], &again, &[]);
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(
        (
            done.candidates_checked,
            done.combos_released,
            done.labels_released
        ),
        (2, 0, 0)
    );
    assert_eq!(combo(&tsdb, lonely), Some(id));
    assert_eq!(label(&tsdb, HOST, host), Some(host_id));
    assert_eq!(candidates(&tsdb), []);
}

// A candidate naming an id that is not live, which only damage leaves, is dropped as stale:
// nothing names it, so nothing is released.
#[test]
fn a_candidate_of_an_id_not_live_is_dropped() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    commit(&mut tsdb, 1, 0, &[point(HOUR, 0, LABELS)], &[], &[]);
    let catalog = tsdb.current().catalog.clone();
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    for entity in [Entity::Combo(9), Entity::Label(HOST, 9)] {
        references::queue(&mut batch, entity).unwrap();
    }
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let done = finish(&mut tsdb, 0, &policy(keep(), keep(), keep()));
    assert_eq!(
        (
            done.candidates_checked,
            done.combos_released,
            done.labels_released
        ),
        (2, 0, 0)
    );
    assert_eq!(candidates(&tsdb), []);
    assert_eq!(tsdb.current().catalog.live_combos(), catalog.live_combos());
}

/// Staging fits any single release but not all table copies, so refusals split the work
/// into rounds.
#[test]
fn recycling_makes_progress_when_staging_fits_one_release_but_not_the_round() {
    /// Enough values for the loaded hash table to span several pages.
    const VALUES: u32 = 800;
    let disk = MemoryDisk::new();
    let storage = open(&disk).into_storage();
    let catalog = crate::catalog::Catalog::new(&schema());
    let account = crate::budget::test_account();
    let mut draft = catalog.draft(u64::MAX, &account).unwrap();
    let mut batch = storage.batch().unwrap();
    for n in 1..=VALUES {
        let id = draft
            .resolve_label(&mut batch, HOST, &n.to_be_bytes())
            .unwrap();
        assert_eq!(id, n);
        references::queue(&mut batch, Entity::Label(HOST, id)).unwrap();
    }
    let (_, copies) = draft.finish();
    batch.commit(Durability::Durable).unwrap();
    drop(copies);
    drop(storage);
    let profile = Profile {
        cleanup_chunks: VALUES as usize,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let current = tsdb.current();
    // Measure every fresh release, since the hash table's last page can be shorter.
    let mut single = 0;
    for id in 1..=VALUES {
        let mut draft = current.catalog.draft(profile.catalog, &account).unwrap();
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        draft.release_label(&mut batch, HOST, id).unwrap();
        single = single.max(draft.peak() - current.catalog.bytes());
    }
    let mut draft = current.catalog.draft(profile.catalog, &account).unwrap();
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    for id in 1..=VALUES {
        draft.release_label(&mut batch, HOST, id).unwrap();
    }
    assert!(draft.peak() - current.catalog.bytes() > single);
    drop(draft);
    drop(batch);
    drop(batch_storage);
    drop(current);
    let budget = tsdb.staging_budget().clone();
    let occupied = budget
        .try_reserve("other staging work", profile.staging - single)
        .unwrap();
    let retention = policy(keep(), keep(), keep());
    let (mut released, mut rounds) = (0, 0);
    for _ in 0..VALUES {
        let done = run(&mut tsdb, 0, &retention, 1).unwrap();
        assert_eq!(done.recycles, 1);
        assert!(done.labels_released > 0, "{done:?}");
        assert_eq!(done.combos_released, 0);
        released += done.labels_released;
        rounds += 1;
        assert_eq!(budget.reserved(), occupied.bytes());
        assert_eq!(candidates(&tsdb).len() as u64, u64::from(VALUES) - released);
        if !done.more {
            break;
        }
    }
    assert_eq!(released, u64::from(VALUES));
    assert!(rounds > 1);
    assert!(tsdb.current().catalog.live_labels(HOST).is_empty());
    drop(occupied);
    check_settled(&tsdb);
}
