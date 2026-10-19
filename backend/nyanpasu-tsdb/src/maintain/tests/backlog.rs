//! The backlog rule (§4.3, "积压"): while maintenance is behind, a commit runs the round that
//! `maintain` would run next before its batch, until it finds none due. Against a twin store
//! maintained a round per call, under failures of the round, on mobile, across a reopen, and
//! against a model of the writes.

use super::*;
use crate::{
    db::tests::MINUTE_MS,
    layout::{CONSOLIDATION, MANIFEST, META},
    storage::{
        ChunkId, StorageError,
        memory::{MemoryBatch, MemorySnapshot},
    },
    write::{AuxOp, PinOp, StageError, WriteError, stage::Counted},
};

/// Keeps everything: rounds of consolidation alone.
fn keep() -> RetentionPolicy {
    policy(
        Retention::default(),
        Retention::default(),
        Retention::default(),
    )
}

/// The round `maintain` would run next at `now`, under `keep`.
fn due<S: Storage>(tsdb: &Tsdb<S>, now: i64) -> Option<Round> {
    let request = Maintain {
        now: Timestamp(now),
        retention: &keep(),
        rounds: 1,
    };
    let snapshot = tsdb.storage().snapshot().unwrap();
    plan(&snapshot, &tsdb.round_context(&tsdb.current()), &request).unwrap()
}

/// The rounds `done` counts.
fn rounds(done: &Maintained) -> u64 {
    done.expired + done.cleanups + done.consolidations + done.merges
}

/// What a consolidation round did that took `rewritten` chunks and built `built`.
fn consolidated(rewritten: u64, built: u64) -> Maintained {
    Maintained {
        consolidations: 1,
        chunks_rewritten: rewritten,
        chunks_built: built,
        ..Maintained::default()
    }
}

/// What a merge round did that took `rewritten` chunks and built `built`.
fn merged(rewritten: u64, built: u64) -> Maintained {
    Maintained {
        merges: 1,
        chunks_rewritten: rewritten,
        chunks_built: built,
        ..Maintained::default()
    }
}

/// Every entry but the metadata, whose state record counts publications and batches.
fn without_meta(entries: Vec<(u8, Vec<u8>, Vec<u8>)>) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    entries
        .into_iter()
        .filter(|(ns, ..)| *ns != META.0)
        .collect()
}

/// The entries of the manifest and of consolidation's bookkeeping: what a round changes, and a
/// batch whose rows stay in the journal does not.
fn chunks_and_books(entries: Vec<(u8, Vec<u8>, Vec<u8>)>) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    entries
        .into_iter()
        .filter(|(ns, ..)| [MANIFEST.0, CONSOLIDATION.0].contains(ns))
        .collect()
}

/// The last batch the state record names.
fn last_batch<S: Storage>(tsdb: &Tsdb<S>) -> Option<u64> {
    read_state(&tsdb.storage().snapshot().unwrap())
        .unwrap()
        .last_batch
}

/// What the queries read: both tiers' totals and the uploads in key order.
fn visible<S: Storage>(tsdb: &Tsdb<S>) -> (u64, u64, Vec<u64>) {
    (
        total(tsdb, MINUTE, all()).values,
        total(tsdb, HOUR, all()).values,
        uploads(tsdb, all(), Direction::Forward).values,
    )
}

/// The minute extent of `day`'s second hour, where [`write_day`] writes.
fn minute_extent(day: i64) -> i64 {
    24 * day + 1
}

/// Writes a backlog of six rounds, all due at the time it returns with the next batch id:
///
/// - days 0 to 2 as [`write_day`] writes them, consolidated by a call that finishes;
/// - five late points into day 0's second hour, which crowd its minute and hour buckets;
/// - day 3 alike, then a flush an hour later, when the minute extents of days 2 and 3, day 2's
///   hour extent and day 2's window are due.
fn backlog<S: Storage>(tsdb: &mut Tsdb<S>) -> (u64, i64) {
    for day in 0..3 {
        write_day(tsdb, day);
    }
    let now = 2 * DAY_MS + 2 * HOUR_MS;
    finish(tsdb, now, &keep());
    for batch in 7..12 {
        write(tsdb, batch, now, &[HOUR_MS], &[], 1);
    }
    let day = 3 * DAY_MS;
    flush(tsdb, 12, day + 2 * HOUR_MS);
    write(tsdb, 13, day + 2 * HOUR_MS, &[day + HOUR_MS], &[day + 5], 8);
    let now = day + 3 * HOUR_MS;
    flush(tsdb, 14, now);
    (15, now)
}

/// What [`backlog`] leaves to do, in the order `maintain` does it.
fn backlog_rounds() -> [Maintained; 6] {
    [
        consolidated(1, 1),
        consolidated(1, 1),
        consolidated(1, 1),
        merged(5, 1),
        merged(5, 1),
        consolidated(1, 1),
    ]
}

// Not behind, a commit stages its batch alone, though a round is due: it publishes one version.
// A call with no budget for the round puts maintenance behind, and the next commit runs that
// round before its batch, which lands as well.
#[test]
fn a_commit_runs_a_round_only_while_maintenance_is_behind() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..3 {
        write_day(&mut tsdb, day);
    }
    let done = finish(&mut tsdb, 2 * DAY_MS + 2 * HOUR_MS, &keep());
    assert!(!done.more);
    assert!(!tsdb.behind());
    // An hour on, day 2's minute extent is quiet.
    let now = 2 * DAY_MS + 3 * HOUR_MS;
    let next = Round::Consolidate {
        tier: MINUTE,
        extent: minute_extent(2),
    };
    assert_eq!(due(&tsdb, now), Some(next));
    let publication = tsdb.current().publication;
    let written = write(&mut tsdb, 7, now, &[now], &[now], 8);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert_eq!(tsdb.current().publication, publication + 1);
    assert_eq!(due(&tsdb, now), Some(next));
    assert_eq!(visible(&tsdb), (worth(0..4), worth(0..4), vec![1, 2, 4, 8]));

    let done = run(&mut tsdb, now, &keep(), 0).unwrap();
    assert_eq!(
        done,
        Maintained {
            more: true,
            ..Maintained::default()
        }
    );
    assert!(tsdb.behind());
    // An invalid batch runs nothing: it is refused before the round.
    let point = SeriesPoint {
        tier: TierId(9),
        at: Timestamp(now),
        labels: LABELS,
        fields: &[1, 0],
    };
    let invalid = Input {
        batch_id: 8,
        now: Timestamp(now),
        series: &[point],
        events: &[],
        aux: &[],
        pins: &[],
    };
    let error = tsdb.write(&invalid, Durability::Durable).unwrap_err();
    assert!(
        matches!(error, WriteError::InvalidInput { .. }),
        "{error:?}"
    );
    assert_eq!(tsdb.current().publication, publication + 1);
    assert!(tsdb.behind());

    let written = write(&mut tsdb, 8, now, &[now + 1], &[now + 1], 16);
    assert_eq!(written.maintained, consolidated(1, 1));
    assert_eq!(written.publication, publication + 3);
    assert_eq!(
        manifest::read_stream(&tsdb.storage().snapshot().unwrap(), MINUTES)
            .unwrap()
            .consolidated,
        Some(minute_extent(2))
    );
    assert_eq!(due(&tsdb, now), None);
    assert!(tsdb.behind());
    assert_eq!(
        visible(&tsdb),
        (worth(0..5), worth(0..5), vec![1, 2, 4, 8, 16])
    );
    check_accounting(&tsdb);
    check_settled(&tsdb);
}

// While behind, each commit runs one round, the one `maintain` runs next: a twin store that a
// call of one round maintains instead takes the same rounds and stores the same chunks and
// bookkeeping after each. Once none is due, a commit ends the backlog, and the next runs none.
#[test]
fn each_commit_while_behind_runs_the_round_maintain_runs_next() {
    let (disk, twin_disk) = (MemoryDisk::new(), MemoryDisk::new());
    let (mut tsdb, mut twin) = (open(&disk), open(&twin_disk));
    let (mut batch, now) = backlog(&mut tsdb);
    backlog(&mut twin);
    assert_eq!(dump_storage(&tsdb), dump_storage(&twin));
    let expected = visible(&tsdb);
    let mut taken = Vec::new();
    for store in [&mut tsdb, &mut twin] {
        let done = run(store, now, &keep(), 1).unwrap();
        assert_eq!(
            done,
            Maintained {
                more: true,
                ..backlog_rounds()[0]
            }
        );
        assert!(store.behind());
    }
    taken.push(backlog_rounds()[0]);

    let bound = 2 * work(&tsdb) + 1;
    for _ in 0..bound {
        let publication = tsdb.current().publication;
        let written = write(&mut tsdb, batch, now, &[], &[], 0);
        batch += 1;
        let done = run(&mut twin, now, &keep(), 1).unwrap();
        assert_eq!(
            written.maintained,
            Maintained {
                more: false,
                ..done
            }
        );
        assert_eq!(
            without_meta(dump_storage(&tsdb)),
            without_meta(dump_storage(&twin))
        );
        assert_eq!(visible(&tsdb), expected);
        check_accounting(&tsdb);
        check_settled(&tsdb);
        let ran = rounds(&written.maintained);
        assert_eq!(tsdb.current().publication, publication + 1 + ran);
        if ran == 0 {
            break;
        }
        taken.push(written.maintained);
        assert!(tsdb.behind());
    }
    assert_eq!(taken, backlog_rounds());
    assert!(!tsdb.behind());
    let publication = tsdb.current().publication;
    let written = write(&mut tsdb, batch, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert_eq!(
        run(&mut tsdb, now, &keep(), 0).unwrap(),
        Maintained::default()
    );
}

// A commit has no policy, so it never expires or cleans up: with only retention's work left,
// a commit runs nothing and ends the backlog, and the consumer's next call does the work.
#[test]
fn a_commit_never_runs_retention() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..4 {
        write_day(&mut tsdb, day);
    }
    let now = 3 * DAY_MS + 2 * HOUR_MS;
    finish(&mut tsdb, now, &keep());
    let retention = policy(Retention::default(), age(24), age(24));
    let done = run(&mut tsdb, now, &retention, 1).unwrap();
    assert_eq!((done.expired, done.more), (1, true));
    assert!(tsdb.behind());
    let snapshot = tsdb.storage().snapshot().unwrap();
    let request = Maintain {
        now: Timestamp(now),
        retention: &retention,
        rounds: 1,
    };
    let next = plan(&snapshot, &tsdb.round_context(&tsdb.current()), &request).unwrap();
    assert!(
        matches!(next, Some(Round::Expire { .. } | Round::Cleanup { .. })),
        "{next:?}"
    );
    drop(snapshot);

    let (storage, publication) = (dump_storage(&tsdb), tsdb.current().publication);
    let written = write(&mut tsdb, 9, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert!(!tsdb.behind());
    assert_eq!(without_meta(dump_storage(&tsdb)), without_meta(storage));

    let done = finish(&mut tsdb, now, &retention);
    assert_eq!((done.expired, done.consolidations, done.merges), (3, 0, 0));
    assert_eq!(watermark(&tsdb, HOURS), Some(1));
    assert_eq!(watermark(&tsdb, EVENTS), Some(1));
}

// A commit's round is dated by the batch's clock and committed with the durability due then,
// as a call's rounds are: within the profile's interval after a durable commit it is only
// consistent, and a crash loses it with a consistent batch.
#[test]
fn a_commits_round_takes_the_durability_due_at_its_clock() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        durable_interval: Some(Duration::from_secs(3600)),
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    // Its last batch is durable, at `now`.
    let (batch, now) = backlog(&mut tsdb);
    let storage = dump_storage(&tsdb);
    assert!(run(&mut tsdb, now, &keep(), 0).unwrap().more);
    assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Consistent);
    let points = [point_at(now)];
    let input = Input {
        series: &points,
        ..empty(batch, now)
    };
    let written = tsdb.write(&input, Durability::Consistent).unwrap();
    assert_eq!(written.maintained, consolidated(1, 1));
    assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Consistent);

    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, profile);
    // Opening commits a new incarnation: compare everything but the metadata.
    assert_eq!(without_meta(dump_storage(&tsdb)), without_meta(storage));
}

/// Counts the next round's writes without committing or publishing its draft.
fn next_round_bytes(tsdb: &Tsdb<MemoryStorage>, now: i64) -> u64 {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let current = tsdb.current();
    let batch_storage = tsdb.storage();
    let mut batch = Counted::new(batch_storage.batch().unwrap());
    let staged = stage(
        &mut batch,
        &snapshot,
        &tsdb.round_context(&current),
        due(tsdb, now).unwrap(),
        Timestamp(now),
    )
    .unwrap();
    let bytes = batch.written();
    drop(staged);
    bytes
}

/// The round uses the decision made before its bytes cross the limit. The caller's earlier
/// Consistent choice still applies to the batch; only the next decision must be Durable.
#[test]
fn a_backlog_round_crosses_the_durable_byte_threshold() {
    let calibration = MemoryDisk::new();
    let mut measured = open(&calibration);
    let (_, now) = backlog(&mut measured);
    let bytes = next_round_bytes(&measured, now);
    assert!(bytes > 0);

    for threshold in [bytes, bytes + 1] {
        for checkpoint in [false, true] {
            let profile = Profile {
                durable_interval: Some(Duration::from_secs(3600)),
                durable_after_bytes: threshold,
                ..Profile::desktop()
            };
            let (disk, twin_disk) = (MemoryDisk::new(), MemoryDisk::new());
            let mut tsdb = open_with(&disk, profile.clone());
            let mut twin = open_with(&twin_disk, profile.clone());
            let (batch, now) = backlog(&mut tsdb);
            backlog(&mut twin);
            let before = dump_storage(&tsdb);
            let before_visible = visible(&tsdb);
            let publication = tsdb.current().publication;
            let durability = tsdb.durability_due(Timestamp(now));
            assert_eq!(durability, Durability::Consistent);
            assert!(run(&mut tsdb, now, &keep(), 0).unwrap().more);

            // Isolate the round's contribution on either side of the exact byte limit.
            let done = run(&mut twin, now, &keep(), 1).unwrap();
            assert_eq!(
                done,
                Maintained {
                    more: true,
                    ..consolidated(1, 1)
                }
            );
            assert_eq!(
                twin.durability_due(Timestamp(now)),
                if threshold == bytes {
                    Durability::Durable
                } else {
                    Durability::Consistent
                }
            );
            let points = [point_at(now)];
            let input = Input {
                series: &points,
                ..empty(batch, now)
            };
            let written = tsdb.write(&input, durability).unwrap();
            assert_eq!(written.maintained, consolidated(1, 1));
            assert_eq!(written.publication, publication + 2);
            assert_eq!(last_batch(&tsdb), Some(batch));
            assert_eq!(
                chunks_and_books(dump_storage(&tsdb)),
                chunks_and_books(dump_storage(&twin))
            );
            let after_visible = visible(&tsdb);
            assert_eq!(
                after_visible,
                (
                    before_visible.0 + points[0].fields[0],
                    before_visible.1,
                    before_visible.2.clone()
                )
            );
            assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Durable);
            check_accounting(&tsdb);
            check_settled(&tsdb);

            if checkpoint {
                let due = tsdb.durability_due(Timestamp(now));
                tsdb.write(&empty(batch + 1, now), due).unwrap();
                assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Consistent);
            }
            let expected = if checkpoint {
                dump_storage(&tsdb)
            } else {
                before
            };
            let expected_publication = if checkpoint {
                tsdb.current().publication
            } else {
                publication
            };
            tsdb.into_storage().crash();
            let reopened = open_with(&disk, profile);
            assert_eq!(reopened.current().publication, expected_publication);
            assert_eq!(
                last_batch(&reopened),
                Some(if checkpoint { batch + 1 } else { batch - 1 })
            );
            assert_eq!(
                without_meta(dump_storage(&reopened)),
                without_meta(expected)
            );
            assert_eq!(
                visible(&reopened),
                if checkpoint {
                    after_visible
                } else {
                    before_visible
                }
            );
        }
    }
}

/// Arms a rollback when the batch after one successful round begins. The underlying memory
/// backend implements the rollback, so the round itself uses the normal commit path.
struct RejectBatch {
    /// The backend whose second transaction can be rolled back.
    storage: MemoryStorage,
    /// Successful batch starts to allow before arming NotApplied; None leaves it unarmed.
    batches_left: std::sync::Mutex<Option<usize>>,
}

impl Storage for RejectBatch {
    type Snapshot = MemorySnapshot;
    type Batch<'s> = MemoryBatch<'s>;

    fn snapshot(&self) -> Result<Self::Snapshot, StorageError> {
        self.storage.snapshot()
    }

    fn batch(&self) -> Result<Self::Batch<'_>, StorageError> {
        let mut left = self.batches_left.lock().unwrap();
        if let Some(count) = *left {
            if count == 0 {
                self.storage.inject_commit(CommitOutcome::NotApplied);
                *left = None;
            } else {
                *left = Some(count - 1);
            }
        }
        self.storage.batch()
    }
}

/// A local staging rejection and a NotApplied batch leave the preceding round published,
/// including its bookkeeping and durability bytes, with no batch, auxiliary or pin changes.
#[test]
fn a_successful_backlog_round_survives_a_rejected_batch() {
    for not_applied in [false, true] {
        let (disk, twin_disk) = (MemoryDisk::new(), MemoryDisk::new());
        let profile = Profile {
            durable_interval: Some(Duration::from_secs(3600)),
            durable_after_bytes: 1,
            ..Profile::desktop()
        };
        let storage = RejectBatch {
            storage: disk.open().unwrap(),
            batches_left: std::sync::Mutex::new(None),
        };
        let mut tsdb = Tsdb::open(storage, crate::db::tests::schema(), profile.clone()).unwrap();
        let mut twin = open_with(&twin_disk, profile);
        let (batch, now) = backlog(&mut tsdb);
        backlog(&mut twin);
        assert!(run(&mut tsdb, now, &keep(), 0).unwrap().more);
        let before = dump_storage(&tsdb);
        let expected_visible = visible(&tsdb);
        let publication = tsdb.current().publication;
        assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Consistent);
        let points = [point_at(now)];
        let aux = [AuxOp::Put {
            key: b"active",
            value: b"conn",
        }];
        let pins = [
            PinOp::Pin {
                key: b"conn",
                labels: LABELS,
            },
            PinOp::Unpin { key: b"missing" },
        ];
        let input = Input {
            series: &points,
            aux: &aux,
            pins: if not_applied { &pins[..1] } else { &pins },
            ..empty(batch, now)
        };
        if not_applied {
            *tsdb.storage().batches_left.lock().unwrap() = Some(1);
        }
        let error = tsdb.write(&input, Durability::Consistent).unwrap_err();
        if not_applied {
            assert!(
                matches!(
                    error,
                    WriteError::Commit {
                        source: crate::storage::CommitFailure::NotApplied { .. }
                    }
                ),
                "{error:?}"
            );
            assert_eq!(*tsdb.storage().batches_left.lock().unwrap(), None);
        } else {
            assert!(
                matches!(
                    error,
                    WriteError::Stage {
                        source: StageError::NotPinned { pin: 1 }
                    }
                ),
                "{error:?}"
            );
        }
        assert_eq!(error.kind(), ErrorKind::Local);
        assert!(error.not_applied());
        let done = run(&mut twin, now, &keep(), 1).unwrap();
        assert_eq!(
            done,
            Maintained {
                more: true,
                ..consolidated(1, 1)
            }
        );
        assert_ne!(
            chunks_and_books(dump_storage(&tsdb)),
            chunks_and_books(before)
        );
        assert_eq!(dump_storage(&tsdb), dump_storage(&twin));
        assert_eq!(tsdb.current().publication, publication + 1);
        assert_eq!(last_batch(&tsdb), Some(batch - 1));
        assert_eq!(visible(&tsdb), expected_visible);
        assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Durable);
        assert!(!tsdb.registry().needs_recovery());
        assert!(tsdb.behind());
        check_accounting(&tsdb);
        check_settled(&tsdb);
        // The rejected id remains usable without recovery; the next round and batch commit.
        let written = tsdb.write(&empty(batch, now), Durability::Durable).unwrap();
        assert_eq!(written.publication, publication + 3);
        assert_eq!(written.maintained, consolidated(1, 1));
        assert_eq!(last_batch(&tsdb), Some(batch));
        assert_eq!(visible(&tsdb), expected_visible);
        check_accounting(&tsdb);
        check_settled(&tsdb);
    }
}

/// A store left behind with [`backlog`]'s first round done, its next batch id and the time.
fn behind(disk: &MemoryDisk) -> (Tsdb<MemoryStorage>, u64, i64) {
    let mut tsdb = open(disk);
    let (batch, now) = backlog(&mut tsdb);
    let done = run(&mut tsdb, now, &keep(), 1).unwrap();
    assert!(done.more);
    assert_eq!(
        due(&tsdb, now),
        Some(Round::Consolidate {
            tier: MINUTE,
            extent: minute_extent(3)
        })
    );
    (tsdb, batch, now)
}

/// The chunk of day 3's minute extent, which the next round of [`behind`] consolidates.
fn next_input<S: Storage>(tsdb: &Tsdb<S>) -> ChunkId {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let listed = manifest::chunks(&snapshot, MINUTES, minute_extent(3)).unwrap();
    assert_eq!(listed.len(), 1);
    listed[0].chunk
}

/// Writes batch `batch_id` at `now`, a point of both tiers and a `closed` row worth 32 at
/// `now`, which stay in the journal, into a store left [`behind`] whose round fails without
/// failing the backend. Checks that the batch landed and the round left no trace: no chunk, no
/// bookkeeping, nothing reserved, and maintenance no longer behind. It reads the hour tier and
/// `closed`, which the round's input, a minute chunk, may have damaged for queries.
fn write_past_a_failing_round(tsdb: &mut Tsdb<MemoryStorage>, batch_id: u64, now: i64) {
    let (storage, publication) = (dump_storage(tsdb), tsdb.current().publication);
    let read = |tsdb: &Tsdb<MemoryStorage>| {
        (
            total(tsdb, HOUR, all()).values,
            uploads(tsdb, all(), Direction::Forward).values,
        )
    };
    let (hours, mut rows) = read(tsdb);
    let written = write(tsdb, batch_id, now, &[now], &[now], 32);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert_eq!(last_batch(tsdb), Some(batch_id));
    assert!(!tsdb.behind());
    rows.push(32);
    assert_eq!(read(tsdb), (hours + 32, rows));
    assert_eq!(
        chunks_and_books(dump_storage(tsdb)),
        chunks_and_books(storage)
    );
    check_accounting(tsdb);
    check_settled(tsdb);
}

// A round that fails without failing the backend leaves no trace, and the batch commits all
// the same: the commit ends the backlog, the next commit runs nothing, and the consumer's next
// call meets the failure, or does the round.
#[test]
fn a_round_failing_locally_or_on_damage_leaves_the_batch_to_commit() {
    // Damage in the round's input.
    let disk = MemoryDisk::new();
    let (mut tsdb, batch, now) = behind(&disk);
    let chunk = next_input(&tsdb);
    tsdb.storage()
        .corrupt_fragment(FragmentId { chunk, index: 0 });
    write_past_a_failing_round(&mut tsdb, batch, now);
    let publication = tsdb.current().publication;
    let written = write(&mut tsdb, batch + 1, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    let error = run(&mut tsdb, now, &keep(), 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Consolidate {
                tier: MINUTE,
                extent,
                ..
            } if extent == minute_extent(3)
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);

    // A round whose commit is not applied.
    let disk = MemoryDisk::new();
    let (mut tsdb, batch, now) = behind(&disk);
    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    write_past_a_failing_round(&mut tsdb, batch, now);
    let written = write(&mut tsdb, batch + 1, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    let done = run(&mut tsdb, now, &keep(), 1).unwrap();
    assert_eq!(
        done,
        Maintained {
            more: true,
            ..consolidated(1, 1)
        }
    );
}

// A round that fails the backend fails the write: the batch is not applied, and the store needs
// recovery. On an unknown outcome the round may have applied; the batch never did.
#[test]
fn a_round_failing_the_backend_fails_the_write() {
    // A storage write in the round fails.
    let disk = MemoryDisk::new();
    let (mut tsdb, batch, now) = behind(&disk);
    let (version, storage) = (tsdb.current(), dump_storage(&tsdb));
    let points = [point_at(now)];
    let input = Input {
        series: &points,
        ..empty(batch, now)
    };
    tsdb.storage().fail_next(OpClass::Write);
    let error = tsdb.write(&input, Durability::Durable).unwrap_err();
    assert!(
        matches!(
            error,
            WriteError::Backlog {
                source: MaintainError::Consolidate { tier: MINUTE, .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(error.not_applied());
    assert!(Arc::ptr_eq(&version, &tsdb.current()));
    assert!(tsdb.registry().needs_recovery());
    check_settled(&tsdb);
    assert!(matches!(
        tsdb.write(&input, Durability::Durable),
        Err(WriteError::RecoveryNeeded)
    ));
    drop(tsdb.into_storage());
    assert_eq!(dump(&disk.open().unwrap().snapshot().unwrap()), storage);

    // The round's commit has an unknown outcome. Applied, the store holds what a twin holds
    // after one more round of `maintain`; not applied, what it held before.
    for applied in [false, true] {
        let disk = MemoryDisk::new();
        let (mut tsdb, batch, now) = behind(&disk);
        let version = tsdb.current();
        let twin_disk = MemoryDisk::new();
        let (mut twin, ..) = behind(&twin_disk);
        if applied {
            run(&mut twin, now, &keep(), 1).unwrap();
        }
        let expected = dump_storage(&twin);
        let points = [point_at(now)];
        let input = Input {
            series: &points,
            ..empty(batch, now)
        };
        tsdb.storage()
            .inject_commit(CommitOutcome::Unknown { applied });
        let error = tsdb.write(&input, Durability::Durable).unwrap_err();
        assert!(
            matches!(
                error,
                WriteError::Backlog {
                    source: MaintainError::Commit {
                        source: CommitFailure::Unknown { .. }
                    }
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        assert!(error.not_applied());
        assert!(Arc::ptr_eq(&version, &tsdb.current()));
        assert!(tsdb.registry().needs_recovery());
        check_settled(&tsdb);
        drop(tsdb.into_storage());
        let stored = dump(&disk.open().unwrap().snapshot().unwrap());
        assert_eq!(stored, expected, "applied: {applied}");
    }
}

/// A point of the minute tier at `now`.
fn point_at(now: i64) -> SeriesPoint<'static> {
    SeriesPoint {
        tier: MINUTE,
        at: Timestamp(now),
        labels: LABELS,
        fields: &[64, 0],
    }
}

/// An empty batch `batch_id` at `now`.
fn empty(batch_id: u64, now: i64) -> Input<'static> {
    Input {
        batch_id,
        now: Timestamp(now),
        series: &[],
        events: &[],
        aux: &[],
        pins: &[],
    }
}

// A call that fails leaves the backlog as it was, whether maintenance was behind or not: an
// invalid policy, or a round whose commit is not applied. A call that finishes ends it.
#[test]
fn a_failing_call_leaves_the_backlog_as_it_was() {
    let disk = MemoryDisk::new();
    let (tsdb, _, now) = behind(&disk);
    // Not behind after a reopen, with rounds due.
    let mut tsdb = reopen(tsdb, &disk);
    for behind in [false, true] {
        assert_eq!(tsdb.behind(), behind);
        let error = run(&mut tsdb, now, &RetentionPolicy::default(), 1).unwrap_err();
        assert!(matches!(error, MaintainError::PolicyShape { .. }));
        assert_eq!(tsdb.behind(), behind);
        tsdb.storage().inject_commit(CommitOutcome::NotApplied);
        let error = run(&mut tsdb, now, &keep(), 1).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Local);
        assert_eq!(tsdb.behind(), behind);
        // A call of one round leaves the rest, and maintenance behind.
        assert!(run(&mut tsdb, now, &keep(), 1).unwrap().more);
    }
    finish(&mut tsdb, now, &keep());
    assert!(!tsdb.behind());
}

// On mobile, which skips the rounds of `closed`, a commit while behind never runs one: once the
// tiers are done, it ends the backlog with `closed`'s window due on desktop.
#[test]
fn on_mobile_a_commit_skips_the_event_rounds() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::mobile());
    write_day(&mut tsdb, 0);
    let now = DAY_MS + 2 * HOUR_MS;
    flush(&mut tsdb, 3, now);
    let done = run(&mut tsdb, now, &keep(), 0).unwrap();
    assert!(done.more);
    let closed = chunk_count(&tsdb, EVENTS);
    let written = write(&mut tsdb, 4, now, &[], &[], 0);
    assert_eq!(written.maintained, consolidated(1, 1));
    let written = write(&mut tsdb, 5, now, &[], &[], 0);
    assert_eq!(written.maintained, consolidated(1, 1));
    assert!(tsdb.behind());
    assert_eq!(due(&tsdb, now), None);
    let desktop = Profile::desktop();
    let current = tsdb.current();
    let context = Context {
        profile: &desktop,
        ..tsdb.round_context(&current)
    };
    let request = Maintain {
        now: Timestamp(now),
        retention: &keep(),
        rounds: 1,
    };
    let snapshot = tsdb.storage().snapshot().unwrap();
    assert_eq!(
        plan(&snapshot, &context, &request).unwrap(),
        Some(Round::ConsolidateEvents {
            stream: CLOSED,
            extent: 0
        })
    );
    drop(snapshot);

    let publication = tsdb.current().publication;
    let written = write(&mut tsdb, 6, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert!(!tsdb.behind());
    assert_eq!(chunk_count(&tsdb, EVENTS), closed);
}

// The backlog is held in memory: a reopen starts with none, and its commits run no round
// until a call says work remains.
#[test]
fn a_reopen_starts_with_no_backlog() {
    let disk = MemoryDisk::new();
    let (tsdb, batch, now) = behind(&disk);
    assert!(tsdb.behind());
    let mut tsdb = reopen(tsdb, &disk);
    assert!(!tsdb.behind());
    let publication = tsdb.current().publication;
    let written = write(&mut tsdb, batch, now, &[], &[], 0);
    assert_eq!(written.maintained, Maintained::default());
    assert_eq!(written.publication, publication + 1);
    assert!(due(&tsdb, now).is_some());
}

#[derive(Clone, Debug)]
enum Step {
    /// Moves the clock `advance` minutes on, then writes points (tier, minutes back, label
    /// set, value) and `closed` rows (minutes back, upload). `None` minutes back is the first
    /// minute of the timeline, whose bucket and window late writes then crowd.
    Write {
        advance: i64,
        points: Vec<(TierId, Option<i64>, usize, u64)>,
        rows: Vec<(Option<i64>, u64)>,
    },
    /// Maintains at the clock with a budget of `rounds`.
    Maintain { rounds: usize },
}

fn step() -> impl Strategy<Value = Step> {
    let tier = prop_oneof![Just(MINUTE), Just(HOUR)];
    let write = (
        0..240i64,
        prop::collection::vec(
            (
                tier,
                prop::option::weighted(0.6, 0..6 * 60i64),
                0..SETS.len(),
                1..1_000u64,
            ),
            0..4,
        ),
        prop::collection::vec((prop::option::weighted(0.7, 0..120i64), 1..1_000u64), 0..3),
    )
        .prop_map(|(advance, points, rows)| Step::Write {
            advance,
            points,
            rows,
        });
    let maintain = (0..=2usize).prop_map(|rounds| Step::Maintain { rounds });
    prop_oneof![3 => write, 2 => maintain]
}

/// The time `back` minutes before `now`, or in the first minute of the timeline.
fn back_from(now: i64, back: Option<i64>) -> i64 {
    back.map_or(5, |back| now - back * MINUTE_MS)
}

/// Writes `points` and `rows` as batch `batch_id` at `now`, as [`Step::Write`] gives them, into
/// `tsdb` and `model`; checks that the commit ran the round `maintain` would run next while
/// maintenance was behind, and none otherwise, and published a version for it and one for the
/// batch.
fn write_step(
    tsdb: &mut Tsdb<MemoryStorage>,
    model: &mut Model,
    batch_id: u64,
    now: i64,
    points: &[(TierId, Option<i64>, usize, u64)],
    rows: &[(Option<i64>, u64)],
) -> Result<(), TestCaseError> {
    let next = if tsdb.behind() { due(tsdb, now) } else { None };
    let (behind, publication) = (tsdb.behind(), tsdb.current().publication);
    let grids = tsdb.shape().grids.clone();
    let series: Vec<SeriesPoint<'_>> = points
        .iter()
        .map(|&(tier, back, set, value)| SeriesPoint {
            tier,
            at: Timestamp(back_from(now, back)),
            labels: &SETS[set],
            fields: Vec::leak(vec![value, 0]),
        })
        .collect();
    for (point, &(.., set, value)) in series.iter().zip(points) {
        let bucket = grids[usize::from(point.tier.0)].bucket().index_of(point.at);
        *model.series.entry((point.tier, bucket, set)).or_default() += value;
    }
    let values: Vec<_> = rows
        .iter()
        .map(|&(_, upload)| {
            let mut values = closed_values(Value::Series(&SETS[0]));
            values[1] = Value::U64(upload);
            values
        })
        .collect();
    let events: Vec<EventRow<'_>> = rows
        .iter()
        .zip(&values)
        .map(|(&(back, _), values)| EventRow {
            stream: CLOSED,
            key: back_from(now, back),
            seq: tsdb.next_event_seq(),
            values,
        })
        .collect();
    for (event, &(_, upload)) in events.iter().zip(rows) {
        let order = (event.key, event.seq.incarnation, event.seq.seq);
        model.rows.insert(order, upload);
    }
    let input = Input {
        batch_id,
        now: Timestamp(now),
        series: &series,
        events: &events,
        aux: &[],
        pins: &[],
    };
    let written = tsdb.write(&input, Durability::Durable).unwrap();
    prop_assert_eq!(written.late, 0);
    let ran = rounds(&written.maintained);
    prop_assert_eq!(ran, u64::from(next.is_some()));
    match next {
        Some(Round::Consolidate { .. } | Round::ConsolidateEvents { .. }) => {
            prop_assert_eq!(written.maintained.consolidations, 1);
        }
        Some(Round::MergeDeltas { .. } | Round::MergeRuns { .. }) => {
            prop_assert_eq!(written.maintained.merges, 1);
        }
        Some(other) => prop_assert!(false, "a commit planned {:?}", other),
        None => prop_assert_eq!(written.maintained, Maintained::default()),
    }
    prop_assert_eq!(tsdb.behind(), behind && next.is_some());
    prop_assert_eq!(written.publication, publication + 1 + ran);
    Ok(())
}

/// Checks what every step leaves: the queries read the model, accounting is exact, and
/// nothing a round reserved stays reserved.
fn check_step(tsdb: &Tsdb<MemoryStorage>, model: &Model) -> Result<(), TestCaseError> {
    check_visible(tsdb, model, &[None; 3])?;
    check_accounting(tsdb);
    check_settled(tsdb);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // Random writes, late ones among them, and calls of up to two rounds that leave maintenance
    // behind or not. After every step the queries equal a model of the writes, accounting is
    // exact, and a commit ran the round a call would run next exactly while maintenance was
    // behind. At the end, commits alone catch up within a bound from what was written.
    #[test]
    fn commits_while_behind_keep_the_writes(
        consolidate_chunks in 2usize..5,
        max_sorted_runs in 1usize..3,
        event_buffer_rows in 1usize..4,
        steps in prop::collection::vec(step(), 1..40),
    ) {
        let disk = MemoryDisk::new();
        let profile = Profile {
            consolidate_chunks,
            max_sorted_runs,
            event_buffer_rows,
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile);
        let mut model = Model::default();
        let (mut now, mut batch_id) = (0, 0);
        for step in &steps {
            match step {
                Step::Write { advance, points, rows } => {
                    now += advance * MINUTE_MS;
                    batch_id += 1;
                    write_step(&mut tsdb, &mut model, batch_id, now, points, rows)?;
                }
                Step::Maintain { rounds: budget } => {
                    let publication = tsdb.current().publication;
                    let done = run(&mut tsdb, now, &keep(), *budget).unwrap();
                    prop_assert!(rounds(&done) <= *budget as u64);
                    prop_assert_eq!(tsdb.current().publication, publication + rounds(&done));
                    prop_assert_eq!(tsdb.behind(), done.more);
                }
            }
            check_step(&tsdb, &model)?;
        }

        // Two days on every extent written has ended and is quiet, once a commit chunks the
        // journal; a call with no budget puts maintenance behind if a round is due.
        now += 2 * DAY_MS;
        batch_id += 1;
        write_step(&mut tsdb, &mut model, batch_id, now, &[], &[])?;
        run(&mut tsdb, now, &keep(), 0).unwrap();
        // A round takes an L0 chunk or moves a cursor past an extent record, merges two late
        // chunks or runs or more into one, or clears a mark that a chunk or such a round set:
        // a few rounds per chunk or extent record bound them.
        let bound = 4 * work(&tsdb) + 1;
        for _ in 0..bound {
            if !tsdb.behind() {
                break;
            }
            batch_id += 1;
            write_step(&mut tsdb, &mut model, batch_id, now, &[], &[])?;
            check_step(&tsdb, &model)?;
        }
        prop_assert!(!tsdb.behind(), "commits did not catch up in {} rounds", bound);
        prop_assert_eq!(run(&mut tsdb, now, &keep(), 0).unwrap(), Maintained::default());
        check_step(&tsdb, &model)?;
    }
}
