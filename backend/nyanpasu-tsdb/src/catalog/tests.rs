//! The catalog as a whole: versions, persistence, the fold ladder, the budget, id reuse and
//! the checks on releases.

use std::{collections::BTreeMap, ops::ControlFlow, time::Duration};

use proptest::prelude::*;

use super::{
    error::CatalogProblem,
    ids::MAX_ID,
    paged::page_bytes,
    persist::{self, combo_key, index_key, label_key, pin_key, raw},
    *,
};
use crate::{
    Aggregate, ErrorKind, FieldSpec, LabelSpec, Profile, TierSpec,
    layout::{COMBO_INDEX, COMBOS, LABEL_USES, LABELS, PIN_KEYS, PINS, record::RecordProblem},
    schema::MAX_DIMENSIONS,
    storage::{
        self, ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, Ns, Storage,
        StorageError,
        memory::{MemoryBatch, MemoryDisk, MemoryStorage},
    },
};

const RULE: LabelId = LabelId(0);
const PROCESS: LabelId = LabelId(1);
const TARGET: LabelId = LabelId(2);
/// A label only events carry.
const HOST: LabelId = LabelId(3);

/// Traffic-like: the target folds first, then the whole combination (§11.1, §14.1).
fn schema() -> Schema {
    let schema = Schema {
        version: 1,
        labels: ["rule", "process", "target", "host"]
            .into_iter()
            .map(|name| LabelSpec { name })
            .collect(),
        series: Some(SeriesSpec {
            dimensions: vec![RULE, PROCESS, TARGET],
            fields: vec![FieldSpec {
                name: "download",
                aggregate: Aggregate::SaturatingSum,
            }],
            tiers: vec![TierSpec {
                name: "hour",
                bucket: Duration::from_secs(3600),
                extent: Duration::from_secs(86_400),
            }],
            fold: vec![vec![TARGET]],
        }),
        events: Vec::new(),
    };
    schema.validate().unwrap();
    schema
}

/// A new catalog whose values hash under fixed keys, as every catalog of these tests does:
/// what a write costs depends on the table pages its values land in, so a budget test would
/// otherwise measure another layout on each run.
fn new_catalog(schema: &Schema) -> Catalog {
    Catalog::with_hasher(schema, ValueHasher::fixed_keys())
}

/// [`Catalog::load`], with the values hashed under fixed keys.
fn load_catalog(
    read: &impl StorageRead,
    schema: &Schema,
    limit: u64,
) -> Result<Catalog, LoadError> {
    Catalog::load_with(read, schema, ValueHasher::fixed_keys(), limit)
}

/// [`persist::load`], with the values hashed under fixed keys.
fn load_inner(
    read: &impl StorageRead,
    schema: &Schema,
    limit: u64,
    ledger: &mut Ledger,
) -> Result<Inner, LoadError> {
    persist::load(read, schema, ValueHasher::fixed_keys(), limit, ledger)
}

fn open() -> MemoryStorage {
    MemoryDisk::new().open().unwrap()
}

fn commit(batch: MemoryBatch<'_>) {
    batch.commit(Durability::Durable).unwrap();
}

/// Runs `edit` on a draft of `catalog` in a batch, then finishes and commits both.
fn apply(
    storage: &MemoryStorage,
    catalog: &Catalog,
    limit: u64,
    edit: impl FnOnce(&mut CatalogDraft, &mut MemoryBatch<'_>),
) -> Catalog {
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(limit, &crate::budget::test_account())
        .unwrap();
    edit(&mut draft, &mut batch);
    let next = draft.finish().0;
    commit(batch);
    next
}

fn resolve(draft: &mut CatalogDraft, batch: &mut MemoryBatch<'_>, values: [&str; 3]) -> u32 {
    let values = values.map(str::as_bytes);
    let resolved = draft.resolve_combo(batch, &values).unwrap();
    assert_eq!(resolved.fold, None, "{values:?} folded");
    resolved.id
}

fn labels(combo: Option<ComboLabels<'_>>) -> Option<Vec<u32>> {
    combo.map(Iterator::collect)
}

/// Every key of `ns`.
fn keys(read: &impl StorageRead, ns: Ns) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    read.scan(ns, KeyRange::All, Direction::Forward, |key, _| {
        keys.push(key.to_vec());
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    keys
}

/// Every value of `ns`.
fn values(read: &impl StorageRead, ns: Ns) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    read.scan(ns, KeyRange::All, Direction::Forward, |_, value| {
        values.push(value.to_vec());
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    values
}

#[test]
fn combinations_and_values_round_trip() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();

    let first = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    let second = resolve(&mut draft, &mut batch, ["match", "curl", "b.com"]);
    // The empty value and the literal "(other)" are ordinary values (§5.2).
    let third = resolve(&mut draft, &mut batch, ["", "(other)", "a.com"]);
    assert_eq!((first, second, third), (1, 2, 3));
    assert_eq!(
        resolve(&mut draft, &mut batch, ["match", "curl", "b.com"]),
        2
    );

    let match_id = draft.find_label(RULE, b"match").unwrap();
    let curl = draft.find_label(PROCESS, b"curl").unwrap();
    let b_com = draft.find_label(TARGET, b"b.com").unwrap();
    assert_eq!(labels(draft.combo(2)), Some(vec![match_id, curl, b_com]));
    assert_eq!(draft.label(TARGET, b_com), Some(&b"b.com"[..]));
    assert_eq!(
        draft.label(RULE, draft.find_label(RULE, b"").unwrap()),
        Some(&b""[..])
    );
    assert_ne!(draft.find_label(PROCESS, b"(other)"), Some(0));
    // Id 0 is the overflow marker in every dimension, and combination 0 is all of them.
    assert_eq!(draft.label(TARGET, 0), None);
    assert_eq!(labels(draft.combo(0)), Some(vec![0, 0, 0]));
    assert_eq!(labels(draft.combo(4)), None);

    // A label of an event column has its own dictionary.
    let host = draft.resolve_label(&mut batch, HOST, b"a.com").unwrap();
    assert_eq!(host, 1);
    assert_eq!(draft.resolve_label(&mut batch, HOST, b"a.com").unwrap(), 1);
    assert_eq!(draft.label(HOST, 1), Some(&b"a.com"[..]));

    let catalog = draft.finish().0;
    commit(batch);
    assert_eq!(labels(catalog.combo(2)), Some(vec![match_id, curl, b_com]));
    assert_eq!(catalog.find_label(HOST, b"a.com"), Some(1));
    let snapshot = storage.snapshot().unwrap();
    assert_eq!(keys(&snapshot, COMBOS).len(), 3);
    assert_eq!(keys(&snapshot, COMBO_INDEX).len(), 3);
    // rule: 2, process: 2, target: 2, host: 1.
    assert_eq!(keys(&snapshot, LABELS).len(), 7);
}

#[test]
fn a_draft_is_invisible_in_its_base_and_old_versions_stay_unchanged() {
    let storage = open();
    let v1 = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            // Enough targets for several pages in every array of the target dictionary.
            for n in 0..2000 {
                resolve(
                    draft,
                    batch,
                    ["match", "curl", &format!("host-{n}.example")],
                );
            }
            draft.resolve_label(batch, HOST, b"h").unwrap();
        },
    );
    let v1_contents = v1.contents();

    let mut batch = storage.batch().unwrap();
    let mut draft = v1.draft(u64::MAX, &crate::budget::test_account()).unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "new.example"]);
    assert_eq!(v1.find_label(TARGET, b"new.example"), None);
    assert_eq!(labels(v1.combo(id)), None);
    // What the draft holds is exactly what it does not share with its base.
    assert_eq!(draft.owned(), draft.unshared());
    let owned = draft.owned();
    let v2 = draft.finish().0;
    commit(batch);

    // The new target copied a page and the spine of each array it wrote, not the arrays.
    assert_eq!(v2.inner.unshared(&v1.inner), owned);
    assert!(owned < v2.bytes() / 2, "{owned} of {}", v2.bytes());
    let target = usize::from(TARGET.0);
    assert!(
        v2.inner.dictionaries[target].unshared(&v1.inner.dictionaries[target])
            <= 4 * (PAGE_ALLOC as u64 + spine_bytes::<u64>(16))
    );
    // Untouched dictionaries are shared whole.
    let host = usize::from(HOST.0);
    assert!(v2.inner.dictionaries[host].same(&v1.inner.dictionaries[host]));

    let v3 = apply(&storage, &v2, u64::MAX, |draft, batch| {
        draft.resolve_label(batch, HOST, b"i").unwrap();
    });
    let v4 = apply(&storage, &v3, u64::MAX, |draft, batch| {
        resolve(draft, batch, ["direct", "curl", "host-1.example"]);
    });
    assert!(v4.inner.dictionaries[target].same(&v2.inner.dictionaries[target]));
    assert!(v4.inner.dictionaries[host].same(&v3.inner.dictionaries[host]));
    assert!(!v3.inner.dictionaries[host].same(&v1.inner.dictionaries[host]));

    // The old version reads as it did, after three later versions.
    assert_eq!(v1.contents(), v1_contents);
    assert_eq!(v1.find_label(HOST, b"i"), None);
    assert_eq!(v4.find_label(HOST, b"i"), Some(2));
}

#[test]
fn a_draft_that_changes_nothing_finishes_as_its_base() {
    let storage = open();
    let v1 = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            resolve(draft, batch, ["match", "curl", "a.com"]);
        },
    );
    let v2 = apply(&storage, &v1, u64::MAX, |draft, batch| {
        resolve(draft, batch, ["match", "curl", "a.com"]);
        assert!(draft.pin(batch, b"conn", 1).unwrap());
    });
    assert!(Arc::ptr_eq(&v1.inner, &v2.inner));
}

#[test]
fn a_loaded_catalog_matches_the_one_written_with_ids_gaps_and_pins() {
    let storage = open();
    let schema = schema();
    let v1 = apply(&storage, &new_catalog(&schema), u64::MAX, |draft, batch| {
        for n in 0..6 {
            resolve(draft, batch, ["match", &format!("p{n}"), &format!("t{n}")]);
        }
        for value in [&b""[..], b"h1", b"h2", &[0xff; 5000]] {
            draft.resolve_label(batch, HOST, value).unwrap();
        }
        for (key, id) in [(&b"a"[..], 3), (b"b", 3), (b"c", 5)] {
            assert!(draft.pin(batch, key, id).unwrap());
        }
    });
    let v2 = apply(&storage, &v1, u64::MAX, |draft, batch| {
        assert!(draft.unpin(batch, b"c").unwrap().is_some());
        for id in [2, 4] {
            let _ = draft.release_combo(batch, id).unwrap();
        }
        // The values only those combinations used.
        for (dimension, value) in [(PROCESS, "p1"), (TARGET, "t3")] {
            let id = draft.find_label(dimension, value.as_bytes()).unwrap();
            draft.release_label(batch, dimension, id).unwrap();
        }
        let h1 = draft.find_label(HOST, b"h1").unwrap();
        draft.release_label(batch, HOST, h1).unwrap();
        // A folded combination: room for the combination but not for its new target.
        let cost = draft.combo_cost();
        draft.set_room(cost);
        let folded = draft
            .resolve_combo(batch, &[b"match", b"p0", b"t9"])
            .unwrap();
        assert_eq!(folded.fold, Some(Fold::Step(0)));
    });

    let snapshot = storage.snapshot().unwrap();
    let loaded = load_catalog(&snapshot, &schema, u64::MAX).unwrap();
    let contents = v2.contents();
    assert_eq!(loaded.contents(), contents);
    // The folded combination took the smallest free id, 2.
    assert_eq!(contents.combos.gaps, [4]);
    assert_eq!(labels(v2.combo(2)).map(|labels| labels[2]), Some(0));
    assert_eq!(contents.labels[usize::from(PROCESS.0)].gaps, [2]);
    assert_eq!(contents.labels[usize::from(HOST.0)].gaps, [2]);
    assert_eq!(pin_count(&snapshot, 3).unwrap(), 2);
    assert!(is_pinned(&snapshot, 3).unwrap());
    assert!(!is_pinned(&snapshot, 5).unwrap());
    assert!(!is_pinned(&snapshot, 1).unwrap());
    assert_eq!(keys(&snapshot, PIN_KEYS), [b"a", b"b"]);

    // Both reuse the same ids next.
    let next_ids = |catalog: &Catalog| {
        let mut batch = storage.batch().unwrap();
        let mut draft = catalog
            .draft(u64::MAX, &crate::budget::test_account())
            .unwrap();
        let combo = resolve(&mut draft, &mut batch, ["direct", "p0", "t0"]);
        let process = draft.resolve_label(&mut batch, PROCESS, b"p9").unwrap();
        let host = draft.resolve_label(&mut batch, HOST, b"h9").unwrap();
        (combo, process, host)
    };
    assert_eq!(next_ids(&v2), (4, 2, 2));
    assert_eq!(next_ids(&loaded), (4, 2, 2));
}

/// Writes `ns`/`key` = `value` over a store holding one combination, then loads it.
fn load_with(schema: &Schema, ns: Ns, key: &[u8], value: &[u8]) -> LoadError {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    if schema.series.is_some() {
        let mut draft = new_catalog(schema)
            .draft(u64::MAX, &crate::budget::test_account())
            .unwrap();
        resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    }
    batch.put(ns, key, value).unwrap();
    commit(batch);
    let error = load_catalog(&storage.snapshot().unwrap(), schema, u64::MAX)
        .err()
        .expect("loading a corrupted record fails");
    assert_eq!(error.kind(), ErrorKind::Corrupted, "{error}");
    error
}

#[test]
fn every_invalid_record_fails_to_load_as_corrupted() {
    let schema = schema();
    let label = |dimension, id, value: &[u8]| match load_with(
        &schema,
        LABELS,
        label_key(LabelId(dimension), id).as_slice(),
        value,
    ) {
        LoadError::InvalidLabel {
            dimension: d,
            id: i,
            problem,
        } if (d, i) == (dimension, id) => problem,
        error => panic!("unexpected {error:?}"),
    };
    assert_eq!(
        label(0, 9, &[2, b'x']),
        CatalogProblem::Encoding {
            problem: RecordProblem::Version { found: 2 }
        }
    );
    assert_eq!(
        label(0, 9, &[]),
        CatalogProblem::Encoding {
            problem: RecordProblem::Empty
        }
    );
    // The value claims five bytes and has one.
    assert_eq!(
        label(0, 9, &[1, 5, b'x']),
        CatalogProblem::Encoding {
            problem: RecordProblem::Malformed {
                error: postcard::Error::DeserializeUnexpectedEnd
            }
        }
    );
    assert_eq!(
        label(9, 1, &raw::label_record(b"x")),
        CatalogProblem::UnknownDimension { labels: 4 }
    );
    assert_eq!(
        label(0, 0, &raw::label_record(b"x")),
        CatalogProblem::ReservedId
    );
    assert_eq!(
        label(0, u32::MAX, &raw::label_record(b"x")),
        CatalogProblem::ReservedId
    );
    assert_eq!(
        label(0, 9, &raw::label_record(b"match")),
        CatalogProblem::DuplicateValue { first: 1 }
    );

    let mut long_key = label_key(LabelId(0), 2).as_slice().to_vec();
    long_key.push(0);
    assert!(matches!(
        load_with(&schema, LABELS, &long_key, &raw::label_record(b"x")),
        LoadError::LabelKey { .. }
    ));

    let combo = |id, value: &[u8]| match load_with(&schema, COMBOS, combo_key(id).as_slice(), value)
    {
        LoadError::InvalidCombo { id: i, problem } if i == id => problem,
        error => panic!("unexpected {error:?}"),
    };
    assert_eq!(
        combo(7, &raw::combo_record(&[1, 1])),
        CatalogProblem::LabelCount {
            count: 2,
            expected: 3
        }
    );
    assert_eq!(
        combo(7, &[raw::combo_record(&[1, 1, 1]), vec![0]].concat()),
        CatalogProblem::Encoding {
            problem: RecordProblem::Trailing { extra: 1 }
        }
    );
    assert_eq!(
        combo(7, &[]),
        CatalogProblem::Encoding {
            problem: RecordProblem::Empty
        }
    );
    assert_eq!(
        combo(7, &[3; 13]),
        CatalogProblem::Encoding {
            problem: RecordProblem::Version { found: 3 }
        }
    );
    assert_eq!(
        combo(7, &raw::combo_record(&[50, 1, 1])),
        CatalogProblem::UnknownLabel {
            position: 0,
            label: 50
        }
    );
    // A label of another dimension's dictionary is not a label of this one.
    assert_eq!(
        combo(7, &raw::combo_record(&[1, 1, 2])),
        CatalogProblem::UnknownLabel {
            position: 2,
            label: 2
        }
    );
    assert_eq!(
        combo(7, &raw::combo_record(&[0, 0, 0])),
        CatalogProblem::WholeOverflow
    );
    assert_eq!(
        combo(0, &raw::combo_record(&[1, 1, 1])),
        CatalogProblem::ReservedId
    );
    // A list of labels decodes up to the most dimensions a schema has, and no further, even
    // when the record holds every label its length claims.
    assert_eq!(
        combo(7, &raw::combo_record(&[1; MAX_DIMENSIONS])),
        CatalogProblem::LabelCount {
            count: MAX_DIMENSIONS,
            expected: 3
        }
    );
    for count in [MAX_DIMENSIONS + 1, 1_000_000] {
        assert_eq!(
            combo(7, &raw::combo_record(&vec![1; count])),
            CatalogProblem::Encoding {
                problem: RecordProblem::Malformed {
                    error: postcard::Error::SerdeDeCustom
                }
            },
            "{count} labels"
        );
    }
    assert!(matches!(
        load_with(&schema, COMBOS, &[0, 1], &raw::combo_record(&[1, 1, 1])),
        LoadError::ComboKey { .. }
    ));

    let without_series = Schema {
        series: None,
        ..schema.clone()
    };
    assert!(matches!(
        load_with(&without_series, COMBOS, combo_key(1).as_slice(), &[1]),
        LoadError::InvalidCombo {
            id: 1,
            problem: CatalogProblem::NoSeries
        }
    ));
}

#[test]
fn a_record_the_backend_reports_damaged_fails_to_load_with_its_kind() {
    let storage = open();
    let schema = schema();
    apply(&storage, &new_catalog(&schema), u64::MAX, |draft, batch| {
        resolve(draft, batch, ["match", "curl", "a.com"]);
    });
    storage.corrupt_entry(COMBOS, combo_key(1).as_slice());
    let error = load_catalog(&storage.snapshot().unwrap(), &schema, u64::MAX)
        .err()
        .unwrap();
    assert!(matches!(error, LoadError::ScanCombos { .. }), "{error:?}");
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

#[test]
fn a_bad_pin_record_is_corrupted() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    batch.put(PINS, pin_key(id).as_slice(), &[7]).unwrap();
    for error in [
        pin_count(&batch, id).unwrap_err(),
        draft.pin(&mut batch, b"conn", id).unwrap_err(),
    ] {
        assert!(
            matches!(
                error,
                PinError::InvalidRecord {
                    id: 1,
                    problem: CatalogProblem::Encoding {
                        problem: RecordProblem::Version { found: 7 }
                    }
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    // A key record of an unknown version, and one naming the reserved id.
    for (record, problem) in [
        (
            vec![7],
            CatalogProblem::Encoding {
                problem: RecordProblem::Version { found: 7 },
            },
        ),
        (raw::pin_key_record(u32::MAX), CatalogProblem::ReservedId),
    ] {
        batch.put(PIN_KEYS, b"bad", &record).unwrap();
        for error in [
            draft.pin(&mut batch, b"bad", id).unwrap_err(),
            draft.unpin(&mut batch, b"bad").unwrap_err(),
        ] {
            assert!(
                matches!(&error, PinError::InvalidKey { key, problem: found }
                    if **key == *b"bad" && *found == problem),
                "{error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
        }
    }
}

#[test]
fn a_bad_reverse_lookup_record_is_corrupted() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            resolve(draft, batch, ["match", "curl", "a.com"]);
            resolve(draft, batch, ["direct", "wget", "b.com"]);
        },
    );
    let mut batch = storage.batch().unwrap();
    // A record of an unknown version.
    batch
        .put(
            COMBO_INDEX,
            index_key(&[1, 1, 1]).as_slice(),
            &[9, 1, 0, 0, 0],
        )
        .unwrap();
    // A record naming a combination with other labels.
    batch
        .put(
            COMBO_INDEX,
            index_key(&[1, 1, 2]).as_slice(),
            &raw::index_record(1),
        )
        .unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let error = draft
        .resolve_combo(&mut batch, &[b"match", b"curl", b"a.com"])
        .unwrap_err();
    assert!(
        matches!(
            error,
            ResolveError::InvalidComboIndex {
                problem: CatalogProblem::Encoding {
                    problem: RecordProblem::Version { found: 9 }
                }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    let error = draft
        .resolve_combo(&mut batch, &[b"match", b"curl", b"b.com"])
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::StaleComboIndex { id: 1 }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

/// A write transaction whose writes fail, as a failed backend's do; reads go through.
struct FailingWrites<'a, W>(&'a mut W);

fn injected() -> StorageError {
    StorageError::Failed {
        source: "injected".into(),
    }
}

impl<W: StorageRead> StorageRead for FailingWrites<'_, W> {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> storage::Result<T> {
        self.0.get(ns, key, f)
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> storage::Result<Option<B>> {
        self.0.scan(ns, range, direction, f)
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> storage::Result<T> {
        self.0.fragment(id, limit, f)
    }
}

impl<W: StorageWrite> StorageWrite for FailingWrites<'_, W> {
    fn put(&mut self, _: Ns, _: &[u8], _: &[u8]) -> storage::Result<()> {
        Err(injected())
    }

    fn delete(&mut self, _: Ns, _: &[u8]) -> storage::Result<()> {
        Err(injected())
    }

    fn put_fragment(&mut self, _: FragmentId, _: &[u8]) -> storage::Result<()> {
        Err(injected())
    }

    fn delete_chunk(&mut self, _: ChunkId) -> storage::Result<()> {
        Err(injected())
    }

    fn commit(self, _: Durability) -> Result<(), CommitFailure> {
        unreachable!("the tests never commit it")
    }
}

#[test]
fn a_failed_write_reports_the_backend_failure_and_changes_nothing_in_memory() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            resolve(draft, batch, ["match", "curl", "a.com"]);
        },
    );
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let mut failing = FailingWrites(&mut batch);
    let error = draft
        .resolve_combo(&mut failing, &[b"match", b"curl", b"b.com"])
        .unwrap_err();
    assert!(
        matches!(
            error,
            ResolveError::WriteLabel {
                dimension: 2,
                id: 2,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert_eq!(draft.find_label(TARGET, b"b.com"), None);

    let error = draft.pin(&mut failing, b"conn", 1).unwrap_err();
    assert!(
        matches!(error, PinError::WriteCount { id: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    let error = draft.release_combo(&mut failing, 1).unwrap_err();
    assert!(
        matches!(error, ReleaseError::DeleteCombo { id: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert_eq!(labels(draft.combo(1)).map(|labels| labels.len()), Some(3));
}

#[test]
#[should_panic(expected = "after a write to its batch failed")]
fn finishing_a_draft_after_a_failed_write_panics() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let _ = draft.resolve_label(&mut FailingWrites(&mut batch), HOST, b"x");
    draft.finish();
}

#[test]
fn the_fold_ladder_takes_the_first_step_that_fits() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            resolve(draft, batch, ["match", "curl", "a.com"]);
        },
    );
    let values: [&[u8]; 3] = [b"match", b"wget", b"b.com"];

    // Room for every new value and the combination: no fold.
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let full = draft.value_cost(PROCESS, b"wget")
        + draft.value_cost(TARGET, b"b.com")
        + draft.combo_cost();
    draft.set_room(full);
    let resolved = draft.resolve_combo(&mut batch, &values).unwrap();
    assert_eq!(resolved, ResolvedCombo { id: 2, fold: None });
    assert!(draft.peak() <= draft.limit - draft.reserve());
    drop((draft, batch));

    // One byte short of that: the target folds, and the new process is still inserted.
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    draft.set_room(full - 1);
    let resolved = draft.resolve_combo(&mut batch, &values).unwrap();
    assert_eq!(
        resolved,
        ResolvedCombo {
            id: 2,
            fold: Some(Fold::Step(0))
        }
    );
    let wget = draft.find_label(PROCESS, b"wget").unwrap();
    assert_eq!(labels(draft.combo(2)), Some(vec![1, wget, 0]));
    // The discarded target is in neither the version nor storage.
    assert_eq!(draft.find_label(TARGET, b"b.com"), None);
    assert!(
        !values_contain(&batch, LABELS, b"b.com"),
        "the folded target was written"
    );
    assert_eq!(
        keys(&batch, COMBOS),
        [combo_key(1).as_slice(), combo_key(2).as_slice()]
    );
    assert_eq!(
        keys(&batch, COMBO_INDEX),
        [
            index_key(&[1, 1, 1]).as_slice(),
            index_key(&[1, wget, 0]).as_slice(),
        ]
    );
    // The same values again find the folded combination, at no cost.
    let owned = draft.owned();
    assert_eq!(draft.resolve_combo(&mut batch, &values).unwrap(), resolved);
    assert_eq!(draft.owned(), owned);
    let folded = draft.finish().0;
    commit(batch);

    // An existing folded combination is found even with no room at all.
    let mut batch = storage.batch().unwrap();
    let mut draft = folded
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    draft.set_room(0);
    let resolved = draft
        .resolve_combo(&mut batch, &[b"match", b"wget", b"c.com"])
        .unwrap();
    assert_eq!(
        resolved,
        ResolvedCombo {
            id: 2,
            fold: Some(Fold::Step(0))
        }
    );
    assert!(Fold::Step(0).folds(schema().series.as_ref().unwrap(), TARGET));
    assert!(!Fold::Step(0).folds(schema().series.as_ref().unwrap(), PROCESS));
}

#[test]
fn the_whole_combination_overflows_last() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            resolve(draft, batch, ["match", "curl", "a.com"]);
        },
    );
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    // One byte short of the folded step, which needs the new process.
    let folded = draft.value_cost(PROCESS, b"wget") + draft.combo_cost();
    draft.set_room(folded - 1);
    let resolved = draft
        .resolve_combo(&mut batch, &[b"match", b"wget", b"b.com"])
        .unwrap();
    assert_eq!(
        resolved,
        ResolvedCombo {
            id: 0,
            fold: Some(Fold::Whole)
        }
    );
    assert!(Fold::Whole.folds(schema().series.as_ref().unwrap(), RULE));
    // No candidate leaked: the draft and the batch hold only the first combination.
    assert_eq!(draft.find_label(PROCESS, b"wget"), None);
    assert_eq!(draft.find_label(TARGET, b"b.com"), None);
    assert_eq!(draft.owned(), draft.unshared());
    assert!(!values_contain(&batch, LABELS, b"wget"));
    assert_eq!(keys(&batch, COMBOS), [combo_key(1).as_slice()]);
    assert_eq!(keys(&batch, COMBO_INDEX).len(), 1);
    assert!(Arc::ptr_eq(&draft.finish().0.inner, &catalog.inner));
    drop(batch);

    // An event label that does not fit is the overflow marker.
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let cost = draft.value_cost(HOST, b"h");
    draft.set_room(cost - 1);
    assert_eq!(draft.resolve_label(&mut batch, HOST, b"h").unwrap(), 0);
    draft.set_room(cost);
    assert_eq!(draft.resolve_label(&mut batch, HOST, b"h").unwrap(), 1);
}

fn values_contain(read: &impl StorageRead, ns: Ns, needle: &[u8]) -> bool {
    values(read, ns).iter().any(|value| value.ends_with(needle))
}

#[test]
fn released_ids_are_reused_smallest_first() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            for n in 1..=6 {
                resolve(draft, batch, ["match", "curl", &format!("t{n}")]);
            }
        },
    );
    let catalog = apply(&storage, &catalog, u64::MAX, |draft, batch| {
        for id in [5, 2, 4] {
            let _ = draft.release_combo(batch, id).unwrap();
        }
        for target in ["t5", "t2"] {
            let id = draft.find_label(TARGET, target.as_bytes()).unwrap();
            draft.release_label(batch, TARGET, id).unwrap();
        }
    });
    apply(&storage, &catalog, u64::MAX, |draft, batch| {
        let ids: Vec<u32> = ["n1", "n2", "n3", "n4"]
            .map(|target| resolve(draft, batch, ["match", "curl", target]))
            .into();
        assert_eq!(ids, [2, 4, 5, 7]);
        let targets: Vec<u32> = ["n1", "n2", "n3", "n4"]
            .map(|target| draft.find_label(TARGET, target.as_bytes()).unwrap())
            .into();
        assert_eq!(targets, [2, 5, 7, 8]);
        // A reused id has the new value, and the old one is gone.
        assert_eq!(draft.label(TARGET, 2), Some(&b"n1"[..]));
        assert_eq!(draft.find_label(TARGET, b"t2"), None);
    });
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "which is pinned")]
fn releasing_a_pinned_combination_trips_a_debug_assertion() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    assert!(draft.pin(&mut batch, b"conn", id).unwrap());
    let _ = draft.release_combo(&mut batch, id);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "which live combination 1 uses")]
fn releasing_a_label_a_combination_uses_trips_a_debug_assertion() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    let curl = draft.find_label(PROCESS, b"curl").unwrap();
    let _ = draft.release_label(&mut batch, PROCESS, curl);
}

#[test]
fn a_pinned_combination_can_be_released_once_unpinned() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    assert!(draft.pin(&mut batch, b"conn", id).unwrap());
    assert!(draft.unpin(&mut batch, b"conn").unwrap().is_some());
    assert_eq!(pin_count(&batch, id).unwrap(), 0);
    // The last unpin deletes the count, and every unpin its key.
    assert_eq!(keys(&batch, PINS), Vec::<Vec<u8>>::new());
    assert_eq!(keys(&batch, PIN_KEYS), Vec::<Vec<u8>>::new());
    let _ = draft.release_combo(&mut batch, id).unwrap();
    assert_eq!(labels(draft.combo(id)), None);
}

/// The uses `read` counts of the value `value` of `dimension` in `draft`.
fn uses(draft: &CatalogDraft, read: &impl StorageRead, dimension: LabelId, value: &str) -> u64 {
    let id = draft.find_label(dimension, value.as_bytes()).unwrap();
    label_uses(read, dimension, id).unwrap()
}

// Each label value counts the live combinations that use it, in storage: a new combination
// adds a use to each of its values but the overflow marker, and a released one takes them off
// and returns the values it left without uses (§5.3).
#[test]
fn a_label_value_counts_the_live_combinations_that_use_it() {
    let storage = open();
    let mut ids = [0; 2];
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            ids[0] = resolve(draft, batch, ["match", "curl", "a.com"]);
            ids[1] = resolve(draft, batch, ["match", "wget", "a.com"]);
            // An existing combination adds nothing.
            assert_eq!(resolve(draft, batch, ["match", "curl", "a.com"]), ids[0]);
        },
    );
    let [a, b] = ids;
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    // A folded one counts the values it keeps: room for the combination, but not for the
    // page its new target would copy.
    draft.set_room(draft.combo_cost());
    let folded = draft
        .resolve_combo(&mut batch, &[b"match", b"curl", b"z.com"])
        .unwrap();
    assert_eq!(folded.fold, Some(Fold::Step(0)));
    let counts = |draft: &CatalogDraft, batch: &MemoryBatch<'_>| {
        [
            (RULE, "match"),
            (PROCESS, "curl"),
            (PROCESS, "wget"),
            (TARGET, "a.com"),
        ]
        .map(|(dimension, value)| uses(draft, batch, dimension, value))
    };
    assert_eq!(counts(&draft, &batch), [3, 2, 1, 2]);
    assert_eq!(
        values(&batch, LABEL_USES),
        [
            raw::uses_record(3),
            raw::uses_record(2),
            raw::uses_record(1),
            raw::uses_record(2)
        ]
    );

    let wget = draft.find_label(PROCESS, b"wget").unwrap();
    assert_eq!(
        draft
            .release_combo(&mut batch, b)
            .unwrap()
            .collect::<Vec<_>>(),
        [(PROCESS, wget)]
    );
    assert_eq!(counts(&draft, &batch), [2, 2, 0, 1]);
    let target = draft.find_label(TARGET, b"a.com").unwrap();
    assert_eq!(
        draft
            .release_combo(&mut batch, a)
            .unwrap()
            .collect::<Vec<_>>(),
        [(TARGET, target)]
    );
    let (rule, process) = (
        draft.find_label(RULE, b"match").unwrap(),
        draft.find_label(PROCESS, b"curl").unwrap(),
    );
    assert_eq!(
        draft
            .release_combo(&mut batch, folded.id)
            .unwrap()
            .collect::<Vec<_>>(),
        [(RULE, rule), (PROCESS, process)]
    );
    // A count of 0 has no record.
    assert_eq!(keys(&batch, LABEL_USES), Vec::<Vec<u8>>::new());
    draft.finish();
}

// A combination whose label has no uses left is damage: releasing it fails as corrupted.
#[test]
fn releasing_a_combination_whose_label_has_no_uses_is_corrupted() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    let curl = draft.find_label(PROCESS, b"curl").unwrap();
    batch
        .delete(LABEL_USES, label_key(PROCESS, curl).as_slice())
        .unwrap();
    let error = draft.release_combo(&mut batch, id).unwrap_err();
    assert!(
        matches!(
            &error,
            ReleaseError::UncountUses {
                id: 1,
                source: UsesError::UsesUnderflow {
                    dimension: 1,
                    id: 1
                }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    batch
        .put(LABEL_USES, label_key(RULE, 1).as_slice(), &[1])
        .unwrap();
    let error = label_uses(&batch, RULE, 1).unwrap_err();
    assert!(matches!(error, UsesError::InvalidUses { .. }), "{error:?}");
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

#[test]
fn a_combination_released_in_a_draft_resolves_anew_in_it() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    resolve(&mut draft, &mut batch, ["match", "curl", "b.com"]);
    let _ = draft.release_combo(&mut batch, id).unwrap();
    // The release deleted the combination's reverse lookup, so the same values make a new
    // one, which reuses the freed id.
    let again = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    assert_eq!(again, id);
    assert_eq!(labels(draft.combo(again)), Some(vec![1, 1, 1]));
    assert_eq!(
        keys(&batch, COMBOS),
        [combo_key(1).as_slice(), combo_key(2).as_slice()]
    );
    assert_eq!(keys(&batch, COMBO_INDEX).len(), 2);
}

/// A draft keeps nothing per resolution: resolving a combination again reads its reverse
/// lookup in the batch again.
#[test]
fn a_draft_finds_every_combination_through_the_reverse_lookup() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    assert_eq!(
        resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]),
        id
    );
    storage.fail_next(storage::conformance::OpClass::Read);
    let error = draft
        .resolve_combo(&mut batch, &[b"match", b"curl", b"a.com"])
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::ReadComboIndex { .. }),
        "{error:?}"
    );
}

/// Every pin count and pin key `read` holds.
fn pins(read: &impl StorageRead) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    (values(read, PINS), keys(read, PIN_KEYS))
}

#[test]
fn a_key_pins_one_combination_and_a_combination_counts_its_keys() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let first = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    let second = resolve(&mut draft, &mut batch, ["match", "curl", "b.com"]);
    for (key, id) in [
        (&b"a"[..], first),
        (b"b", first),
        (b"c", second),
        (b"", first),
    ] {
        assert!(draft.pin(&mut batch, key, id).unwrap());
    }
    assert_eq!(pin_count(&batch, first).unwrap(), 3);
    assert_eq!(pin_count(&batch, second).unwrap(), 1);
    // A key pins one combination: pinning it again writes nothing, whichever combination.
    let before = pins(&batch);
    assert!(!draft.pin(&mut batch, b"a", second).unwrap());
    assert!(!draft.pin(&mut batch, b"a", first).unwrap());
    assert_eq!(pins(&batch), before);

    // An unpin takes off the pin of its own key.
    assert!(draft.unpin(&mut batch, b"c").unwrap().is_some());
    assert_eq!(pin_count(&batch, second).unwrap(), 0);
    assert_eq!(pin_count(&batch, first).unwrap(), 3);
    for key in [&b"a"[..], b"b", b""] {
        assert!(draft.unpin(&mut batch, key).unwrap().is_some());
    }
    assert_eq!(pins(&batch), (Vec::new(), Vec::new()));
    // A key that pins nothing, now or ever, has nothing to unpin.
    assert!(draft.unpin(&mut batch, b"a").unwrap().is_none());
    assert!(draft.unpin(&mut batch, b"never").unwrap().is_none());
    assert_eq!(pins(&batch), (Vec::new(), Vec::new()));
    let _ = draft.release_combo(&mut batch, first).unwrap();
}

// Combination 0 is never released, so it has no count, but its keys are kept for the unpin.
#[test]
fn a_key_of_the_whole_overflow_has_no_count() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    assert!(draft.pin(&mut batch, b"conn", 0).unwrap());
    assert_eq!(pins(&batch), (Vec::new(), vec![b"conn".to_vec()]));
    assert!(!draft.pin(&mut batch, b"conn", 0).unwrap());
    assert!(draft.unpin(&mut batch, b"conn").unwrap().is_some());
    assert_eq!(pins(&batch), (Vec::new(), Vec::new()));
}

#[test]
fn a_key_whose_combination_has_no_count_fails_to_unpin_and_writes_nothing() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    batch
        .put(PIN_KEYS, b"conn", &raw::pin_key_record(id))
        .unwrap();
    let before = pins(&batch);
    let error = draft.unpin(&mut batch, b"conn").unwrap_err();
    assert!(
        matches!(error, PinError::CountUnderflow { id: 1 }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert_eq!(pins(&batch), before);
    draft.finish();
}

#[test]
fn a_damaged_pin_count_fails_to_pin_and_writes_nothing() {
    let storage = open();
    let mut batch = storage.batch().unwrap();
    let mut draft = new_catalog(&schema())
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let id = resolve(&mut draft, &mut batch, ["match", "curl", "a.com"]);
    // A record that decodes, with a count no consumer reaches.
    batch
        .put(PINS, pin_key(id).as_slice(), &raw::pin_record(u64::MAX))
        .unwrap();
    let namespaces = [LABELS, COMBOS, COMBO_INDEX, PINS, PIN_KEYS];
    let before = namespaces.map(|ns| (keys(&batch, ns), values(&batch, ns)));

    let error = draft.pin(&mut batch, b"conn", id).unwrap_err();
    assert!(
        matches!(error, PinError::CountOverflow { id: 1 }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert_eq!(
        namespaces.map(|ns| (keys(&batch, ns), values(&batch, ns))),
        before
    );
    assert_eq!(pin_count(&batch, id).unwrap(), u64::MAX);
    // No write failed, so the draft is still whole.
    draft.finish();
}

#[test]
fn loading_admits_the_whole_catalog_before_it_allocates_any() {
    let schema = schema();
    let storage = open();
    // A value and a combination far past every other id: loading gives each id below them a
    // free slot.
    apply(&storage, &new_catalog(&schema), u64::MAX, |draft, batch| {
        resolve(draft, batch, ["match", "curl", "a.com"]);
        let label = raw::label_record(b"h");
        batch
            .put(LABELS, label_key(HOST, 100_000).as_slice(), &label)
            .unwrap();
        let combo = raw::combo_record(&[1, 1, 0]);
        batch
            .put(COMBOS, combo_key(100_000).as_slice(), &combo)
            .unwrap();
    });
    let snapshot = storage.snapshot().unwrap();
    let fixed = new_catalog(&schema).inner.fixed_bytes();
    let mut ledger = Ledger::default();
    load_inner(&snapshot, &schema, u64::MAX, &mut ledger).unwrap();
    let needed = ledger.peak();
    assert_eq!(ledger.owned(), needed);

    // Exactly enough budget: everything loads, every id below the highest free, and the
    // version is exactly what loading held, its own structure included.
    let loaded = load_catalog(&snapshot, &schema, needed).unwrap();
    assert_eq!(loaded.label(HOST, 100_000), Some(&b"h"[..]));
    assert_eq!(labels(loaded.combo(100_000)), Some(vec![1, 1, 0]));
    let contents = loaded.contents();
    assert_eq!(contents.labels[usize::from(HOST.0)].gaps.len(), 99_999);
    assert_eq!(contents.combos.gaps.len(), 99_998);
    assert_eq!(loaded.bytes(), needed);

    // One byte less: refused by the budget, not as corruption, before anything past the
    // version's own structure is allocated.
    let mut ledger = Ledger::default();
    let error = load_inner(&snapshot, &schema, needed - 1, &mut ledger)
        .err()
        .unwrap();
    assert!(
        matches!(&error, LoadError::ReserveMemory { source }
            if (source.needed, source.available) == (needed - fixed, needed - fixed - 1)),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(ledger.allocated(), fixed);

    // The highest id, in a dictionary or in the combinations, is refused under a real budget.
    let limit = Profile::desktop().catalog;
    for (ns, key, value) in [
        (
            LABELS,
            label_key(TARGET, MAX_ID).as_slice().to_vec(),
            raw::label_record(b"t"),
        ),
        (
            COMBOS,
            combo_key(MAX_ID).as_slice().to_vec(),
            raw::combo_record(&[0, 0, 1]),
        ),
    ] {
        let storage = open();
        let mut batch = storage.batch().unwrap();
        batch.put(ns, &key, &value).unwrap();
        commit(batch);
        let mut ledger = Ledger::default();
        let error = load_inner(&storage.snapshot().unwrap(), &schema, limit, &mut ledger)
            .err()
            .unwrap();
        assert!(
            matches!(error, LoadError::ReserveMemory { .. }),
            "{error:?}"
        );
        assert_eq!(ledger.allocated(), fixed);
    }
}

// An empty catalog has nothing to admit: it loads under any budget, as a new one is made.
#[test]
fn an_empty_catalog_loads_under_any_budget() {
    let schema = schema();
    let storage = open();
    let loaded = load_catalog(&storage.snapshot().unwrap(), &schema, 1).unwrap();
    assert_eq!(loaded.bytes(), new_catalog(&schema).bytes());
    assert_eq!(loaded.contents(), new_catalog(&schema).contents());
}

// A profile change can shrink the budget below a stored catalog. Loading then fails before
// it copies what does not fit, instead of holding the catalog past the new budget.
#[test]
fn a_catalog_written_under_a_larger_limit_fails_to_load_under_a_smaller_one() {
    let schema = schema();
    let storage = open();
    let large = Profile::desktop().catalog;
    apply(&storage, &new_catalog(&schema), large, |draft, batch| {
        resolve(draft, batch, ["match", "curl", "a.com"]);
        let long = vec![b'x'; 2 * 1024 * 1024];
        assert_eq!(draft.resolve_label(batch, HOST, &long).unwrap(), 1);
    });
    let snapshot = storage.snapshot().unwrap();
    load_catalog(&snapshot, &schema, large).unwrap();

    let small = 64 * 1024;
    let mut ledger = Ledger::default();
    let error = load_inner(&snapshot, &schema, small, &mut ledger)
        .err()
        .unwrap();
    assert!(
        matches!(&error, LoadError::ReserveMemory { .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(
        ledger.peak() <= small,
        "loading held {} past {small}",
        ledger.peak()
    );
}

/// Codex's case: ten thousand values of 4,097 bytes, a page and 17 bytes each, reopened under
/// 64 KiB. Planning counts them and holds nothing per value, and loading fails on the budget
/// before it allocates anything past the version's own structure. Under a budget that holds
/// them, they load into one page of exactly their bytes.
#[test]
fn many_long_values_fail_to_load_under_a_small_budget_before_anything_is_held_for_them() {
    assert!(!std::mem::needs_drop::<dictionary::LoadPlan>());
    let schema = schema();
    let storage = open();
    let value = |id: u32| {
        let mut value = vec![0; 4_097];
        value[..4].copy_from_slice(&id.to_be_bytes());
        value
    };
    let mut batch = storage.batch().unwrap();
    for id in 1..=10_000 {
        let record = raw::label_record(&value(id));
        batch
            .put(LABELS, label_key(HOST, id).as_slice(), &record)
            .unwrap();
    }
    commit(batch);
    let snapshot = storage.snapshot().unwrap();
    let fixed = new_catalog(&schema).inner.fixed_bytes();

    let mut ledger = Ledger::default();
    let error = load_inner(&snapshot, &schema, 64 * 1024, &mut ledger)
        .err()
        .unwrap();
    assert!(
        matches!(&error, LoadError::ReserveMemory { source } if source.needed > 40_970_000),
        "{error:?}"
    );
    assert_eq!(ledger.allocated(), fixed);

    let loaded = load_catalog(&snapshot, &schema, u64::MAX).unwrap();
    let host = &loaded.inner.dictionaries[usize::from(HOST.0)];
    assert_eq!(host.heap_page_lens(), [40_970_000]);
    for id in [1, 4_321, 10_000] {
        assert_eq!(loaded.label(HOST, id), Some(&value(id)[..]));
    }
}

/// An edit of a draft, for [`written_and_loaded`].
type Edit = dyn Fn(&mut CatalogDraft, &mut MemoryBatch<'_>);

/// Applies `edits` in turn, each in a draft of its own under no limit, then loads the
/// result. Checks that loading held only what the loaded version holds, and that the loaded
/// version is no larger than the written one.
fn written_and_loaded(edits: &[&Edit]) -> (Catalog, Inner) {
    let schema = schema();
    let storage = open();
    let mut catalog = new_catalog(&schema);
    for edit in edits {
        catalog = apply(&storage, &catalog, u64::MAX, edit);
    }
    let mut ledger = Ledger::default();
    let snapshot = storage.snapshot().unwrap();
    let loaded = load_inner(&snapshot, &schema, u64::MAX, &mut ledger).unwrap();
    assert_eq!(loaded.contents(), catalog.contents());
    assert!(
        loaded.bytes() <= catalog.bytes(),
        "loaded {} past the written {}",
        loaded.bytes(),
        catalog.bytes()
    );
    assert_eq!(ledger.peak(), loaded.bytes());
    (catalog, loaded)
}

// A reused id moves its new value among the old ones, in id order. Packed into pages in that
// order, the values would take three pages where the written heap took two; loading packs
// them side by side instead.
#[test]
fn a_reused_id_does_not_grow_the_loaded_heap() {
    fn value(n: usize, len: usize) -> Vec<u8> {
        vec![n as u8; len]
    }
    written_and_loaded(&[
        &|draft, batch| {
            for (n, len) in [2040, 408, 1632, 2448, 1020].into_iter().enumerate() {
                let id = draft.resolve_label(batch, HOST, &value(n, len)).unwrap();
                assert_eq!(id, n as u32 + 1);
            }
        },
        &|draft, batch| draft.release_label(batch, HOST, 2).unwrap(),
        &|draft, batch| {
            assert_eq!(draft.resolve_label(batch, HOST, &value(9, 612)).unwrap(), 2);
        },
    ]);
}

// Releasing most values of a full table and inserting one more rebuilds it small, while the
// highest id stays: a table sized for the ids would be larger than the written one.
#[test]
fn a_loaded_table_is_sized_for_its_values_not_its_ids() {
    let (catalog, loaded) = written_and_loaded(&[
        &|draft, batch| {
            for n in 0..1536u16 {
                draft.resolve_label(batch, HOST, &n.to_be_bytes()).unwrap();
            }
        },
        &|draft, batch| {
            for id in 1..1536 {
                draft.release_label(batch, HOST, id).unwrap();
            }
        },
        &|draft, batch| {
            assert_eq!(draft.resolve_label(batch, HOST, b"new").unwrap(), 1);
        },
    ]);
    let host = usize::from(HOST.0);
    assert_eq!(catalog.inner.dictionaries[host].table_slots(), 8);
    assert_eq!(loaded.dictionaries[host].table_slots(), 8);
    assert_eq!(
        loaded.dictionaries[host].live_ids().collect::<Vec<_>>(),
        [1, 1536]
    );
}

// The review's counterexample: dimension 1 holds 50,000 short values and dimension 0 the
// largest value the write budget still admits. Loading reads dimension 0 first, so if it grew
// dimension 1's table as it went, the old and the new table would sit beside the large value,
// past the limit.
#[test]
fn a_catalog_filled_to_its_limit_loads_under_it() {
    let schema = schema();
    let storage = open();
    let limit = 8 * 1024 * 1024;
    let catalog = apply(&storage, &new_catalog(&schema), limit, |draft, batch| {
        for n in 0..50_000u32 {
            let id = draft
                .resolve_label(batch, PROCESS, &n.to_be_bytes())
                .unwrap();
            assert_eq!(id, n + 1);
        }
    });
    let mut written = None;
    let catalog = apply(&storage, &catalog, limit, |draft, batch| {
        // Past a page, a value costs its page and a constant.
        let probe = 8192;
        let constant = draft.value_cost(RULE, &vec![b'x'; probe]) - page_bytes::<u8>(probe);
        let room = draft.limit - draft.held() - draft.reserve;
        let largest = ((room - constant) / 8 * 8 - ARC_HEADER as u64) as usize;
        assert!(draft.value_cost(RULE, &vec![b'x'; largest + 1]) > room);
        let value = vec![b'x'; largest];
        assert_eq!(draft.resolve_label(batch, RULE, &value).unwrap(), 1);
        assert!(draft.peak() <= limit);
        written = Some(largest);
    });
    assert!(written.unwrap() > 6_000_000, "{written:?}");

    let mut ledger = Ledger::default();
    let loaded = load_inner(&storage.snapshot().unwrap(), &schema, limit, &mut ledger).unwrap();
    assert_eq!(loaded.contents(), catalog.contents());
    // Loading allocated everything once, the version's own structure included, and holds
    // no more than the version it reloads.
    assert_eq!(ledger.peak(), loaded.bytes());
    assert!(loaded.bytes() <= catalog.bytes());
    assert!(ledger.peak() <= limit);
}

/// Names for schemas with many dimensions.
const NAMES: [&str; 17] = [
    "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8", "d9", "d10", "d11", "d12", "d13", "d14",
    "d15", "d16",
];

/// A schema whose series has `dimensions` dimensions and no fold steps.
fn wide_schema(dimensions: usize) -> Schema {
    let mut schema = schema();
    schema.labels = NAMES[..dimensions]
        .iter()
        .map(|&name| LabelSpec { name })
        .collect();
    let series = schema.series.as_mut().unwrap();
    series.dimensions = (0..dimensions as u16).map(LabelId).collect();
    series.fold = Vec::new();
    schema
}

#[test]
fn sixteen_dimensions_fill_the_reverse_lookup_key() {
    let schema = wide_schema(16);
    schema.validate().unwrap();
    let values = [&b"v"[..]; 16];
    let storage = open();
    let catalog = apply(&storage, &new_catalog(&schema), u64::MAX, |draft, batch| {
        let resolved = draft.resolve_combo(batch, &values).unwrap();
        assert_eq!(resolved, ResolvedCombo { id: 1, fold: None });
    });
    let snapshot = storage.snapshot().unwrap();
    assert_eq!(
        keys(&snapshot, COMBO_INDEX),
        [index_key(&[1; 16]).as_slice()]
    );
    assert_eq!(index_key(&[1; 16]).as_slice().len(), 64);

    // A loaded catalog finds the combination through the stored reverse lookup.
    let loaded = load_catalog(&snapshot, &schema, u64::MAX).unwrap();
    assert_eq!(loaded.contents(), catalog.contents());
    let mut batch = storage.batch().unwrap();
    let mut draft = loaded
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    let resolved = draft.resolve_combo(&mut batch, &values).unwrap();
    assert_eq!(resolved, ResolvedCombo { id: 1, fold: None });
    assert_eq!(keys(&batch, COMBOS).len(), 1);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "17 series dimensions")]
fn more_dimensions_than_a_key_holds_trip_a_debug_assertion() {
    new_catalog(&wide_schema(17));
}

/// Fills a catalog under `limit` until the whole combination overflows.
fn fill(storage: &MemoryStorage, limit: u64) -> Catalog {
    let mut catalog = new_catalog(&schema());
    for round in 0.. {
        let mut overflowed = false;
        catalog = apply(storage, &catalog, limit, |draft, batch| {
            for n in 0..50 {
                let process = format!("p{round}-{n}");
                let target = format!("t{round}-{n}");
                let resolved = draft
                    .resolve_combo(batch, &[b"match", process.as_bytes(), target.as_bytes()])
                    .unwrap();
                overflowed |= resolved.fold == Some(Fold::Whole);
                assert!(draft.peak() <= limit);
            }
        });
        if overflowed {
            return catalog;
        }
    }
    unreachable!()
}

#[test]
fn a_full_catalog_can_still_release() {
    let storage = open();
    let limit = 64 * 1024;
    let full = fill(&storage, limit);
    assert!(
        full.inner.combos.live_ids().count() > 10,
        "{} combinations",
        full.inner.combos.live_ids().count()
    );
    // A release always fits in a fresh draft, within the limit. The combination has a target
    // of its own, so nothing else uses it.
    let (id, target) = full
        .inner
        .combos
        .live_ids()
        .map(|id| (id, full.combo(id).unwrap().nth(2).unwrap()))
        .filter(|&(_, target)| target != 0)
        .last()
        .unwrap();
    let mut peaks = Vec::new();
    let released = apply(&storage, &full, limit, |draft, batch| {
        let _ = draft.release_combo(batch, id).unwrap();
        peaks.push(draft.peak());
    });
    let released = apply(&storage, &released, limit, |draft, batch| {
        draft.release_label(batch, TARGET, target).unwrap();
        peaks.push(draft.peak());
    });
    assert!(
        peaks.iter().all(|&peak| peak <= limit),
        "{peaks:?} past {limit}"
    );
    assert_eq!(labels(released.combo(id)), None);
    assert_eq!(released.label(TARGET, target), None);
}

#[test]
fn a_release_without_room_changes_nothing() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            // Enough values for a table of several pages.
            for n in 0..1200 {
                draft
                    .resolve_label(batch, HOST, format!("h{n}").as_bytes())
                    .unwrap();
            }
        },
    );
    let mut batch = storage.batch().unwrap();
    // A limit below the base, as after a lower profile: releases may still use the reserve.
    let mut draft = catalog.draft(0, &crate::budget::test_account()).unwrap();
    let ceiling = catalog.bytes() + draft.reserve();
    let mut refused = None;
    for id in 1..=1200 {
        match draft.release_label(&mut batch, HOST, id) {
            Ok(()) => assert!(draft.peak() <= ceiling),
            Err(error) => {
                refused = Some((id, error));
                break;
            }
        }
    }
    let (id, error) = refused.expect("the reserve runs out");
    assert!(id > 1, "the first release always fits");
    assert!(
        matches!(&error, ReleaseError::ReserveRelease { source } if source.what == "catalog"),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(draft.label(HOST, id).is_some());
    assert!(values_contain(
        &batch,
        LABELS,
        format!("h{}", id - 1).as_bytes()
    ));
    // The draft stays usable.
    let released = draft.finish().0;
    commit(batch);
    assert_eq!(released.label(HOST, 1), None);
    assert_eq!(released.label(HOST, id), catalog.label(HOST, id));
}

#[derive(Clone, Debug)]
enum Op {
    Resolve([u8; 3]),
    Label(u8),
    /// This many new short values in this dimension, enough to grow its table.
    Bulk(u8, u16),
    Pin(u8),
    Unpin(u8),
    ReleaseCombo(u8),
    ReleaseLabel(u8, u8),
    Finish,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => any::<[u8; 3]>().prop_map(Op::Resolve),
        2 => any::<u8>().prop_map(Op::Label),
        1 => (0..4u8, 1..600u16).prop_map(|(dimension, count)| Op::Bulk(dimension, count)),
        2 => any::<u8>().prop_map(Op::Pin),
        1 => any::<u8>().prop_map(Op::Unpin),
        2 => any::<u8>().prop_map(Op::ReleaseCombo),
        2 => (0..4u8, any::<u8>()).prop_map(|(dimension, n)| Op::ReleaseLabel(dimension, n)),
        1 => Just(Op::Finish),
    ]
}

/// The `n`th value of a dimension: mostly short, some longer than a heap page.
fn value(dimension: usize, n: u8) -> Vec<u8> {
    let n = n % 24;
    if n % 11 == 10 {
        vec![b'a' + dimension as u8; 4000 + 100 * usize::from(n)]
    } else {
        format!("{dimension}:{n}").into_bytes()
    }
}

fn pick<T: Copy>(items: &[T], n: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[usize::from(n) % items.len()])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    // Under a small budget, no sequence of operations lets the peak pass the limit, every
    // resolution succeeds, and every combination round-trips; the ledger always matches
    // what the draft does not share. Loading under the same budget gives back the last
    // version, holding nothing but what it returns, which is no larger than that version.
    #[test]
    fn the_budget_bounds_every_sequence(
        limit in 8 * 1024u64..160 * 1024,
        ops in prop::collection::vec(op(), 1..250),
    ) {
        let schema = schema();
        let series = schema.series.clone().unwrap();
        let storage = open();
        let mut catalog = new_catalog(&schema);
        let mut batch = storage.batch().unwrap();
        let mut draft = catalog.draft(limit, &crate::budget::test_account()).unwrap();
        // Pin key → its combination, and combination → its keys.
        let mut pin_keys = BTreeMap::<Vec<u8>, u32>::new();
        let mut pins = BTreeMap::<u32, u64>::new();
        let mut next_key = 0u32;
        let mut bulk = 0u32;

        for op in ops {
            match op {
                Op::Resolve(ns) => {
                    let values: Vec<Vec<u8>> =
                        (0..3).map(|position| value(position, ns[position])).collect();
                    let refs: Vec<&[u8]> = values.iter().map(Vec::as_slice).collect();
                    let resolved = draft.resolve_combo(&mut batch, &refs).unwrap();
                    prop_assert_eq!(resolved.id == 0, resolved.fold == Some(Fold::Whole));
                    let combo: Vec<u32> = draft.combo(resolved.id).unwrap().collect();
                    for (position, &label) in combo.iter().enumerate() {
                        let dimension = series.dimensions[position];
                        if resolved.fold.is_some_and(|fold| fold.folds(&series, dimension)) {
                            prop_assert_eq!(label, 0);
                        } else {
                            prop_assert_eq!(draft.label(dimension, label), Some(&values[position][..]));
                        }
                    }
                }
                Op::Label(n) => {
                    let value = value(3, n);
                    let id = draft.resolve_label(&mut batch, HOST, &value).unwrap();
                    if id != 0 {
                        prop_assert_eq!(draft.label(HOST, id), Some(&value[..]));
                    }
                }
                Op::Bulk(dimension, count) => {
                    let label = LabelId(dimension.into());
                    for _ in 0..count {
                        bulk += 1;
                        let value = format!("bulk{bulk}");
                        let id = draft.resolve_label(&mut batch, label, value.as_bytes()).unwrap();
                        if id != 0 {
                            prop_assert_eq!(draft.label(label, id), Some(value.as_bytes()));
                        }
                    }
                }
                Op::Pin(n) => {
                    let live: Vec<u32> = iter::once(0).chain(draft.next.combos.live_ids()).collect();
                    let id = pick(&live, n).unwrap();
                    let used: Vec<&Vec<u8>> = pin_keys.keys().collect();
                    match pick(&used, n / 2).filter(|_| n % 4 == 0) {
                        // A key in use pins nothing more.
                        Some(key) => prop_assert!(!draft.pin(&mut batch, key, id).unwrap()),
                        None => {
                            next_key += 1;
                            let key = format!("key{next_key}").into_bytes();
                            prop_assert!(draft.pin(&mut batch, &key, id).unwrap());
                            pin_keys.insert(key, id);
                            if id != 0 {
                                *pins.entry(id).or_default() += 1;
                            }
                        }
                    }
                }
                Op::Unpin(n) => {
                    let used: Vec<&Vec<u8>> = pin_keys.keys().collect();
                    if let Some(key) = pick(&used, n).cloned() {
                        prop_assert!(draft.unpin(&mut batch, &key).unwrap().is_some());
                        let id = pin_keys.remove(&key).unwrap();
                        if id != 0 {
                            let count = pins.get_mut(&id).unwrap();
                            *count -= 1;
                            if *count == 0 {
                                pins.remove(&id);
                            }
                        }
                    }
                }
                Op::ReleaseCombo(n) => {
                    let free: Vec<u32> = draft
                        .next
                        .combos
                        .live_ids()
                        .filter(|id| !pins.contains_key(id))
                        .collect();
                    if let Some(id) = pick(&free, n) {
                        match draft.release_combo(&mut batch, id) {
                            Ok(_) => prop_assert!(draft.combo(id).is_none()),
                            Err(ReleaseError::ReserveRelease { .. }) => {
                                prop_assert!(draft.combo(id).is_some());
                            }
                            Err(error) => panic!("{error:?}"),
                        }
                    }
                }
                Op::ReleaseLabel(dimension, n) => {
                    let label = LabelId(dimension.into());
                    let position = series.dimensions.iter().position(|&d| d == label);
                    let dictionary = draft.next.dictionary(label);
                    let unused: Vec<u32> = dictionary
                        .live_ids()
                        .filter(|&id| {
                            position.is_none_or(|position| {
                                draft.next.combos.user_of(position, id).is_none()
                            })
                        })
                        .collect();
                    if let Some(id) = pick(&unused, n) {
                        match draft.release_label(&mut batch, label, id) {
                            Ok(()) => prop_assert_eq!(draft.label(label, id), None),
                            Err(ReleaseError::ReserveRelease { .. }) => {
                                prop_assert!(draft.label(label, id).is_some());
                            }
                            Err(error) => panic!("{error:?}"),
                        }
                    }
                }
                Op::Finish => {
                    let peak = draft.peak();
                    catalog = draft.finish().0;
                    // The new version is part of what the peak counted.
                    prop_assert!(catalog.bytes() <= peak);
                    commit(batch);
                    batch = storage.batch().unwrap();
                    draft = catalog.draft(limit, &crate::budget::test_account()).unwrap();
                }
            }
            prop_assert!(draft.peak() <= limit, "peak {} past {limit}", draft.peak());
            prop_assert_eq!(draft.owned(), draft.unshared());
        }

        catalog = draft.finish().0;
        commit(batch);
        let snapshot = storage.snapshot().unwrap();
        let mut ledger = Ledger::default();
        let loaded = load_inner(&snapshot, &schema, limit, &mut ledger).unwrap();
        prop_assert_eq!(loaded.contents(), catalog.contents());
        prop_assert_eq!(ledger.peak(), loaded.bytes());
        prop_assert!(loaded.bytes() <= catalog.bytes(), "loaded {} past {}", loaded.bytes(), catalog.bytes());
        for id in catalog.inner.combos.live_ids() {
            prop_assert_eq!(pin_count(&snapshot, id).unwrap(), pins.get(&id).copied().unwrap_or(0));
        }
        for (key, &id) in &pin_keys {
            prop_assert_eq!(pinned_combo(&snapshot, key).unwrap(), Some(id));
        }
        prop_assert_eq!(keys(&snapshot, PIN_KEYS).len(), pin_keys.len());
        // Each value counts the live combinations that use it, and only a used one has a
        // record.
        let live = catalog.live_combos();
        let mut used = 0;
        for (position, &dimension) in series.dimensions.iter().enumerate() {
            for (id, _) in catalog.live_labels(dimension) {
                let users = live.iter().filter(|(_, labels)| labels[position] == id).count();
                prop_assert_eq!(label_uses(&snapshot, dimension, id).unwrap(), users as u64);
                used += usize::from(users > 0);
            }
        }
        prop_assert_eq!(keys(&snapshot, LABEL_USES).len(), used);

        // Under a smaller limit, the catalog loads whole or not at all, and loading never
        // holds more than that limit.
        let small = limit / 2;
        let mut ledger = Ledger::default();
        match load_inner(&snapshot, &schema, small, &mut ledger) {
            Ok(inner) => prop_assert_eq!(inner.contents(), catalog.contents()),
            Err(error) => prop_assert_eq!(error.kind(), ErrorKind::Local, "{:?}", error),
        }
        prop_assert!(ledger.peak() <= small, "loading held {} past {small}", ledger.peak());
    }
}

/// The first release may fail on staging, but neither storage nor the draft changes.
#[test]
fn a_staging_refusal_of_the_first_release_changes_nothing() {
    let storage = open();
    let catalog = apply(
        &storage,
        &new_catalog(&schema()),
        u64::MAX,
        |draft, batch| {
            assert_eq!(resolve(draft, batch, ["match", "curl", "target"]), 1);
        },
    );
    let cost = {
        let draft = catalog
            .draft(u64::MAX, &crate::budget::test_account())
            .unwrap();
        draft.next.combos.release_cost(1)
    };
    assert!(cost > 0);
    let budget = crate::budget::MemoryBudget::new(catalog.draft_bytes() + cost - 1);
    let account =
        MemoryAccount::new(&budget, "staging", crate::budget::MemoryPlan::default()).unwrap();
    let mut draft = catalog.draft(u64::MAX, &account).unwrap();
    let mut batch = storage.batch().unwrap();
    let error = draft.release_combo(&mut batch, 1).unwrap_err();
    assert!(
        matches!(error, ReleaseError::StageRelease { .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(draft.next.contents(), catalog.contents());
    assert_eq!(draft.ledger.allocated(), catalog.draft_bytes());
    assert_eq!(budget.reserved(), catalog.draft_bytes());
    assert_eq!(
        load_catalog(&batch, &schema(), u64::MAX)
            .unwrap()
            .contents(),
        catalog.contents()
    );
    for (dimension, id) in [RULE, PROCESS, TARGET]
        .into_iter()
        .zip(catalog.combo(1).unwrap())
    {
        assert_eq!(label_uses(&batch, dimension, id).unwrap(), 1);
    }
    let (unchanged, charge) = draft.finish();
    assert!(Arc::ptr_eq(&unchanged.inner, &catalog.inner));
    assert_eq!(unchanged.generation(), catalog.generation());
    drop(charge);
    assert_eq!(budget.reserved(), 0);
}

/// A label's data-sized wire output is admitted exactly before its storage write.
#[test]
fn label_serialization_is_exact_and_refusal_writes_nothing() {
    use crate::budget::{MemoryAccount, MemoryBudget, MemoryPlan};

    let disk = MemoryDisk::new();
    let storage = disk.open().unwrap();
    let value = vec![b'x'; 130_000];
    let expected = raw::label_record(&value);
    for limit in [expected.len() - 1, expected.len()] {
        let budget = MemoryBudget::new(limit as u64);
        let account = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        let mut batch = storage.batch().unwrap();
        let result = persist::put_label(&mut batch, LabelId(0), 1, &value, &account);
        if limit < expected.len() {
            assert!(matches!(result, Err(ResolveError::StageResolution { .. })));
            assert_eq!(account.peak_bytes(), 0);
            batch
                .get(LABELS, label_key(LabelId(0), 1).as_slice(), |stored| {
                    assert!(stored.is_none());
                })
                .unwrap();
        } else {
            result.unwrap();
            assert_eq!(account.peak_bytes(), expected.len() as u64);
            batch
                .get(LABELS, label_key(LabelId(0), 1).as_slice(), |stored| {
                    assert_eq!(stored, Some(expected.as_slice()));
                })
                .unwrap();
        }
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }
}
