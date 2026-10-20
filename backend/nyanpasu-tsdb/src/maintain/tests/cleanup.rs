//! Cleanup validates metadata before work, preserves failed rounds, and deletes fragments
//! by prefix on both engines. Empty extent records fail in both maintenance planners.

use super::*;
use crate::{
    chunk::format::{ChunkKind, MAX_CHUNK_LEN, MAX_FRAGMENTS, MAX_ROWS, MIN_FRAGMENT0_LEN},
    layout::{CONSOLIDATION, EXTENTS, MANIFEST, manifest::ChunkEntry, record::encode},
    storage::{ChunkId, MAX_FRAGMENT_LEN, StorageWrite},
};

/// A policy that expires only the stream under test.
fn retention(stream: StreamRef) -> RetentionPolicy {
    match stream {
        HOURS => policy(Retention::default(), age(24), Retention::default()),
        EVENTS => policy(Retention::default(), Retention::default(), age(24)),
        _ => panic!("unexpected test stream"),
    }
}

/// Writes real chunks of one stream and expires their extent, leaving every reference held
/// by those chunks alone. The profile chunks every batch's journal immediately.
fn expired<S: Storage>(
    tsdb: &mut Tsdb<S>,
    stream: StreamRef,
    chunks: usize,
) -> Vec<manifest::Listed> {
    for index in 0..chunks {
        let series = [SeriesPoint {
            tier: HOUR,
            at: Timestamp(HOUR_MS),
            labels: LABELS,
            fields: &[1, 0],
        }];
        let values = closed_values(Value::Series(LABELS));
        let events = [EventRow {
            stream: CLOSED,
            key: 1,
            seq: tsdb.next_event_seq(),
            values: &values,
        }];
        tsdb.write(
            &Input {
                batch_id: index as u64 + 1,
                now: Timestamp(2 * HOUR_MS),
                series: if stream == HOURS { &series } else { &[] },
                events: if stream == EVENTS { &events } else { &[] },
                aux: &[],
                pins: &[],
            },
            Durability::Durable,
        )
        .unwrap();
    }
    let listed = manifest::stream_chunks(&tsdb.storage().snapshot().unwrap(), stream).unwrap();
    assert_eq!(listed.len(), chunks);
    let done = run(tsdb, 2 * DAY_MS, &retention(stream), 1).unwrap();
    assert_eq!((done.expired, done.chunks_deleted), (1, 0));
    assert_eq!(watermark(tsdb, stream), Some(0));
    listed
}

/// Overwrites only the manifest entry, leaving the valid payload and bookkeeping intact.
fn overwrite<S: Storage>(
    tsdb: &Tsdb<S>,
    stream: StreamRef,
    listed: &manifest::Listed,
    entry: &ChunkEntry,
) {
    damage(tsdb, |batch| {
        let key = manifest::extent_key(stream, listed.extent).u64(listed.chunk.0);
        batch.put(MANIFEST, key.as_slice(), &encode(entry)).unwrap();
    });
}

/// Every field cleanup trusts is damaged separately. The full storage dump includes
/// references, uses, candidates and catalog records.
fn rejects_metadata<S: Storage>(tsdb: &mut Tsdb<S>, stream: StreamRef) {
    let listed = expired(tsdb, stream, 1)[0];
    let entry = listed.entry;
    let payload = tsdb.storage().snapshot().unwrap();
    let rows = match stream {
        HOURS => tsdb.round_context(&tsdb.current()).shape.series_chunk_rows as u32,
        _ => MAX_ROWS as u32,
    };
    let cases = [
        ChunkEntry { kind: 0, ..entry },
        ChunkEntry {
            kind: u8::MAX,
            ..entry
        },
        ChunkEntry { rows: 0, ..entry },
        ChunkEntry {
            rows: rows + 1,
            ..entry
        },
        ChunkEntry {
            fragments: 0,
            ..entry
        },
        ChunkEntry {
            fragments: 1,
            ..entry
        },
        ChunkEntry {
            fragments: MAX_FRAGMENTS as u32 + 1,
            ..entry
        },
        ChunkEntry {
            fragments: u32::MAX,
            ..entry
        },
        ChunkEntry {
            fragment0: 0,
            ..entry
        },
        ChunkEntry {
            fragment0: MIN_FRAGMENT0_LEN as u32 - 1,
            ..entry
        },
        ChunkEntry {
            fragment0: MAX_FRAGMENT_LEN as u32 + 1,
            ..entry
        },
        ChunkEntry {
            fragment0: u32::MAX,
            ..entry
        },
        ChunkEntry { bytes: 0, ..entry },
        ChunkEntry {
            bytes: u64::from(entry.fragment0) - 1,
            ..entry
        },
        ChunkEntry {
            bytes: u64::from(entry.fragment0),
            ..entry
        },
        ChunkEntry {
            bytes: MAX_CHUNK_LEN + 1,
            ..entry
        },
        ChunkEntry {
            bytes: u64::MAX,
            ..entry
        },
        ChunkEntry {
            key_range: (2, 1),
            ..entry
        },
        ChunkEntry {
            key_range: (i64::MIN, entry.key_range.1),
            ..entry
        },
        ChunkEntry {
            key_range: (entry.key_range.0, i64::MAX),
            ..entry
        },
    ];
    let spanning_l0 = (stream == HOURS).then_some(ChunkEntry {
        key_range: (1, 2),
        ..entry
    });
    for broken in cases.into_iter().chain(spanning_l0) {
        overwrite(tsdb, stream, &listed, &broken);
        let (before, storage, head) = (tsdb.current(), dump_storage(tsdb), tsdb.head_reserved());
        let error = run(tsdb, 2 * DAY_MS, &retention(stream), 1).unwrap_err();
        assert!(
            matches!(error, MaintainError::Cleanup {
                source: RetentionError::InvalidChunk { chunk }, ..
            } if chunk == listed.chunk.0),
            "{stream:?}: {broken:?}: {error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        assert!(Arc::ptr_eq(&before, &tsdb.current()));
        assert_eq!(dump_storage(tsdb), storage);
        assert_eq!(tsdb.head_reserved(), head);
        assert_eq!(tsdb.staging_reserved(), 0);
        assert!(tsdb.registry().pending().is_none());
        assert!(!tsdb.registry().needs_recovery());

        // Even an explicitly requested recycling round must keep these ids. No chunk was
        // cleaned, so the newest references and the catalog cannot change.
        let read = tsdb.storage().snapshot().unwrap();
        for index in 0..entry.fragments {
            let id = FragmentId {
                chunk: listed.chunk,
                index,
            };
            payload
                .fragment(id, MAX_FRAGMENT_LEN, |original| {
                    assert!(original.is_some());
                    read.fragment(id, MAX_FRAGMENT_LEN, |bytes| assert_eq!(bytes, original))
                        .unwrap();
                })
                .unwrap();
        }
        let context = tsdb.round_context(&before);
        assert!(recycle::plan(&read, &context).unwrap().is_none());
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        let error = recycle::recycle(&mut batch, &context).err().unwrap();
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        assert_eq!(dump(&batch), storage);
        assert_eq!(
            before.catalog.live_combos(),
            tsdb.current().catalog.live_combos()
        );
    }
    overwrite(tsdb, stream, &listed, &entry);
    let done = finish(tsdb, 2 * DAY_MS, &retention(stream));
    assert_eq!((done.chunks_deleted, done.combos_released), (1, 1));
    assert!(done.labels_released > 0);
}

/// Hostile metadata never deletes data or releases its ids; repairing it permits both.
#[test]
fn cleanup_rejects_invalid_metadata_without_changing_state() {
    for stream in [HOURS, EVENTS] {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(
            &disk,
            Profile {
                journal_bytes: 1,
                ..Profile::desktop()
            },
        );
        rejects_metadata(&mut tsdb, stream);
    }
}

/// Cleanup takes an L0 chunk's keys out of consolidation's bookkeeping whatever kind its
/// entry records, so an entry whose kind was damaged to another valid kind leaves none behind.
#[test]
fn cleanup_forgets_the_l0_bookkeeping_whatever_kind_is_recorded() {
    let books = |tsdb: &Tsdb<MemoryStorage>| -> Vec<_> {
        dump_storage(tsdb)
            .into_iter()
            .filter(|(ns, ..)| *ns == CONSOLIDATION.0)
            .collect()
    };
    for stream in [HOURS, EVENTS] {
        let mut left = Vec::new();
        for damaged in [false, true] {
            let disk = MemoryDisk::new();
            let mut tsdb = open_with(
                &disk,
                Profile {
                    journal_bytes: 1,
                    ..Profile::desktop()
                },
            );
            let listed = expired(&mut tsdb, stream, 1)[0];
            assert!(!books(&tsdb).is_empty(), "the L0 chunk has bookkeeping");
            if damaged {
                let entry = ChunkEntry {
                    kind: ChunkKind::Consolidated as u8,
                    ..listed.entry
                };
                overwrite(&tsdb, stream, &listed, &entry);
            }
            let done = finish(&mut tsdb, 2 * DAY_MS, &retention(stream));
            assert_eq!(done.chunks_deleted, 1);
            left.push(books(&tsdb));
        }
        assert_eq!(left[0], left[1]);
        assert_eq!(left[1], []);
    }
}

/// A bad entry past the fixed listing window fails the round after it deleted the chunks
/// before it, and the round's transaction with them: nothing changes.
#[test]
fn a_bad_entry_late_in_a_round_fails_it_whole() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(
        &disk,
        Profile {
            journal_bytes: 1,
            cleanup_chunks: 66,
            ..Profile::desktop()
        },
    );
    let listed = expired(&mut tsdb, HOURS, 66)[65];
    overwrite(
        &tsdb,
        HOURS,
        &listed,
        &ChunkEntry {
            fragments: 0,
            ..listed.entry
        },
    );
    changes_nothing(&mut tsdb, |tsdb| {
        run(tsdb, 2 * DAY_MS, &retention(HOURS), 1).unwrap_err()
    });
}

/// A round validates only its D selected chunks. Prefix deletion removes sparse and extra
/// fragment keys, without decoding an expired payload or trusting its advertised count.
fn deletes_prefixes<S: Storage>(tsdb: &mut Tsdb<S>, stream: StreamRef) {
    let listed = expired(tsdb, stream, 2);
    let (first, second) = (listed[0], listed[1]);
    let sentinel = FragmentId {
        chunk: ChunkId(u64::MAX),
        index: u32::MAX,
    };
    let probes = [0, 1, first.entry.fragments, u32::MAX - 1, u32::MAX];
    damage(tsdb, |batch| {
        // Replace the valid payload by sparse garbage. Metadata remains valid, and cleanup
        // must remove it without decoding it, together with unadvertised fragments.
        batch.delete_chunk(first.chunk).unwrap();
        for index in [0, first.entry.fragments, u32::MAX] {
            batch
                .put_fragment(
                    FragmentId {
                        chunk: first.chunk,
                        index,
                    },
                    b"expired",
                )
                .unwrap();
        }
        batch.put_fragment(sentinel, b"keep").unwrap();
    });
    overwrite(
        tsdb,
        stream,
        &second,
        &ChunkEntry {
            fragments: 0,
            ..second.entry
        },
    );
    let done = run(tsdb, 2 * DAY_MS, &retention(stream), 1).unwrap();
    assert_eq!(
        (done.chunks_deleted, done.cleanups, done.more),
        (1, 1, true)
    );
    let read = tsdb.storage().snapshot().unwrap();
    for index in probes {
        read.fragment(
            FragmentId {
                chunk: first.chunk,
                index,
            },
            MAX_FRAGMENT_LEN,
            |bytes| {
                assert!(bytes.is_none(), "fragment {index} survived prefix deletion");
            },
        )
        .unwrap();
    }
    read.fragment(sentinel, MAX_FRAGMENT_LEN, |bytes| {
        assert_eq!(bytes, Some(&b"keep"[..]))
    })
    .unwrap();
    assert_eq!(
        manifest::stream_chunks(&read, stream).unwrap(),
        [manifest::Listed {
            entry: ChunkEntry {
                fragments: 0,
                ..second.entry
            },
            ..second
        }]
    );
    drop(read);
    let error = run(tsdb, 2 * DAY_MS, &retention(stream), 1).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    overwrite(tsdb, stream, &second, &second.entry);
    let done = finish(tsdb, 2 * DAY_MS, &retention(stream));
    assert_eq!(done.chunks_deleted, 1);
    check_accounting(tsdb);
}

/// Both series and event cleanup obey D and delete sparse or extra fragments on memory.
#[test]
fn cleanup_deletes_fragments_by_prefix_within_its_limit() {
    for stream in [HOURS, EVENTS] {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(
            &disk,
            Profile {
                journal_bytes: 1,
                cleanup_chunks: 1,
                ..Profile::desktop()
            },
        );
        deletes_prefixes(&mut tsdb, stream);
    }
}

/// A stored empty extent fails in retention and in series or event consolidation planning.
#[test]
fn stored_empty_extents_fail_both_maintenance_planners() {
    for stream in [HOURS, EVENTS] {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        damage(&tsdb, |batch| {
            batch
                .put(
                    EXTENTS,
                    manifest::extent_key(stream, 0).as_slice(),
                    &encode(&ExtentRecord::default()),
                )
                .unwrap();
        });
        changes_nothing(&mut tsdb, |tsdb| {
            run(tsdb, 2 * DAY_MS, &retention(stream), 1).unwrap_err()
        });
        changes_nothing(&mut tsdb, |tsdb| {
            let read = tsdb.storage().snapshot().unwrap();
            plan_backlog(
                &read,
                &tsdb.round_context(&tsdb.current()),
                Timestamp(2 * DAY_MS),
            )
            .unwrap_err()
        });
    }
}

/// Repeat the metadata and prefix contracts against the durable redb engine.
#[cfg(feature = "redb")]
#[test]
fn cleanup_metadata_and_prefix_deletion_on_redb() {
    use crate::{db::tests::schema, storage::redb::RedbStorage};

    for stream in [HOURS, EVENTS] {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile {
            journal_bytes: 1,
            cleanup_chunks: 1,
            ..Profile::desktop()
        };
        let storage =
            RedbStorage::open(dir.path().join("cleanup.redb"), profile.backend_cache).unwrap();
        let mut tsdb = Tsdb::open(storage, schema(), profile).unwrap();
        rejects_metadata(&mut tsdb, stream);
        drop(tsdb);
        let storage = RedbStorage::open(
            dir.path().join("prefix.redb"),
            Profile::desktop().backend_cache,
        )
        .unwrap();
        let profile = Profile {
            journal_bytes: 1,
            cleanup_chunks: 1,
            ..Profile::desktop()
        };
        let mut tsdb = Tsdb::open(storage, schema(), profile).unwrap();
        deletes_prefixes(&mut tsdb, stream);
    }
}
